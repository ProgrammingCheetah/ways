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
        env: std::collections::BTreeMap::new(),
    };
    let process = runner("echo {prompt}; exit 2");
    let port: &dyn Runner = &process;
    let output = port.run(&request).expect("a run");
    assert_eq!(output.exit_code, 2);
    assert_eq!(output.transcript, "read the brief\n");
}

fn request(role: &str, profile: Profile, dir: &Path) -> RunRequest {
    RunRequest {
        role: RoleName::from(role),
        profile,
        prompt: "read the brief".to_owned(),
        dir: dir.to_path_buf(),
        env: std::collections::BTreeMap::new(),
    }
}

/// A runner with two roles, each its own template and environment, and a default for the rest.
fn two_roles() -> ProcessRunner {
    let team = RoleRun::new(CommandLine::from(
        "printf 'team %s %s %s' {prompt} \"$HOME_DIR\" \"$MODEL\"",
    ))
    .with_env("HOME_DIR", "/profiles/team")
    .with_env("MODEL", "opus");
    let programmer = RoleRun::new(CommandLine::from(
        "printf 'programmer %s %s %s' {prompt} \"$HOME_DIR\" \"$MODEL\"",
    ))
    .with_env("HOME_DIR", "/profiles/programmer");
    runner("printf 'default %s' {prompt}")
        .with_env("MODEL", "sonnet")
        .with_role(RoleName::from("team"), team)
        .with_role(RoleName::from("programmer"), programmer)
}

#[test]
fn two_roles_resolve_to_two_templates_and_environments() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let process = two_roles();
    let port: &dyn Runner = &process;
    let team = port
        .run(&request("team", Profile::Build, dir.path()))
        .expect("a run");
    let programmer = port
        .run(&request("programmer", Profile::Lean, dir.path()))
        .expect("a run");
    assert_eq!(team.transcript, "team read the brief /profiles/team opus");
    assert_eq!(
        programmer.transcript,
        "programmer read the brief /profiles/programmer sonnet"
    );
}

#[test]
fn a_role_left_out_runs_the_default_command() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let output = two_roles()
        .run(&request("reviewer", Profile::Review, dir.path()))
        .expect("a run");
    assert_eq!(output.transcript, "default read the brief");
}

#[test]
fn the_profile_adds_environment_and_the_role_wins_over_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let role = RoleRun::new(CommandLine::from("printf '%s %s' \"$CONFIG\" \"$MODEL\""))
        .with_env("MODEL", "from-role");
    let process = runner("true")
        .with_env("CONFIG", "from-runner")
        .with_profile_env(Profile::Lean, "CONFIG", "/profiles/lean")
        .with_profile_env(Profile::Lean, "MODEL", "from-profile")
        .with_role(RoleName::from("keeper"), role);
    let lean = process
        .run(&request("keeper", Profile::Lean, dir.path()))
        .expect("a run");
    assert_eq!(lean.transcript, "/profiles/lean from-role");
    let build = process
        .run(&request("keeper", Profile::Build, dir.path()))
        .expect("a run");
    assert_eq!(build.transcript, "from-runner from-role");
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

#[test]
fn the_requests_environment_wins_over_every_layer() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let role = RoleRun::new(CommandLine::from(
        "printf '%s %s %s' \"$MODEL\" \"$ZAF_INSTANCE\" \"$EXTRA\"",
    ))
    .with_env("MODEL", "from-role");
    let process = runner("true")
        .with_env("ZAF_INSTANCE", "from-runner")
        .with_profile_env(Profile::Lean, "ZAF_INSTANCE", "from-profile")
        .with_role(RoleName::from("keeper"), role);
    let mut request = request("keeper", Profile::Lean, dir.path());
    request
        .env
        .insert("MODEL".to_owned(), "from-request".to_owned());
    request.env.insert(
        "ZAF_INSTANCE".to_owned(),
        "team instance 1 on #2".to_owned(),
    );
    request
        .env
        .insert("EXTRA".to_owned(), "only-request".to_owned());
    let output = process.run(&request).expect("a run");
    assert_eq!(
        output.transcript,
        "from-request team instance 1 on #2 only-request"
    );
}
