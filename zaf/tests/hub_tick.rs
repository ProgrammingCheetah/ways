//! `POST /api/tick` through the built binary: the hub's tick is `zaf spawn --once --no-wait` run in
//! the project's root, so a project with one ready task and a fake runner has it started, and the
//! answer names it. The hub is served in this process, over the real binary as its ticker.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Project, outcome};
use zaf::app::hub::controls::Controls;
use zaf::app::hub::guard::{Guard, KEY_HEADER};
use zaf::app::hub::{Dashboard, Page};
use zaf::app::projects::Projects;
use zaf::cli::hub::tick::Subprocess;

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [phases]\nTask = [\"implement\", \"review\"]\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n";

const TASK_BODY: &str = "Goal: Members export their reading list as CSV.\n\
    Owned paths: src/export/**\n\
    Interfaces: none\n\
    Design pages: none\n\
    Architecture: app over ports\n\
    Acceptance:\n  - an empty list exports the header row only\n\
    Tests: beside the use case\n\
    Security trigger: no\n\
    Model: sonnet\n\
    Size: S\n\
    Blocked by: none\n";

/// Writes `.zaf/actions/<name>` with `metadata` under its description, then `body`.
fn action(project: &Project, name: &str, metadata: &str, body: &str) {
    let dir = project.root.join(".zaf/actions");
    fs::create_dir_all(&dir).expect("actions");
    let file = dir.join(name);
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: {name}\n# zaf 1: failed\n{metadata}{body}"),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// A project whose task 4 (of epic 1) is ready at `implement`, an action that is the fake runner:
/// it leaves the file `started` in the project's root. The project is on its zaf home's list, and
/// its profiles are current, as a tick needs.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let ticket = serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "ready"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
        "issueType": {"name": "Task"},
    });
    project.on("issue view 4 *", &ticket.to_string());
    project.on(
        "issue list --label task --state open *",
        &format!("[{ticket}]"),
    );
    project.on("issue list --label epic *", "[]");
    project.on("issue list --label task --state all *", "[]");
    project.on("issue list --label claimed *", "[]");
    project.on("issue list --label question *", "[]");
    // Whatever else the tick reads is empty.
    project.on("*", "[]");
    project.provide_slice_actions();
    let root = project.root.display().to_string();
    action(
        &project,
        "implement",
        "# zaf kind: model\n# zaf ends: exit\n",
        &format!("cd '{root}'\ntouch started\n"),
    );
    action(
        &project,
        "review",
        "# zaf kind: wait\n# zaf ends: review-posted\n",
        "true\n",
    );
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// A `zaf` that runs the built binary as the project's runs do: the fake `gh` first on the path
/// and the project's zaf home, which the hub's own environment does not carry.
fn wrapper(project: &Project) -> std::path::PathBuf {
    let command = project.command_in(&project.root, &[]);
    let mut script = String::from("#!/bin/sh\n");
    for (name, value) in command.get_envs() {
        match value {
            Some(value) => script.push_str(&format!(
                "export {}='{}'\n",
                name.to_string_lossy(),
                value.to_string_lossy()
            )),
            None => script.push_str(&format!("unset {}\n", name.to_string_lossy())),
        }
    }
    script.push_str(&format!("exec '{}' \"$@\"\n", env!("CARGO_BIN_EXE_zaf")));
    let file = project.temp.path().join("zaf-as-the-project-runs-it");
    fs::write(&file, script).expect("wrapper");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    file
}

/// A hub serving the tick over `project`'s zaf home, and what a request to it needs.
struct Hub {
    port: u16,
    key: String,
    _page: Page,
}

fn serve(project: &Project) -> Hub {
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://zaf/").expect("a link");
    let key = link.rsplit_once("key=").expect("a key").1.to_owned();
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let controls = Controls::new(Some(project.home.clone()))
        .with_tick(Arc::new(Subprocess::of(wrapper(project))));
    let page = dashboard.start_with(
        "frame\n",
        Arc::new(|| Err("not read in this test".to_owned())),
        guard,
        controls,
    );
    Hub {
        port,
        key,
        _page: page,
    }
}

impl Hub {
    /// A `POST /api/tick` with `body`, with the key when `keyed`: the whole answer.
    fn tick(&self, keyed: bool, body: &serde_json::Value) -> String {
        let header = if keyed {
            format!("{KEY_HEADER}: {}\r\n", self.key)
        } else {
            String::new()
        };
        let body = body.to_string();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .expect("timeout");
        write!(
            stream,
            "POST /api/tick HTTP/1.1\r\nHost: zaf\r\n{header}Content-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("writes");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("reads");
        answer
    }
}

/// Waits until `file` exists; a test that waits for nothing would hide a hang.
fn await_file(file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !file.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            file.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_tick_starts_the_ready_task_and_names_it() {
    let project = project();
    let hub = serve(&project);
    let root = project.root.canonicalize().expect("root");

    let answer = hub.tick(
        true,
        &serde_json::json!({ "project": root, "version": "0.3.0" }),
    );

    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    let (_, body) = answer.split_once("\r\n\r\n").expect("a body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON");
    let said = body["tick"].to_string();
    assert!(said.contains("started implement #4"), "{said}");
    assert_eq!(body["version"], "0.3.0");
    // The fake runner ran, in the project's root.
    await_file(&project.root.join("started"));
    // And the version stayed on the project's entry.
    let listed = Projects::read(&project.home);
    assert_eq!(listed[0].version.as_deref(), Some("0.3.0"));
}

#[test]
fn without_the_key_nothing_is_ticked() {
    let project = project();
    let hub = serve(&project);
    let root = project.root.canonicalize().expect("root");

    let answer = hub.tick(
        false,
        &serde_json::json!({ "project": root, "version": "0.3.0" }),
    );

    assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!project.root.join("started").exists(), "the runner ran");
    assert_eq!(Projects::read(&project.home)[0].version, None);
}
