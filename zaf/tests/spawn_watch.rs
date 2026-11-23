//! `zaf spawn --watch`, the scheduler that stays up, through the binary: a tick at once and then
//! one whenever asked (`.zaf/tick.request`) or each `[spawn] tick_minutes`; a tick that fails is
//! logged and the loop goes on; a held `spawn.lock` holds the tick back; `tick_minutes = 0` fails
//! the load; and `zaf spawn` with neither flag still refuses. It holds `.zaf/scheduler.lock`, so a
//! second one says where the first is, and one left by a scheduler that died is taken over;
//! `zaf spawn --once` and the verbs that change what a tick reads ask it for a tick, and with no
//! scheduler they work as before. It restarts as a new binary that answers `--version`, between
//! two ticks, as the same process, and keeps its image when the new one does not run. SIGINT and
//! SIGTERM end it once the tick that runs has ended, its lock let go. The timer itself is a minute
//! at least, so its pacing is checked on the loop (`app::schedule::scheduler`); no test here
//! waits for it.

mod common;

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{Project, Running, outcome, scheduler_up};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [spawn]\ntick_minutes = 1\n";

/// A project with current profiles, an empty board and a `pre-tick` hook that marks each tick and
/// exits with `hook_exit`.
fn project(manifest: &str, hook_exit: u8) -> Project {
    project_with_remote(manifest, hook_exit, true)
}

/// [`project`], with the remote the build start opens its branch on only when `remote`.
fn project_with_remote(manifest: &str, hook_exit: u8, remote: bool) -> Project {
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
    // A fresh scheduler opens the build first: signed papers and a remote to open it on.
    project.ready_to_build("0.3.0", remote);
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
        stdout.contains("nothing to do"),
        "the tick ran: {stdout}{stderr}"
    );
    assert!(stdout.contains("tick "), "{stdout}");
    assert!(stdout.contains("next tick "), "{stdout}");
    assert!(!stdout.lines().any(|line| line == "idle"), "{stdout}");
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
    let before = forge_reads(&project);

    let mut watch = scheduler(&project);
    watch.wait_for_lines("skipped: another tick holds spawn.lock", 1);
    assert!(watch.runs(), "a held lock ended the watch");

    let stderr = watch.stderr();
    assert!(!stderr.contains("failed"), "a skip is no failure: {stderr}");
    // Nothing ran: no hook, no forge read, no plan.
    assert!(!project.root.join("ticked").exists(), "the pre-tick ran");
    assert_eq!(forge_reads(&project), before, "no forge read");
}

/// How many `gh` calls were made that a tick makes: the build start's `base-open` asks the forge
/// for its default branch (`api repos/…`) and is no tick's read.
fn forge_reads(project: &Project) -> usize {
    project
        .gh_calls()
        .iter()
        .filter(|call| !call.starts_with("api repos/"))
        .count()
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

/// Whether `.zaf/scheduler.lock` can be taken: no scheduler holds it.
fn lock_is_free(project: &Project) -> bool {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(project.root.join(".zaf/scheduler.lock"))
        .expect("the lock file");
    file.try_lock().is_ok()
}

#[test]
fn spawn_watch_refuses_unsigned_design_and_frees_the_lock() {
    let project = project(MANIFEST, 0);
    let design = project.root.join(".teams/releases/0.3.0/design.md");
    fs::write(&design, "# Design\n").expect("an unsigned design");

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--watch", "--version", "0.3.0"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("design.md"), "{stderr}");
    assert!(stderr.contains("zaf begin --version 0.3.0"), "{stderr}");
    assert!(!stdout.contains("scheduler up"), "{stdout}");
    assert!(lock_is_free(&project), "the lock was not let go");
    assert!(!project.root.join("ticked").exists(), "a tick ran");
}

#[test]
fn spawn_watch_refuses_without_a_remote() {
    let project = project_with_remote(MANIFEST, 0, false);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--watch", "--version", "0.3.0"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("no remote"), "{stderr}");
    assert!(lock_is_free(&project), "the lock was not let go");
    assert!(!project.root.join("ticked").exists(), "a tick ran");
}

#[test]
fn spawn_watch_starts_the_build_first() {
    let project = project(MANIFEST, 0);
    let running = scheduler(&project);
    first_tick(&running);

    let out = running.stdout();
    let lines: Vec<&str> = out.lines().collect();
    let at = |wanted: &dyn Fn(&str) -> bool| lines.iter().position(|line| wanted(line));
    let made = at(&|line| line == "made feature/0.3.0");
    let committed = at(&|line| line.starts_with("committed ") && line.len() > "committed ".len());
    let tick = at(&|line| line.starts_with("tick "));
    let (Some(made), Some(committed), Some(tick)) = (made, committed, tick) else {
        panic!("no made, committed and tick lines in:\n{out}");
    };
    assert!(made < committed && committed < tick, "{out}");
}

#[test]
fn spawn_watch_restart_skips_the_start() {
    let project = project(MANIFEST, 0);
    let zaf = install(&project, "zaf");
    let running = scheduler_of(&project, &zaf);
    first_tick(&running);

    install(&project, "zaf");
    running.wait_for_lines("scheduler restarted from zaf ", 1);
    running.wait_for_lines("next tick ", 2);

    let out = running.stdout();
    let (_, after) = out
        .split_once("scheduler restarted from zaf ")
        .expect("the restart line");
    for text in ["made ", "committed ", " is open"] {
        assert!(!after.contains(text), "the restart said {text:?}:\n{out}");
    }
    // The first image's start said both.
    assert!(out.contains("made feature/0.3.0"), "{out}");
    assert!(out.contains("committed "), "{out}");
}

/// Starts the scheduler of `project` from the built binary.
fn scheduler(project: &Project) -> Running {
    Running::start(project.command_in(&project.root, &["spawn", "--watch", "--version", "0.3.0"]))
}

/// Starts the scheduler from the zaf binary at `program`.
fn scheduler_of(project: &Project, program: &Path) -> Running {
    Running::start(project.command_of(
        program,
        &project.root,
        &["spawn", "--watch", "--version", "0.3.0"],
    ))
}

/// Waits for the scheduler's first tick to end: its `next tick` line.
fn first_tick(running: &Running) {
    running.wait_for_lines("next tick ", 1);
}

/// How many ticks the `pre-tick` hook has seen.
fn ticks(project: &Project) -> usize {
    fs::read_to_string(project.root.join("ticked"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// A copy of the built binary at `<temp>/bin/<name>`, written beside it and renamed into place,
/// as an install replaces a binary.
fn install(project: &Project, name: &str) -> PathBuf {
    let bin = project.temp.path().join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let staged = bin.join(format!("{name}.new"));
    fs::copy(env!("CARGO_BIN_EXE_zaf"), &staged).expect("copy the binary");
    let path = bin.join(name);
    fs::rename(&staged, &path).expect("into place");
    path
}

/// Answers a ruling on question #45, which waits on nothing.
fn question_45(project: &mut Project) {
    project
        .on_first(
            "issue view 45 *",
            r#"{"number":45,"title":"Which way?","labels":[{"name":"question"}],"state":"OPEN","comments":[]}"#,
        )
        .on_first("issue close 45 *", "")
        .on_first(
            "issue comment 45 *",
            "https://github.com/example/tracker/issues/45#issuecomment-1\n",
        );
}

#[test]
fn a_watch_restarts_as_the_installed_binary() {
    // Each tick marks its start, then takes a second: long enough to replace the binary while it
    // runs, so the restart is seen to wait for the tick's end.
    let project = project(MANIFEST, 0);
    let hook = project.root.join(".zaf/actions/hooks/pre-tick");
    fs::write(
        &hook,
        "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\n\
         echo tick >> ticked\nsleep 1\n",
    )
    .expect("a slow hook");
    let zaf = install(&project, "zaf");
    let running = scheduler_of(&project, &zaf);
    running.wait_for("its first tick began", |_, _| ticks(&project) == 1);

    // Installed while the first tick runs.
    install(&project, "zaf");
    running.wait_for_lines("restarting as zaf ", 1);
    running.wait_for_lines("scheduler restarted from zaf ", 1);
    // The restart came after the first tick's end, and the new image ticked at once.
    running.wait_for_lines("asked by: restart", 1);

    // Asked for a tick, the new image runs it.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    running.wait_for_lines("asked by: zaf spawn --once", 1);

    let out = running.stdout();
    let at = |text: &str| {
        out.find(text)
            .unwrap_or_else(|| panic!("no {text:?} in:\n{out}"))
    };
    assert!(at("next tick ") < at("restarting as zaf "), "{out}");
    // The same process throughout: its pid in both start lines.
    let pid = format!(": pid {}, since ", running.pid());
    assert!(
        out.lines()
            .any(|line| line.starts_with("scheduler up: ") && line.contains(&pid)),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|line| line.starts_with("scheduler restarted from zaf ") && line.contains(&pid)),
        "{out}"
    );
    assert!(
        !running.stderr().contains("this image goes on"),
        "{}",
        running.stderr()
    );
}

#[test]
fn a_binary_that_does_not_run_keeps_the_old_image_and_says_so() {
    let project = project(MANIFEST, 0);
    let zaf = install(&project, "zaf");
    let running = scheduler_of(&project, &zaf);
    first_tick(&running);

    // A file that is no zaf: its `--version` fails.
    let bin = zaf.parent().expect("bin");
    let broken = bin.join("broken");
    fs::write(&broken, "#!/bin/sh\nexit 3\n").expect("broken");
    fs::set_permissions(&broken, fs::Permissions::from_mode(0o755)).expect("mode");
    fs::rename(&broken, &zaf).expect("installed");
    running.wait_for("the refusal", |_, err| err.contains("does not run"));

    // The old image goes on, and still ticks when asked.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    running.wait_for_lines("asked by: zaf spawn --once", 1);
    let err = running.stderr();
    assert!(
        err.contains(&format!(
            "the new binary at {} does not run (its --version ended with exit 3); this image goes \
             on, and tries again once the file changes",
            zaf.display()
        )),
        "{err}"
    );
    assert_eq!(err.matches("does not run").count(), 1, "said once: {err}");
    assert!(
        !running.stdout().contains("restarting as"),
        "{}",
        running.stdout()
    );
}

#[test]
fn a_second_scheduler_says_where_the_first_is_and_exits() {
    let project = project(MANIFEST, 0);
    let mut first = scheduler(&project);
    first_tick(&first);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--watch", "--version", "0.3.0"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains(&format!(
            "zaf spawn: a scheduler already runs here: pid {}, since ",
            first.pid()
        )),
        "{stderr}"
    );
    assert!(stderr.contains("--version 0.3.0)"), "{stderr}");
    assert!(first.runs(), "the first goes on");
    assert_eq!(ticks(&project), 1, "the second ran no tick");
}

#[test]
fn an_event_asks_and_the_tick_does_not_wait_for_the_timer() {
    let mut project = project(MANIFEST, 0);
    question_45(&mut project);
    let running = scheduler(&project);
    first_tick(&running);
    let asked = Instant::now();

    let (code, stdout, stderr) = outcome(&project.zaf(&["board", "ruling", "45", "Yes."]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    running.wait_for_lines("asked by: ruling #45", 1);
    // The timer is a minute away.
    assert!(
        asked.elapsed() < Duration::from_secs(30),
        "{:?}",
        asked.elapsed()
    );
    running.wait_for_lines("next tick ", 2);
    assert_eq!(ticks(&project), 2);
}

#[test]
fn several_requests_make_one_tick_that_names_them() {
    let project = project(MANIFEST, 0);
    let running = scheduler(&project);
    first_tick(&running);

    // Two asks that land between two looks, as two verbs that end at once leave them.
    fs::write(
        project.root.join(".zaf/tick.request"),
        "2027-01-15T08:00:00Z ruling #45\n2027-01-15T08:00:00Z sign #1\n",
    )
    .expect("two requests");

    running.wait_for_lines("asked by: ruling #45, sign #1", 1);
    running.wait_for_lines("next tick ", 2);
    // Another look or two: no tick of its own for either.
    std::thread::sleep(Duration::from_millis(2_500));
    assert_eq!(ticks(&project), 2, "{}", running.stdout());
}

#[test]
fn a_verb_works_with_no_scheduler_running() {
    let mut project = project(MANIFEST, 0);
    question_45(&mut project);

    let (code, stdout, stderr) = outcome(&project.zaf(&["board", "ruling", "45", "Yes."]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "#45 ruled\n");
    assert!(!project.root.join(".zaf/tick.request").exists());
    assert_eq!(ticks(&project), 0);
}

/// The one line a verb leaves on stderr when the running scheduler, this test's process as
/// [`scheduler_up`] names it, could not be asked for a tick for `who`: `verb`'s, naming the
/// scheduler and why, ending with its timer.
fn assert_not_asked(stderr: &str, verb: &str, who: &str) {
    let opening = format!(
        "zaf {verb}: the running scheduler (pid {}, since 2027-01-15T08:00:00Z (zaf 0.3.1, \
         --version 0.3.0)) was not asked for a tick for {who}: cannot write ",
        std::process::id()
    );
    assert!(stderr.starts_with(&opening), "{stderr}");
    assert!(
        stderr.ends_with(
            "tick.request: Is a directory (os error 21); its timer picks the change up at its \
             next tick\n"
        ),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
}

#[test]
fn a_ruling_that_cannot_ask_for_a_tick_still_rules_and_says_so() {
    let mut project = project(MANIFEST, 0);
    question_45(&mut project);
    let _scheduler = scheduler_up(&project);
    // The request file cannot be written: something else is in its place.
    fs::create_dir_all(project.root.join(".zaf/tick.request")).expect("in the way");

    let (code, stdout, stderr) = outcome(&project.zaf(&["board", "ruling", "45", "Yes."]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "#45 ruled\n");
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue comment 45")),
        "the ruling is recorded: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| call.starts_with("issue close 45")),
        "the question is closed: {calls:?}"
    );
    assert_not_asked(&stderr, "board ruling", "ruling #45");
    assert_eq!(ticks(&project), 0);
}

#[test]
fn signing_that_cannot_ask_for_a_tick_still_signs_and_says_so() {
    let mut project = project(MANIFEST, 0);
    project
        .on_first(
            "issue list --label epic *",
            r#"[{"number":1,"title":"Export","body":"Members export their reading list.","labels":[{"name":"epic"}],"comments":[]}]"#,
        )
        .on_first(
            "issue comment 1 *",
            "https://github.com/example/tracker/issues/1#issuecomment-1\n",
        );
    let _scheduler = scheduler_up(&project);
    fs::create_dir_all(project.root.join(".zaf/tick.request")).expect("in the way");

    let (code, stdout, stderr) = outcome(&project.zaf_with_terminal_stdin(&["epic", "sign", "1"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.starts_with("signed #1 sha256:"), "{stdout}");
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue comment 1 ") && call.contains("zaf:signature")),
        "the signature is recorded: {calls:?}"
    );
    assert_not_asked(&stderr, "epic sign", "sign #1");
    assert_eq!(ticks(&project), 0);
}

#[test]
fn once_defers_to_a_running_scheduler() {
    let project = project(MANIFEST, 0);
    let running = scheduler(&project);
    first_tick(&running);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.starts_with(&format!(
            "asked the running scheduler for a tick: pid {}, since ",
            running.pid()
        )) && stdout.ends_with(", --version 0.3.0)\n")
            && stdout.lines().count() == 1,
        "{stdout}"
    );
    running.wait_for_lines("asked by: zaf spawn --once", 1);
    running.wait_for_lines("next tick ", 2);
    assert_eq!(ticks(&project), 2, "the scheduler ran it, once");

    // As JSON, the same ask.
    let (code, stdout, _) =
        outcome(&project.zaf(&["--json", "spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(json["asked"]["pid"], running.pid());

    // A dry run computes here, whatever runs.
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["spawn", "--once", "--dry-run", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("asked the running scheduler"), "{stdout}");
}

#[test]
fn the_scheduler_ends_cleanly_on_a_signal_and_releases_its_lock() {
    let project = project(MANIFEST, 0);
    let lock = project.root.join(".zaf/scheduler.lock");

    // Between two ticks.
    let mut running = scheduler(&project);
    first_tick(&running);
    running.signal("TERM");
    assert_eq!(running.wait_exit(), Some(0), "{}", running.stderr());
    assert!(
        running
            .stdout()
            .ends_with("scheduler stopped on SIGTERM; scheduler.lock released\n"),
        "{}",
        running.stdout()
    );
    assert_eq!(fs::read_to_string(&lock).expect("the lock file"), "");

    // During a tick: the tick ends first.
    let hook = project.root.join(".zaf/actions/hooks/pre-tick");
    fs::write(
        &hook,
        "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\n\
         echo tick >> ticked\nsleep 1\necho done >> hook-ended\n",
    )
    .expect("a slow hook");
    let mut running = scheduler(&project);
    running.wait_for("the tick began", |_, _| ticks(&project) == 2);
    running.signal("INT");
    assert_eq!(running.wait_exit(), Some(0), "{}", running.stderr());
    let out = running.stdout();
    assert!(
        project.root.join("hook-ended").exists(),
        "the tick ran to its end"
    );
    assert!(
        out.find("next tick ") < out.find("scheduler stopped on SIGINT"),
        "{out}"
    );
    assert_eq!(fs::read_to_string(&lock).expect("the lock file"), "");

    // With none running, `--once` ticks here again.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("asked the running scheduler"), "{stdout}");
    assert_eq!(ticks(&project), 3);
}

#[test]
fn a_stale_lock_line_of_a_dead_scheduler_is_taken_over() {
    let project = project(MANIFEST, 0);
    let state = project.root.join(".zaf");
    fs::create_dir_all(&state).expect("state");
    // A scheduler that died with no chance to clear its line: no process has this pid and start.
    fs::write(
        state.join("scheduler.lock"),
        "{\"pid\":4194303,\"start\":\"1\",\"since\":\"2027-01-15T08:00:00Z\",\
         \"zaf\":\"0.3.0\",\"version\":\"0.3.0\"}\n",
    )
    .expect("a stale line");
    // While no scheduler holds it, `--once` ticks here.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("asked the running scheduler"), "{stdout}");

    let running = scheduler(&project);
    first_tick(&running);

    let out = running.stdout();
    assert!(
        out.contains(&format!(
            "scheduler up, taking scheduler.lock over from pid 4194303, which is gone: pid {}, ",
            running.pid()
        )),
        "{out}"
    );
    let line = fs::read_to_string(state.join("scheduler.lock")).expect("the lock line");
    assert!(
        line.starts_with(&format!("{{\"pid\":{},", running.pid())),
        "{line}"
    );
}

/// The process start this test's own process has, as `/proc/<pid>/stat` reports it; none off
/// Linux, where a test that needs it passes over.
fn own_start() -> Option<String> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let (_, after) = stat.rsplit_once(')')?;
    after.split_whitespace().nth(19).map(str::to_owned)
}

#[test]
fn once_ticks_itself_when_the_line_is_alive_but_the_lock_is_free() {
    let project = project(MANIFEST, 0);
    let Some(start) = own_start() else {
        return;
    };
    let state = project.root.join(".zaf");
    fs::create_dir_all(&state).expect("state");
    // A line naming a live process, this test's, which holds no lock on the file: the line only
    // says where a scheduler would be, and none runs.
    fs::write(
        state.join("scheduler.lock"),
        format!(
            "{{\"pid\":{},\"start\":\"{start}\",\"since\":\"2027-01-15T08:00:00Z\",\
             \"zaf\":\"0.3.0\",\"version\":\"0.3.0\"}}\n",
            std::process::id()
        ),
    )
    .expect("a line");

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("asked the running scheduler"), "{stdout}");
    assert_eq!(ticks(&project), 1, "{stdout}{stderr}");
    assert!(!state.join("tick.request").exists());
}

#[test]
fn once_with_another_version_is_refused_and_names_the_schedulers() {
    let project = project(MANIFEST, 0);
    let running = scheduler(&project);
    first_tick(&running);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.5.0"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.contains(&format!("pid {}, since ", running.pid()))
            && stderr.contains("--version 0.3.0")
            && stderr.contains("no tick was asked for or run"),
        "{stderr}"
    );
    // A look or two later: no tick was asked of the scheduler, and none ran here.
    std::thread::sleep(Duration::from_millis(2_500));
    assert_eq!(ticks(&project), 1, "{}", running.stdout());
    assert!(
        !running.stdout().contains("zaf spawn --once"),
        "{}",
        running.stdout()
    );
    let request = project.root.join(".zaf/tick.request");
    assert_eq!(fs::read_to_string(&request).unwrap_or_default(), "");
}

/// Puts a `gh` first on the project's search path that takes `pause` before it answers as the
/// fake does, and counts each call as it begins (`gh-began`) and once it has answered (`gh-ended`);
/// `in-gh` marks the first.
fn slow_gh(project: &mut Project, pause: &str) {
    let wrap = project.temp.path().join("gh-slow");
    fs::create_dir_all(&wrap).expect("wrap");
    let root = project.root.display().to_string();
    let file = wrap.join("gh");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n\
             case \"$*\" in 'api repos/'*) exec '{fake}' \"$@\" ;; esac\n\
             echo call >> '{root}/gh-began'\n\
             touch '{root}/in-gh'\n\
             sleep {pause}\n\
             '{fake}' \"$@\"\n\
             code=$?\n\
             echo call >> '{root}/gh-ended'\n\
             exit $code\n",
            fake = common::fixtures().join("gh").display()
        ),
    )
    .expect("a slow gh");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    let path = common::path_with_zaf(&[&wrap, &common::fixtures()]);
    project.env("PATH", &path);
}

/// The lines of the root's file `name`; none when it is not there.
fn count(project: &Project, name: &str) -> usize {
    fs::read_to_string(project.root.join(name))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

#[test]
fn ctrl_c_ends_the_scheduler_after_the_tick_and_kills_none_of_its_children() {
    use std::os::unix::process::CommandExt;

    let mut project = project(MANIFEST, 0);
    slow_gh(&mut project, "0.5");
    // The scheduler leads a process group of its own, as a shell's foreground job does: a Ctrl-C
    // in its terminal is a SIGINT to every process of that group.
    let mut command =
        project.command_in(&project.root, &["spawn", "--watch", "--version", "0.3.0"]);
    command.process_group(0);
    let mut running = Running::start(command);
    let in_gh = project.root.join("in-gh");
    running.wait_for("the tick read the board", |_, _| in_gh.exists());

    let group = format!("-{}", running.pid());
    let status = std::process::Command::new("kill")
        .args(["-INT", "--", &group])
        .status()
        .expect("kill");
    assert!(status.success(), "kill -INT {group}");

    assert_eq!(running.wait_exit(), Some(0), "{}", running.stderr());
    let (out, err) = (running.stdout(), running.stderr());
    // The tick ran to its end, every read whole, and only then did the scheduler stop.
    assert!(
        out.find("next tick ") < out.find("scheduler stopped on SIGINT"),
        "{out}{err}"
    );
    assert!(out.contains("nothing to do"), "{out}{err}");
    assert!(!err.contains("failed"), "{err}");
    assert!(!err.contains("cannot"), "{err}");
    assert!(count(&project, "gh-began") > 0);
    assert_eq!(
        count(&project, "gh-began"),
        count(&project, "gh-ended"),
        "a read was cut short: {out}{err}"
    );
    assert_eq!(ticks(&project), 1);
}

#[test]
fn a_stop_that_lands_during_a_restart_still_stops_the_scheduler() {
    let project = project(MANIFEST, 0);
    let request = project.root.join(".zaf/tick.request");
    // The first attempt holds the restart in its window, between its last look for a stop and the
    // exec: it waits for the request file's lock, which it takes to keep what no tick served. The
    // others send the stop as the restart is announced, a little later each time.
    for attempt in 0..4_u64 {
        let zaf = install(&project, "zaf");
        let mut running = scheduler_of(&project, &zaf);
        first_tick(&running);
        let held = (attempt == 0).then(|| {
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&request)
                .expect("the request file");
            file.lock().expect("its lock");
            file
        });

        install(&project, "zaf");
        running.wait_for_lines("restarting as zaf ", 1);
        if attempt > 1 {
            std::thread::sleep(Duration::from_millis(2 * (attempt - 1)));
        }
        running.signal("TERM");
        drop(held);

        assert_eq!(
            running.wait_exit(),
            Some(0),
            "attempt {attempt}:\n{}\n{}",
            running.stdout(),
            running.stderr()
        );
        assert!(
            running
                .stdout()
                .ends_with("scheduler stopped on SIGTERM; scheduler.lock released\n"),
            "attempt {attempt}:\n{}\n{}",
            running.stdout(),
            running.stderr()
        );
    }
}

/// The board of a built version: its one epic, signed and challenged, and its one task done.
fn built_board(project: &mut Project) {
    use zaf::domain::{BodyHash, TicketBody};
    let body = "Release: 0.3.0\n\nEpic 1.";
    let hash = BodyHash::of(&TicketBody::from(body));
    let epics = serde_json::json!([{
        "number": 1,
        "title": "Epic 1",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": [
            {"body": format!(
                "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                hash.as_str()
            )},
            {"body": "<!-- zaf:report challenger instance 9 on #1 -->\nEvery task is exact."},
        ],
    }]);
    let tasks = serde_json::json!([{
        "number": 2, "title": "t", "labels": [{"name": "task"}],
        "parent": {"number": 1}, "state": "CLOSED", "stateReason": "COMPLETED",
    }]);
    project
        .on_first("issue list --label task *", &tasks.to_string())
        .on_first("issue list --label epic *", &epics.to_string());
}

/// Gives the project a `release-open` action that counts its runs and prints a pull request.
fn release_open_action(project: &Project) -> PathBuf {
    let runs = project.root.join("release-opened");
    let actions = project.root.join(".zaf/actions");
    let file = actions.join("release-open");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n# zaf description: open the release pull request\n# zaf 1: refused\n\
             echo \"$1 base=$ZAF_BASE\" >> {}\n\
             echo 12\necho https://forge.example/example/tracker/pulls/12\n",
            runs.display()
        ),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    runs
}

#[test]
fn piped_watch_says_release_pull_request() {
    let mut project = project(MANIFEST, 0);
    built_board(&mut project);
    let runs = release_open_action(&project);

    let running = scheduler(&project);
    running.wait_for_lines("zaf built ", 1);
    // The watch goes on after the line: the tick's wait for the next one is said.
    running.wait_for_lines("next tick ", 1);

    let out = running.stdout();
    let said: Vec<&str> = out
        .lines()
        .filter(|line| line.starts_with("zaf built "))
        .collect();
    assert_eq!(
        said,
        [
            "zaf built 0.3.0: release pull request #12, https://forge.example/example/tracker/pulls/12"
        ],
        "{out}"
    );
    assert_eq!(
        fs::read_to_string(&runs).expect("the action ran").trim(),
        "0.3.0 base=feature/0.3.0"
    );
    running.signal("TERM");
    let mut running = running;
    assert_eq!(running.wait_exit(), Some(0), "{}", running.stderr());
}
