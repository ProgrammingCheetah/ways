use super::*;

fn gate_section(full: &str, light: Option<&str>) -> GateSection {
    GateSection {
        full: CommandLine::from(full),
        light: light.map(CommandLine::from),
        tests: None,
    }
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

#[test]
fn a_passing_command_logs_exit_code_zero() {
    let dir = temp_dir();
    let gate = ShellGate::new(dir.path(), &gate_section("true", None));
    let gate_log = gate.run_logged(GateLevel::Full).expect("run");
    assert_eq!(gate_log.exit_code, Some(0));
    assert!(gate_log.path.starts_with(dir.path().join(RUNS_DIR)));
    let captured = std::fs::read_to_string(&gate_log.path).expect("log");
    assert_eq!(captured, "");
}

#[test]
fn a_failing_command_logs_its_exit_code() {
    let dir = temp_dir();
    let gate = ShellGate::new(dir.path(), &gate_section("true", Some("false")));
    let gate_log = gate.run_logged(GateLevel::Light).expect("run");
    assert_eq!(gate_log.exit_code, Some(1));
    assert!(gate_log.path.is_file());
    let port: &dyn Gate = &gate;
    let failed_run = GateRun {
        verdict: GateVerdict::Failed,
        output: String::new(),
    };
    assert_eq!(port.run(GateLevel::Light), Ok(failed_run));
}

#[test]
fn stdout_and_stderr_land_in_one_file_in_the_gate_directory() {
    let dir = temp_dir();
    let command = "pwd; echo to-stdout; echo to-stderr >&2; exit 3";
    let gate = ShellGate::new(dir.path(), &gate_section(command, None));
    let gate_log = gate.run_logged(GateLevel::Full).expect("run");
    assert_eq!(gate_log.exit_code, Some(3));
    let captured = std::fs::read_to_string(&gate_log.path).expect("log");
    let workdir = dir.path().canonicalize().expect("canonical dir");
    let expected_output = format!("{}\nto-stdout\nto-stderr\n", workdir.display());
    assert_eq!(captured, expected_output);
}

#[test]
fn every_run_gets_its_own_ignored_file() {
    let dir = temp_dir();
    let gate = ShellGate::new(dir.path(), &gate_section("echo again", None));
    let port: &dyn Gate = &gate;
    let passed_run = GateRun {
        verdict: GateVerdict::Passed,
        output: "again\n".to_owned(),
    };
    assert_eq!(port.run(GateLevel::Full), Ok(passed_run.clone()));
    assert_eq!(port.run(GateLevel::Full), Ok(passed_run));
    let runs = dir.path().join(RUNS_DIR);
    let logs = std::fs::read_dir(&runs)
        .expect("runs dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("full-"))
        .count();
    assert_eq!(logs, 2);
    let ignore = std::fs::read_to_string(runs.join(".gitignore")).expect("ignore file");
    assert_eq!(ignore, "*\n");
}

#[test]
fn an_undeclared_level_is_unavailable_and_runs_nothing() {
    let dir = temp_dir();
    let gate = ShellGate::new(dir.path(), &gate_section("true", None));
    assert!(matches!(
        gate.run_logged(GateLevel::Light),
        Err(PortError::Unavailable(_))
    ));
    assert!(!dir.path().join(RUNS_DIR).exists());
}
