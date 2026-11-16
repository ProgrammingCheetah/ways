//! The tick's `zaf`-kind step, through the binary (docs/zaf-actions.md §5.1, step 3): `zaf spawn
//! --once --version <v>` runs a task's ready `zaf`-kind action in its own process under that
//! version, whatever its environment holds. A `target: task` action whose branch pattern names
//! `{version}` gets its slice, and its body gets `ZAF_VERSION`, for the `zaf do` calls it makes.
//! An inline run reads as running from the moment its folder opens, and two ticks that overlap
//! never run the same action twice: the gate is asked again under `spawn.lock` before the folder
//! opens.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [phases]\nTask = [\"merge\"]\n";

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

/// A project with current profiles whose task 7 (of epic 1) is claimed and stands at `merge`, a
/// `zaf`-kind `target: task` action that writes the version and the slice it was given to
/// `merged`. The default task branch pattern, `feature/{version}-{epic}-{task}`, names the version.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    let ticket = serde_json::json!({
        "number": 7,
        "title": "Export the list",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "claimed"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [{"body": "claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}],
        "issueType": {"name": "Task"},
    });
    project
        .on(
            "issue list --label task --state open *",
            &format!("[{ticket}]"),
        )
        .on("issue list --label claimed *", r#"[{"number":7}]"#)
        .on("issue view 7 *", &ticket.to_string())
        .on("*", "[]");
    project.provide_slice_actions();
    let dir = project.root.join(".zaf/actions");
    let file = dir.join("merge");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
         # zaf kind: zaf\n# zaf ends: merged\n# zaf target: task\n\
         printf '%s %s\\n' \"$ZAF_VERSION\" \"$ZAF_SLICE\" > \"$ZAF_ROOT/merged\"\n",
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

#[test]
fn the_tick_runs_its_zaf_action_under_its_own_version() {
    let project = project();

    // `Project` removes `ZAF_VERSION` from every run, as a tick started by hand or by
    // `zaf spawn --watch` has none: the version is the flag's alone.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));

    assert!(!stderr.contains("NOT_RUN"), "{stdout}{stderr}");
    assert!(stdout.contains("ran merge #7: exit 0"), "{stdout}{stderr}");
    assert_eq!(code, 0, "{stdout}{stderr}");
    let merged = fs::read_to_string(project.root.join("merged")).expect("the action ran");
    assert_eq!(merged, "0.1 feature/0.1-1-7\n");
}

#[test]
fn an_inline_action_leaves_a_run_record() {
    let project = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stderr.contains("not recorded"), "{stderr}");
    let folders: Vec<_> = fs::read_dir(project.root.join(".zaf/phases"))
        .expect("the phase run folders")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(folders.len(), 1, "{folders:?}");
    let run = &folders[0];
    let read = |name: &str| fs::read_to_string(run.join(name)).ok();
    assert_eq!(read("action").as_deref(), Some("merge"));
    assert_eq!(read("task").as_deref(), Some("7"));
    assert_eq!(read("size").as_deref(), Some("S"));
    let instance = read("instance").expect("instance");
    assert!(
        instance.starts_with("merge instance ") && instance.ends_with(" on #1"),
        "{instance}"
    );
    // No pull request is recorded: no head at the start, none left.
    assert_eq!(read("head").as_deref(), Some(""));
    assert_eq!(read("head-end").as_deref(), Some(""));
    // A step of the tick's own process: no process of its own to record.
    assert_eq!(read("pid"), None);
    // It passed, so `zaf do` would have printed nothing; the exit comes last.
    assert_eq!(read("output").as_deref(), Some(""));
    assert_eq!(read("exit").as_deref(), Some("0\n"));
}

#[test]
fn a_question_asked_inside_an_inline_action_is_recorded_in_its_run() {
    let mut project = project();
    // The merge asks a question on its task as it runs, then passes.
    let file = project.root.join(".zaf/actions/merge");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
             # zaf kind: zaf\n# zaf ends: merged\n# zaf target: task\n\
             cd '{root}'\n\
             zaf board question 7 'Squash or merge?' > asked 2>&1\n",
            root = project.root.display()
        ),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    project
        .env("PATH", &common::path_with_zaf(&[&common::fixtures()]))
        .on_first(
            "issue create *",
            "https://github.com/example/tracker/issues/30\n",
        )
        .on_first("issue edit *", "");

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("ran merge #7: exit 0"), "{stdout}{stderr}");
    let asked = fs::read_to_string(project.root.join("asked")).expect("asked");
    assert_eq!(asked, "question #30 on #7\n");
    let folders: Vec<_> = fs::read_dir(project.root.join(".zaf/phases"))
        .expect("the phase run folders")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(folders.len(), 1, "{folders:?}");
    // The inline run's folder holds the question it asked, as a launched phase's does.
    assert_eq!(
        fs::read_to_string(folders[0].join("questions")).expect("questions"),
        "30\n"
    );
}

/// The project, whose `merge` adds a line to `runs`, says it started in `started` and waits for
/// `release` (twenty seconds at most, so a test that fails never hangs).
fn project_waiting() -> Project {
    let project = project();
    let file = project.root.join(".zaf/actions/merge");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
         # zaf kind: zaf\n# zaf ends: merged\n# zaf target: task\n\
         cd \"$ZAF_ROOT\"\necho run >> runs\ntouch started\n\
         i=0\nwhile [ ! -f release ] && [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done\n",
    )
    .expect("action");
    project
}

/// Starts `zaf spawn --once --version 0.1` without waiting for it.
fn start_tick(project: &Project) -> Child {
    project
        .command_in(&project.root, &["spawn", "--once", "--version", "0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start a tick")
}

/// Waits up to twenty seconds for `file`.
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

/// Waits for `child` to end, twenty seconds at most, killing it past that; gives its exit code
/// and output.
fn finish(mut child: Child) -> (i32, String, String) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().expect("try_wait").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().expect("output");
    outcome(&output)
}

/// The phase run folders.
fn run_folders(project: &Project) -> Vec<PathBuf> {
    fs::read_dir(project.root.join(".zaf/phases"))
        .map(|entries| entries.map(|entry| entry.expect("entry").path()).collect())
        .unwrap_or_default()
}

/// Where the dry run places task 7 now: `lane`, `running`, `waiting` or `done`.
fn place_of_seven(project: &Project) -> String {
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["--json", "spawn", "--once", "--dry-run", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let tickets = json["placement"]["tickets"].as_array().expect("tickets");
    let seven = tickets
        .iter()
        .find(|ticket| ticket["ticket"] == 7)
        .expect("task 7");
    seven["place"].as_str().expect("a place").to_owned()
}

#[test]
fn an_inline_run_reads_as_running_while_it_runs() {
    let project = project_waiting();
    assert_eq!(place_of_seven(&project), "lane");

    let tick = start_tick(&project);
    await_file(&project.root.join("started"));

    // The tick's own process runs it: no `pid` of its own, the `inline` mark names the tick.
    let folders = run_folders(&project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    assert!(folders[0].join("inline").exists());
    assert!(!folders[0].join("pid").exists());
    assert_eq!(place_of_seven(&project), "running");

    fs::write(project.root.join("release"), "").expect("release");
    let (code, stdout, stderr) = finish(tick);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("ran merge #7: exit 0"), "{stdout}{stderr}");
    assert_ne!(place_of_seven(&project), "running");
}

/// Adds task 8 to the open tickets, of a type with no phases that carries `blocked` and waits on
/// nothing, so each tick's label sync sets it `ready`: the one board write between a tick's read
/// of the facts and its steps. A `gh` first on the search path holds that write, for a tick run
/// with `WHO=second` alone, until `go` is there (twenty seconds at most), and marks `held` as it
/// begins to.
fn hold_second_after_its_read(project: &mut Project) {
    let seven = serde_json::json!({
        "number": 7,
        "title": "Export the list",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "claimed"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [{"body": "claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}],
        "issueType": {"name": "Task"},
    });
    let eight = serde_json::json!({
        "number": 8,
        "title": "Fix the header",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "blocked"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
        "issueType": {"name": "Bug"},
    });
    project
        .on_first(
            "issue list --label task --state open *",
            &format!("[{seven},{eight}]"),
        )
        .on_first("issue view 8 *", &eight.to_string());
    let wrap = project.temp.path().join("gh-hold");
    fs::create_dir_all(&wrap).expect("wrap");
    let root = project.root.display().to_string();
    let file = wrap.join("gh");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'issue edit 8 '*)\n\
             if [ \"$WHO\" = second ]; then\n\
             touch '{root}/held'\n\
             i=0\nwhile [ ! -f '{root}/go' ] && [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done\n\
             fi ;;\n\
             esac\n\
             exec '{fake}' \"$@\"\n",
            fake = common::fixtures().join("gh").display()
        ),
    )
    .expect("a gh that holds");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    project.env(
        "PATH",
        &common::path_with_zaf(&[&wrap, &common::fixtures()]),
    );
}

#[test]
fn two_ticks_never_run_the_same_inline_action_twice() {
    let mut project = project_waiting();
    hold_second_after_its_read(&mut project);

    // The second tick reads the lane, then is held past its read and before its steps.
    let second = project
        .command_in(&project.root, &["spawn", "--once", "--version", "0.1"])
        .env("WHO", "second")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the second tick");
    await_file(&project.root.join("held"));
    // The first reads the same lane, opens its run and starts the action.
    let first = start_tick(&project);
    await_file(&project.root.join("started"));
    // Only now does the second reach its step, with the lane it read before the run began.
    fs::write(project.root.join("go"), "").expect("go");
    let (second_code, second_out, second_err) = finish(second);
    fs::write(project.root.join("release"), "").expect("release");
    let (first_code, first_out, first_err) = finish(first);

    let all = format!("{first_out}{first_err}\n---\n{second_out}{second_err}");
    assert_eq!(first_code, 0, "{all}");
    assert_eq!(second_code, 0, "{all}");
    let runs = fs::read_to_string(project.root.join("runs")).expect("runs");
    assert_eq!(runs.lines().count(), 1, "the action ran once: {all}");
    assert_eq!(run_folders(&project).len(), 1, "{all}");
    assert!(first_out.contains("ran merge #7: exit 0"), "{all}");
    // The second asked the gate again under the lock, found the first's run, and ran nothing.
    assert!(!second_out.contains("ran merge #7: exit 0"), "{all}");
    assert!(second_err.contains("NOT_RUN merge on #7"), "{all}");
}

#[test]
fn an_inline_run_whose_tick_died_still_reads_as_running_while_its_action_lives() {
    let project = project_waiting();
    let mut tick = start_tick(&project);
    await_file(&project.root.join("started"));
    let folders = run_folders(&project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    // The action's own process is recorded as soon as it starts.
    await_file(&folders[0].join("inline-body"));

    // The tick dies; the action, in a process group of its own, goes on.
    tick.kill().expect("kill the tick");
    tick.wait().expect("the tick ended");
    assert_eq!(place_of_seven(&project), "running");
    // So a tick now leaves the ticket alone: the action does not run a second time.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let runs = fs::read_to_string(project.root.join("runs")).expect("runs");
    assert_eq!(runs.lines().count(), 1, "{stdout}{stderr}");
    assert_eq!(run_folders(&project).len(), 1, "{stdout}{stderr}");

    // Once the action has ended, nothing runs: the run has ended with no exit code.
    fs::write(project.root.join("release"), "").expect("release");
    let deadline = Instant::now() + Duration::from_secs(20);
    while place_of_seven(&project) == "running" {
        assert!(Instant::now() < deadline, "it reads running for ever");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!folders[0].join("exit").exists());
}
