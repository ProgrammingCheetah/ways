//! `POST /api/watch` through the built binary: the hub's watch ticks with `zaf spawn --once` run in
//! the project's root while the hub itself holds the project's scheduler lock, so a project with one
//! ready task and a fake runner has it started by the watch's first tick, and a stop ends the watch.
//! The hub is served in this process, over the real binary as the watch's tick.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use common::{Project, jj, outcome, run, write};
use zaf::app::hub::controls::Controls;
use zaf::app::hub::guard::{Guard, KEY_HEADER};
use zaf::app::hub::{Dashboard, Page};
use zaf::app::spawn::record::watch_running;
use zaf::app::spawn::watch::Watches;
use zaf::cli::hub::watch::Hub as Watching;
use zaf::domain::DocumentSignature;

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [vcs]\nkind = \"jj\"\n\n\
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

/// Makes the project a jj repository on a bare remote, with the shipped `base-open` and
/// `papers-commit` actions and the release folder of 0.3.0 signed: the build start the hub's
/// watch runs first needs them.
fn open_the_papers(project: &Project) {
    let root = &project.root;
    let temp = project.temp.path();
    jj(temp, &["git", "init", "repo"]);
    jj(
        root,
        &["config", "set", "--repo", "user.name", "Test Author"],
    );
    jj(
        root,
        &["config", "set", "--repo", "user.email", "a@example.com"],
    );
    write(root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    for name in ["base-open", "papers-commit"] {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the crate sits in the repository")
            .join("plugins/teams/actions")
            .join(name);
        let file = root.join(".zaf/actions").join(name);
        fs::write(&file, fs::read_to_string(shipped).expect("shipped")).expect("action");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).expect("mode");
    }
    run(temp, "git", &["init", "-q", "--bare", "remote.git"]);
    let bare = temp.join("remote.git");
    jj(
        root,
        &["git", "remote", "add", "origin", &bare.to_string_lossy()],
    );
    jj(root, &["commit", "-m", "Start the project"]);
    jj(root, &["bookmark", "create", "main", "-r", "@-"]);
    jj(root, &["git", "push", "--bookmark", "main"]);
    for name in ["objectives.md", "design.md"] {
        write(
            root,
            &format!(".teams/releases/0.3.0/{name}"),
            &DocumentSignature::sign("# Papers\n", UNIX_EPOCH),
        );
    }
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
    // The forge's default branch, which `base-open` cuts the integration branch from.
    project.on("api *", "main\n");
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
    // `base-open` says `zaf forge default-branch`, and the hub starts the build in this very
    // process: the built binary must come first on its path, never an installed one.
    // SAFETY: this file's one test sets it before it starts any thread.
    unsafe {
        std::env::set_var("PATH", common::path_with_zaf(&[&common::fixtures()]));
        std::env::set_var("FAKE_GH_DIR", project.temp.path().join("gh-scene"));
    }
    // The hub runs the actions in this process, so the running binary is the test's, not zaf.
    common::no_zaf_link_folder(&project.home);
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    open_the_papers(&project);
    project
}

/// A `zaf` that runs the built binary as the project's runs do: the fake `gh` first on the path
/// and the project's zaf home, which the hub's own environment does not carry.
fn wrapper(project: &Project) -> PathBuf {
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

/// A hub serving the watch control over `project`'s zaf home, and what a request to it needs.
struct Served {
    port: u16,
    key: String,
    watches: Option<Watches>,
    _page: Page,
}

fn serve(project: &Project) -> Served {
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://zaf/").expect("a link");
    let key = link.rsplit_once("key=").expect("a key").1.to_owned();
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let watches = Watches::new();
    let watching = Watching::of(
        watches.handle(),
        Some(project.home.clone()),
        wrapper(project),
    );
    let controls = Controls::new(Some(project.home.clone())).with_watch(Arc::new(watching));
    let page = dashboard.start_with(
        "frame\n",
        Arc::new(|| Err("not read in this test".to_owned())),
        guard,
        controls,
    );
    Served {
        port,
        key,
        watches: Some(watches),
        _page: page,
    }
}

impl Served {
    /// A `POST /api/watch` with `body` and the key: the whole answer.
    fn watch(&self, body: &serde_json::Value) -> String {
        let body = body.to_string();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .expect("timeout");
        write!(
            stream,
            "POST /api/watch HTTP/1.1\r\nHost: zaf\r\n{KEY_HEADER}: {}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            self.key,
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
    let deadline = Instant::now() + Duration::from_secs(30);
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
fn the_watch_ticks_at_once_through_the_verb_and_a_stop_ends_it() {
    let project = project();
    let mut hub = serve(&project);
    let root = project.root.canonicalize().expect("root");
    let state = root.join(".zaf");

    let answer = hub.watch(&serde_json::json!({
        "project": root, "action": "start", "version": "0.3.0"
    }));

    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(watch_running(&state), "the hub holds the project's lock");
    // The first tick is the verb run in the project's root, which the hub's lock did not turn
    // into an ask for a tick: it started the ready task.
    await_file(&root.join("started"));

    let again = hub.watch(&serde_json::json!({
        "project": root, "action": "start", "version": "0.3.0"
    }));
    assert!(again.starts_with("HTTP/1.1 409 Conflict"), "{again}");

    let stopped = hub.watch(&serde_json::json!({ "project": root, "action": "stop" }));
    assert!(stopped.starts_with("HTTP/1.1 200 OK"), "{stopped}");
    assert!(!watch_running(&state), "stopped");

    // A second start, ended with the hub.
    let restarted = hub.watch(&serde_json::json!({ "project": root, "action": "start" }));
    assert!(restarted.starts_with("HTTP/1.1 200 OK"), "{restarted}");
    assert!(watch_running(&state), "the remembered version serves");
    drop(hub.watches.take());
    assert!(!watch_running(&state), "the watch ended with the hub");
}
