//! `zaf spawn --once --no-wait`, through the binary: while another tick holds `spawn.lock` it
//! exits 0 at once, doing nothing (that tick reads the same forge); plain `--once` keeps waiting
//! for the lock; and `zaf spawn` with neither flag still refuses.

mod common;

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n";

/// A project with current profiles, an empty board and a `pre-tick` hook that marks each tick.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let hooks = project.root.join(".zaf/actions/hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let file = hooks.join("pre-tick");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\ntouch ticked\n",
    )
    .expect("hook");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    project
}

/// Takes `spawn.lock` as a running tick does, until the returned file is dropped.
fn hold_lock(project: &Project) -> File {
    let state = project.root.join(".zaf");
    fs::create_dir_all(&state).expect("state dir");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("spawn.lock"))
        .expect("lock file");
    file.lock().expect("lock");
    file
}

/// Starts `zaf` with `args` without waiting for it.
fn start(project: &Project, args: &[&str]) -> Child {
    project
        .command_in(&project.root, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start zaf")
}

/// Waits up to `within` for `child` to end; `None` when it is still running (and is then killed).
fn ended_within(mut child: Child, within: Duration) -> Option<std::process::Output> {
    let deadline = Instant::now() + within;
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return Some(child.wait_with_output().expect("output"));
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill");
            child.wait().expect("wait");
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_held_lock_is_not_waited_on() {
    let project = project();
    let _held = hold_lock(&project);
    let before = project.gh_calls().len();

    let child = start(
        &project,
        &["spawn", "--once", "--no-wait", "--version", "0.3.0"],
    );
    let output = ended_within(child, Duration::from_secs(10))
        .expect("--no-wait returned while the lock was held");

    let (code, stdout, stderr) = outcome(&output);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    // Nothing ran: no hook, no forge read, no plan.
    assert!(!project.root.join("ticked").exists(), "the pre-tick ran");
    assert_eq!(project.gh_calls().len(), before, "no forge read");
    assert!(!stdout.contains("slots"), "{stdout}");
}

#[test]
fn a_free_lock_is_taken_by_no_wait() {
    let project = project();

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["spawn", "--once", "--no-wait", "--version", "0.3.0"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("slots 1 of 1"), "the tick ran: {stdout}");
    assert!(project.root.join("ticked").exists());
}

#[test]
fn plain_once_still_waits() {
    let project = project();
    let held = hold_lock(&project);
    let before = project.gh_calls().len();

    let mut child = start(&project, &["spawn", "--once", "--version", "0.3.0"]);
    // Long enough for a tick that did not wait to have ended.
    std::thread::sleep(Duration::from_secs(1));
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "plain --once ended while the lock was held"
    );
    // The plan reads the forge, and is made under the lock.
    assert_eq!(project.gh_calls().len(), before, "read before the lock");

    drop(held);
    let output = ended_within(child, Duration::from_secs(20)).expect("it never took the lock");

    let (code, stdout, stderr) = outcome(&output);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("slots 1 of 1"), "{stdout}");
    assert!(project.root.join("ticked").exists());
}

#[test]
fn neither_flag_still_refuses() {
    let project = project();

    let (code, _, stderr) = outcome(&project.zaf(&["spawn"]));

    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("--once"), "{stderr}");
}

#[test]
fn no_wait_needs_once() {
    let project = project();

    let (code, _, stderr) = outcome(&project.zaf(&["spawn", "--no-wait", "--version", "0.3.0"]));

    assert_eq!(code, 2, "{stderr}");
}
