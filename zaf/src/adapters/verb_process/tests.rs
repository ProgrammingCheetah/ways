use std::sync::Mutex;

use super::*;

/// A verb run of `sh -c <script>`: the first word stands for the program name `zaf`.
fn sh(script: &str, stdin: Option<&str>) -> VerbRun {
    VerbRun {
        argv: ["zaf", "-c", script].map(String::from).to_vec(),
        stdin: stdin.map(String::from),
        cwd: PathBuf::from("/"),
    }
}

fn stand_in(interval: Duration) -> VerbProcess {
    VerbProcess::standing_in("/bin/sh", interval)
}

fn here() -> Vec<(OsString, OsString)> {
    std::env::vars_os().collect()
}

#[test]
fn directory_rule() {
    let own = || Some(PathBuf::from("/own"));
    let some = |text: &str| Some(OsString::from(text));
    assert_eq!(
        directory(some("/root"), some("/project"), own),
        Some(PathBuf::from("/root"))
    );
    assert_eq!(
        directory(None, some("/project"), own),
        Some(PathBuf::from("/project"))
    );
    assert_eq!(directory(None, None, own), Some(PathBuf::from("/own")));
    // An empty variable is not a directory.
    assert_eq!(
        directory(some(""), some("/project"), own),
        Some(PathBuf::from("/project"))
    );
    assert_eq!(
        directory(some(""), some(""), own),
        Some(PathBuf::from("/own"))
    );
    assert_eq!(directory(None, None, || None), None);
}

#[test]
fn ticks_rise() {
    let runner = stand_in(Duration::from_millis(100));
    let ticks = Mutex::new(Vec::new());
    let outcome = runner.run_in(
        &sh(
            "echo first >&2; sleep 0.1; echo second >&2; sleep 0.6",
            None,
        ),
        here(),
        &|tick| ticks.lock().unwrap().push(tick),
        &Cancel::new(),
    );
    assert_eq!(outcome.exit, 0);
    assert_eq!(outcome.stderr, "first\nsecond\n");
    let ticks = ticks.into_inner().unwrap();
    assert!(ticks.len() >= 3, "ticks: {ticks:?}");
    assert!(
        ticks
            .windows(2)
            .all(|pair| pair[0].elapsed < pair[1].elapsed),
        "elapsed does not rise: {ticks:?}"
    );
    assert!(ticks[0].elapsed >= Duration::from_millis(100));
    assert_eq!(ticks.last().unwrap().last_stderr.as_deref(), Some("second"));
}

#[test]
fn cancel_kills_group() {
    let folder = tempfile::tempdir().unwrap();
    let pid_file = folder.path().join("grandchild");
    let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
    let runner = stand_in(Duration::from_secs(60));
    let cancel = Cancel::new();
    let raiser = cancel.clone();
    let waited = pid_file.clone();
    let raised = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read_to_string(&waited).is_ok_and(|text| text.trim().is_empty())
            || !waited.exists()
        {
            assert!(Instant::now() < deadline, "the grandchild never started");
            thread::sleep(Duration::from_millis(10));
        }
        raiser.cancel();
        Instant::now()
    });
    let outcome = runner.run_in(&sh(&script, None), here(), &|_| {}, &cancel);
    let ended = Instant::now();
    let raised = raised.join().unwrap();
    assert!(ended.duration_since(raised) < Duration::from_secs(1));
    assert_eq!(outcome.exit, 2);
    assert!(outcome.stderr.ends_with(CANCELLED), "{}", outcome.stderr);
    let grandchild: libc::pid_t = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while runs(grandchild) {
        assert!(Instant::now() < deadline, "the grandchild still runs");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` is a process that still runs: a zombie is dead, though signal 0 reaches it (in a
/// container with no init to reap the orphans of a killed group it stays one for good).
fn runs(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only asks whether the process is there.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // The state follows the command's name, which is in parentheses and may hold any text.
        Ok(stat) => stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next())
            .is_none_or(|state| state != 'Z' && state != 'X'),
        // No /proc (macOS): the signal's answer stands.
        Err(_) => true,
    }
}

#[test]
fn stdin_reaches_child() {
    let outcome = stand_in(TICK_INTERVAL).run_in(
        &sh("cat", Some("title: a body\nwith two lines\n")),
        here(),
        &|_| {},
        &Cancel::new(),
    );
    assert_eq!(outcome.exit, 0);
    assert_eq!(outcome.stdout, "title: a body\nwith two lines\n");
}

#[test]
fn mcp_child_drops_nesting_markers() {
    let mut environment = here();
    environment.push((DEPTH_VAR.into(), "3".into()));
    environment.push((CHAIN_VAR.into(), "merge>fix".into()));
    environment.push(("ZAF_KEPT_FOR_THE_TEST".into(), "yes".into()));
    let outcome =
        stand_in(TICK_INTERVAL).run_in(&sh("env", None), environment, &|_| {}, &Cancel::new());
    assert_eq!(outcome.exit, 0);
    let names: Vec<&str> = outcome
        .stdout
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect();
    assert!(!names.contains(&DEPTH_VAR), "{names:?}");
    assert!(!names.contains(&CHAIN_VAR), "{names:?}");
    assert!(names.contains(&"ZAF_KEPT_FOR_THE_TEST"), "{names:?}");
}

#[test]
fn exit_code_and_both_streams_are_captured() {
    let outcome = stand_in(TICK_INTERVAL).run_in(
        &sh("echo out; echo err >&2; exit 3", None),
        here(),
        &|_| {},
        &Cancel::new(),
    );
    assert_eq!(outcome.stdout, "out\n");
    assert_eq!(outcome.stderr, "err\n");
    assert_eq!(outcome.exit, 3);
}

#[test]
fn a_program_that_cannot_start_is_exit_2() {
    let outcome = VerbProcess::standing_in("/nonexistent/zaf", TICK_INTERVAL).run_in(
        &sh("true", None),
        here(),
        &|_| {},
        &Cancel::new(),
    );
    assert_eq!(outcome.exit, 2);
    assert!(
        outcome.stderr.contains("/nonexistent/zaf"),
        "{}",
        outcome.stderr
    );
}
