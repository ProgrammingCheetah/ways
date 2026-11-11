//! `zaf spawn --watch`, through the binary: a tick at once and then each `[spawn] tick_minutes`
//! until it is stopped; a tick that fails is logged and the loop goes on; a held `spawn.lock`
//! skips the tick; `tick_minutes = 0` fails the load; and `zaf spawn` with neither flag still
//! refuses. The interval itself is a minute at least, so the loop's pacing is checked on the
//! loop (`app::spawn::watch`); here each test stops the watch after its first tick.

mod common;

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [spawn]\ntick_minutes = 1\n";

/// A project with current profiles, an empty board and a `pre-tick` hook that marks each tick and
/// exits with `hook_exit`.
fn project(manifest: &str, hook_exit: u8) -> Project {
    let mut project = Project::new(manifest);
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
        format!(
            "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\n\
             echo tick >> ticked\nexit {hook_exit}\n"
        ),
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

/// Starts `zaf spawn --watch --version 0.3.0` without waiting for it.
fn start_watch(project: &Project) -> Child {
    project
        .command_in(&project.root, &["spawn", "--watch", "--version", "0.3.0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start zaf")
}

/// Waits up to `within` for `done` to hold.
fn wait_until(within: Duration, done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

/// Whether `child` is still running; stops it and gives its output when `stop`.
fn running(child: &mut Child) -> bool {
    child.try_wait().expect("try_wait").is_none()
}

/// Kills the watch, which only a signal ends, and gives its stdout and stderr.
fn stop(mut child: Child) -> (String, String) {
    child.kill().expect("kill the watch");
    let output = child.wait_with_output().expect("output");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_first_tick_runs_at_once_and_the_watch_goes_on() {
    let project = project(MANIFEST, 0);

    let mut child = start_watch(&project);
    let ticked = project.root.join("ticked");
    assert!(
        wait_until(Duration::from_secs(20), || ticked.exists()),
        "no tick ran before the first interval"
    );
    // The next tick is a minute away: the loop sleeps, it does not end.
    std::thread::sleep(Duration::from_millis(500));
    assert!(running(&mut child), "the watch ended after one tick");

    let (stdout, stderr) = stop(child);
    assert!(stdout.contains("a tick every 1 minutes"), "{stdout}");
    assert!(
        stdout.contains("slots 1 of 1"),
        "the tick ran: {stdout}{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&ticked).expect("ticked").lines().count(),
        1
    );
}

#[test]
fn a_failed_tick_does_not_end_the_watch() {
    // The pre-tick hook exits 1, which fails the tick with exit 2.
    let project = project(MANIFEST, 1);

    let mut child = start_watch(&project);
    let ticked = project.root.join("ticked");
    assert!(wait_until(Duration::from_secs(20), || ticked.exists()));
    std::thread::sleep(Duration::from_millis(500));
    assert!(running(&mut child), "a failed tick ended the watch");

    let (_, stderr) = stop(child);
    assert!(
        stderr.contains("tick 1 failed with exit 2; the watch goes on"),
        "{stderr}"
    );
}

#[test]
fn a_held_lock_is_skipped() {
    let project = project(MANIFEST, 0);
    let _held = hold_lock(&project);
    let before = project.gh_calls().len();

    let mut child = start_watch(&project);
    std::thread::sleep(Duration::from_secs(2));
    assert!(running(&mut child), "a held lock ended the watch");

    let (stdout, stderr) = stop(child);
    assert!(stdout.contains("skipped"), "{stdout}{stderr}");
    assert!(!stderr.contains("failed"), "a skip is no failure: {stderr}");
    // Nothing ran: no hook, no forge read, no plan.
    assert!(!project.root.join("ticked").exists(), "the pre-tick ran");
    assert_eq!(project.gh_calls().len(), before, "no forge read");
}

#[test]
fn a_zero_interval_fails() {
    let manifest = MANIFEST.replace("tick_minutes = 1", "tick_minutes = 0");
    let project = project_without_hook(&manifest);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--watch", "--version", "0.3.0"]));

    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stderr.contains("tick_minutes"), "{stderr}");
    assert!(!project.root.join("ticked").exists());
}

/// A project that only has to fail to load.
fn project_without_hook(manifest: &str) -> Project {
    Project::new(manifest)
}

#[test]
fn a_watch_needs_a_version() {
    let project = project(MANIFEST, 0);

    let (code, _, stderr) = outcome(&project.zaf(&["spawn", "--watch"]));

    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("--version"), "{stderr}");
    assert!(!project.root.join("ticked").exists());
}

#[test]
fn a_watch_is_not_a_once_or_a_dry_run() {
    let project = project(MANIFEST, 0);

    for flag in ["--once", "--dry-run"] {
        let (code, _, stderr) =
            outcome(&project.zaf(&["spawn", "--watch", flag, "--version", "0.3.0"]));
        assert_eq!(code, 2, "{flag}: {stderr}");
    }
    assert!(!project.root.join("ticked").exists());
}

#[test]
fn neither_flag_still_refuses() {
    let project = project(MANIFEST, 0);

    let (code, _, stderr) = outcome(&project.zaf(&["spawn"]));

    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("--once"), "{stderr}");
}
