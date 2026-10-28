use super::*;
use crate::domain::{Profile, RoleName};

fn runner(command: &str) -> ProcessRunner {
    ProcessRunner::new(CommandLine::from(command), Duration::from_secs(10))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("a captured file")
}

#[test]
fn the_prompt_is_substituted_and_captured_in_the_report() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = runner("echo {prompt}; echo done")
        .execute("read the brief", dir.path())
        .expect("a run");
    assert!(report.status.success());
    assert_eq!(read(&report.report), "read the brief\ndone\n");
    assert_eq!(report.report, dir.path().join("report.md"));
}

#[test]
fn a_prompt_with_quotes_and_shell_characters_arrives_whole() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let prompt = "it's $HOME; `id` \"quoted\"";
    let report = runner("printf '%s' {prompt}")
        .execute(prompt, dir.path())
        .expect("a run");
    assert_eq!(read(&report.report), prompt);
}

#[test]
fn the_dir_is_substituted_and_is_the_working_directory() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = runner("printf '%s\\n' {dir}; pwd")
        .execute("unused", dir.path())
        .expect("a run");
    let lines = read(&report.report);
    let expected = dir.path().to_string_lossy().into_owned();
    let mut lines = lines.lines();
    assert_eq!(lines.next(), Some(expected.as_str()));
    let working = lines.next().expect("the working directory");
    let canonical = fs::canonicalize(dir.path()).expect("canonical");
    assert_eq!(fs::canonicalize(working).expect("canonical"), canonical);
}

#[test]
fn extra_environment_reaches_the_process() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = runner("printf '%s' \"$CONFIG_HOME\"")
        .with_env("CONFIG_HOME", "/profiles/lean")
        .execute("unused", dir.path())
        .expect("a run");
    assert_eq!(read(&report.report), "/profiles/lean");
}

#[test]
fn stderr_goes_to_the_errors_file_and_stdout_stays_clean() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = runner("echo out; echo oops >&2")
        .execute("unused", dir.path())
        .expect("a run");
    assert_eq!(read(&report.report), "out\n");
    assert_eq!(read(&report.errors), "oops\n");
    assert_eq!(report.errors, dir.path().join("errors.log"));
}

#[test]
fn a_failing_command_reports_its_exit_code() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = runner("exit 3")
        .execute("unused", dir.path())
        .expect("a run");
    assert_eq!(report.status.code(), Some(3));
}

#[test]
fn a_command_that_outlives_the_timeout_is_killed_and_unavailable() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let slow = ProcessRunner::new(CommandLine::from("sleep 5"), Duration::from_secs(1));
    let started = Instant::now();
    let result = slow.execute("unused", dir.path());
    assert!(matches!(result, Err(PortError::Unavailable(_))));
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn the_port_returns_the_exit_code_and_the_report_as_transcript() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let request = RunRequest {
        role: RoleName::from("keeper"),
        profile: Profile::Lean,
        prompt: "read the brief".to_owned(),
        dir: dir.path().to_path_buf(),
    };
    let process = runner("echo {prompt}; exit 2");
    let port: &dyn Runner = &process;
    let output = port.run(&request).expect("a run");
    assert_eq!(output.exit_code, 2);
    assert_eq!(output.transcript, "read the brief\n");
}

#[test]
fn an_unwritable_directory_is_a_backend_failure() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let absent = dir.path().join("absent");
    let result = runner("true").execute("unused", &absent);
    assert!(matches!(result, Err(PortError::Backend(_))));
}

#[test]
fn single_quote_escapes_embedded_quotes() {
    assert_eq!(single_quote("a b"), "'a b'");
    assert_eq!(single_quote("it's"), r"'it'\''s'");
}
