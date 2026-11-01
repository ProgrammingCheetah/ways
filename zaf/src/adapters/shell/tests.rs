use std::time::{Duration, Instant};

use super::*;

fn shell(dir: &Path) -> SystemShell {
    SystemShell::new(dir, Duration::from_secs(30))
}

fn line(text: &str) -> CommandLine {
    CommandLine::from(text)
}

#[test]
fn a_passing_line_returns_its_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = shell(dir.path()).run(&line("echo hello")).expect("runs");
    assert_eq!(run.verdict, GateVerdict::Passed);
    assert_eq!(run.output, "hello\n");
}

#[test]
fn a_failing_line_is_a_finished_run_with_a_failed_verdict() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = shell(dir.path())
        .run(&line("echo before; exit 4"))
        .expect("runs");
    assert_eq!(run.verdict, GateVerdict::Failed);
    assert_eq!(run.output, "before\n");
}

#[test]
fn stdout_and_stderr_come_back_together_in_order() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = shell(dir.path())
        .run(&line("echo one; echo two >&2; echo three"))
        .expect("runs");
    assert_eq!(run.output, "one\ntwo\nthree\n");
}

#[test]
fn the_line_runs_in_the_directory_it_was_given() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("marker.txt"), "found").expect("marker");
    let run = shell(dir.path())
        .run(&line("cat marker.txt"))
        .expect("runs");
    assert_eq!(run.output, "found");
}

#[test]
fn a_syntax_error_is_reported_in_the_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = shell(dir.path()).run(&line("if then fi")).expect("runs");
    assert_eq!(run.verdict, GateVerdict::Failed);
    assert!(!run.output.is_empty(), "the shell's complaint is captured");
}

#[test]
fn a_run_past_the_bound_is_killed_and_unavailable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let short = SystemShell::new(dir.path(), Duration::from_millis(200));
    let started = Instant::now();
    let error = short.run(&line("sleep 30")).expect_err("times out");
    assert!(matches!(error, PortError::Unavailable(_)), "{error:?}");
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "killed, not awaited"
    );
}

#[test]
fn a_missing_directory_is_unavailable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nowhere");
    let error = shell(&missing).run(&line("true")).expect_err("no dir");
    assert!(matches!(error, PortError::Unavailable(_)), "{error:?}");
}
