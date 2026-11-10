use super::*;
use crate::adapters::shell::SystemShell;
use crate::ports::Shell;

fn gate_section(full: &str, light: Option<&str>) -> GateSection {
    GateSection {
        full: CommandLine::from(full),
        light: light.map(CommandLine::from),
        tests: None,
        slot: None,
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

#[test]
fn env_is_set_beside_the_inherited_environment_and_wins_over_it() {
    let dir = temp_dir();
    let command = "echo \"$FROM_ENV_TABLE $HOME\"; test -n \"$PATH\"";
    let env = BTreeMap::from([
        ("FROM_ENV_TABLE".to_owned(), "set".to_owned()),
        ("HOME".to_owned(), "/overridden".to_owned()),
    ]);
    let gate = ShellGate::new(dir.path(), &gate_section(command, None)).with_env(env);
    let gate_log = gate.run_logged(GateLevel::Full).expect("run");
    assert_eq!(gate_log.exit_code, Some(0));
    let captured = std::fs::read_to_string(&gate_log.path).expect("log");
    assert_eq!(captured, "set /overridden\n");
}

/// A stub slot: it logs each call and what `.zaf/slots/` holds at that moment, then runs its
/// arguments. Run as `sh <stub>`, so the file needs no executable bit.
struct Rig {
    work: tempfile::TempDir,
    state: tempfile::TempDir,
    tools: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let rig = Self {
            work: temp_dir(),
            state: temp_dir(),
            tools: temp_dir(),
        };
        let script = format!(
            "echo call >> '{calls}'\ncat '{slots}'/* >> '{seen}' 2>/dev/null\necho >> '{seen}'\nexec \"$@\"\n",
            calls = rig.tools.path().join("calls").display(),
            slots = rig.state.path().join("slots").display(),
            seen = rig.tools.path().join("seen").display(),
        );
        std::fs::write(rig.tools.path().join("stub.sh"), script).expect("stub");
        rig
    }

    fn slot(&self) -> Slot {
        let prefix = format!("sh {}", self.tools.path().join("stub.sh").display());
        Slot::new(&prefix, self.state.path(), "team instance 1 on #7")
    }

    fn gate(&self, full: &str, light: &str) -> ShellGate {
        ShellGate::new(self.work.path(), &gate_section(full, Some(light)))
            .with_slot(Some(self.slot()))
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(self.tools.path().join("calls"))
            .map_or(0, |text| text.lines().count())
    }

    fn seen(&self) -> String {
        std::fs::read_to_string(self.tools.path().join("seen")).unwrap_or_default()
    }

    fn records_left(&self) -> usize {
        std::fs::read_dir(self.state.path().join("slots")).map_or(0, Iterator::count)
    }
}

#[test]
fn gate_runs_inside_the_slot() {
    let rig = Rig::new();
    let gate = rig.gate("echo full-ran", "echo light-ran");

    let full = gate.run_logged(GateLevel::Full).expect("full");
    assert_eq!(full.exit_code, Some(0));
    assert_eq!(rig.calls(), 1, "the full level calls the stub once");
    let captured = std::fs::read_to_string(&full.path).expect("log");
    assert_eq!(captured, "full-ran\n");

    let light = gate.run_logged(GateLevel::Light).expect("light");
    assert_eq!(light.exit_code, Some(0));
    assert_eq!(rig.calls(), 2, "the light level calls the stub once");

    let tests = SystemShell::new(rig.work.path(), std::time::Duration::from_secs(30))
        .with_slot(Some(rig.slot()), "tests");
    let ran = tests
        .run(&CommandLine::from("echo test-ran"))
        .expect("tests");
    assert_eq!(ran.verdict, GateVerdict::Passed);
    assert_eq!(ran.output, "test-ran\n");
    assert_eq!(rig.calls(), 3, "the tests level calls the stub once");

    let seen = rig.seen();
    for level in ["full", "light", "tests"] {
        assert!(
            seen.contains(&format!("\"level\":\"{level}\"")),
            "a record names {level} while the stub holds the run: {seen}"
        );
    }
    assert!(seen.contains("team instance 1 on #7"), "{seen}");
    assert_eq!(rig.records_left(), 0, "every record is gone after its run");
}

#[test]
fn a_failing_line_still_frees_the_slot_record() {
    let rig = Rig::new();
    let gate = rig.gate("exit 4", "true");
    let log = gate.run_logged(GateLevel::Full).expect("runs");
    assert_eq!(
        log.exit_code,
        Some(4),
        "the slot passes the exit code through"
    );
    assert_eq!(rig.records_left(), 0);
}

#[test]
fn a_nested_gate_takes_no_second_slot() {
    let rig = Rig::new();
    let manifest = format!(
        "[project]\nname = \"p\"\n[board]\nkind = \"github-issues\"\nrepo = \"o/p\"\n\
         [gate]\nfull = \"echo nested\"\nslot = \"sh {}\"\n",
        rig.tools.path().join("stub.sh").display()
    );
    let root = temp_dir();
    std::fs::write(
        root.path().join(crate::composition::MANIFEST_FILE),
        manifest,
    )
    .expect("manifest");
    let composition =
        crate::composition::Composition::load_with_home(root.path(), None).expect("loads");

    assert!(composition.slot_unless(false, "the Engineer").is_some());
    let held = composition.slot_unless(true, "the Engineer");
    assert!(held.is_none(), "a run inside a held slot takes none");

    let section = composition.manifest().gate.clone().expect("gate");
    let nested = ShellGate::new(rig.work.path(), &section).with_slot(held);
    let log = nested.run_logged(GateLevel::Full).expect("runs");
    assert_eq!(log.exit_code, Some(0));
    assert_eq!(std::fs::read_to_string(&log.path).expect("log"), "nested\n");
    assert_eq!(rig.calls(), 0, "the stub is never called");
    assert_eq!(rig.records_left(), 0);
}

#[test]
fn an_action_inside_the_slot_sees_the_variable() {
    let rig = Rig::new();
    let line = "echo \"line=$ZAF_SLOT_HELD\"; sh -c 'echo \"child=$ZAF_SLOT_HELD\"'";
    let gate = rig.gate(line, "true");
    let log = gate.run_logged(GateLevel::Full).expect("runs");
    assert_eq!(
        std::fs::read_to_string(&log.path).expect("log"),
        "line=1\nchild=1\n"
    );
}

#[test]
fn no_slot_runs_the_line_as_before() {
    let dir = temp_dir();
    let gate = ShellGate::new(dir.path(), &gate_section("echo plain", None));
    let log = gate.run_logged(GateLevel::Full).expect("runs");
    assert_eq!(log.exit_code, Some(0));
    assert_eq!(std::fs::read_to_string(&log.path).expect("log"), "plain\n");
    assert!(!dir.path().join("slots").exists());
}
