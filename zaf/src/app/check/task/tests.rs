use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use crate::fakes::FakeGate;
use crate::ports::GateRun;

/// A shell whose test commands pass or fail by a script, keeping every line it ran.
#[derive(Default)]
struct ScriptedShell {
    failing: Vec<&'static str>,
    ran: Mutex<Vec<String>>,
}

impl Shell for ScriptedShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        let line = line.to_string();
        self.ran.lock().expect("lock").push(line.clone());
        let failed = self.failing.iter().any(|name| line.contains(name));
        let gate_run = if failed {
            GateRun {
                verdict: GateVerdict::Failed,
                output: "running 1 test\ntest result: FAILED. 0 passed; 1 failed\n".to_owned(),
            }
        } else {
            GateRun {
                verdict: GateVerdict::Passed,
                output: "test result: ok\n".to_owned(),
            }
        };
        Ok(gate_run)
    }
}

fn template(tests: &str, architecture: &str) -> TaskTemplate {
    let body = format!(
        "Goal: export the list.\n\
         Owned paths: src/export/** (new), src/lib.rs\n\
         Interfaces: none\n\
         Design pages: none\n\
         Architecture: {architecture}\n\
         Acceptance:\n\
         \x20 - an empty list exports as an empty file\n\
         \x20 - the four cargo commands pass\n\
         Tests: {tests}\n\
         Security trigger: no\n\
         Model: sonnet\n\
         Size: S\n\
         Blocked by: —\n"
    );
    body.parse().expect("template parses")
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let export = dir.path().join("src/export");
    std::fs::create_dir_all(&export).expect("dirs");
    std::fs::write(
        export.join("writer.rs"),
        "pub struct Exporter;\npub fn write_all() {}\n",
    )
    .expect("write");
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod export;\n").expect("write");
    std::fs::write(dir.path().join("README.md"), "Importer lives elsewhere.\n").expect("write");
    dir
}

fn passing_gate() -> FakeGate {
    let gate = FakeGate::default();
    let run = GateRun {
        verdict: GateVerdict::Passed,
        output: String::new(),
    };
    gate.declare(GateLevel::Light, run);
    gate
}

fn statuses(check: &TaskCheck) -> BTreeMap<(String, String), LineStatus> {
    check
        .lines
        .iter()
        .map(|line| {
            (
                (line.kind.to_string(), line.subject.clone()),
                line.status.clone(),
            )
        })
        .collect()
}

fn key(kind: &str, subject: &str) -> (String, String) {
    (kind.to_owned(), subject.to_owned())
}

#[test]
fn a_named_test_that_passes_and_one_that_fails() {
    let dir = project();
    let gate = passing_gate();
    let shell = ScriptedShell {
        failing: vec!["exports_nothing"],
        ..ScriptedShell::default()
    };
    let tests_template = CommandLine::from("cargo test {name} -- --exact");
    let inputs = TaskInputs {
        gate: Some(&gate),
        tests: Some(&tests_template),
        shell: &shell,
        root: dir.path(),
    };
    let check = check_task(
        &template("`exports_all`, `exports_nothing`", "`Exporter`"),
        &inputs,
    );
    let found = statuses(&check);
    assert_eq!(found[&key("gate", "light")], LineStatus::Pass);
    assert_eq!(found[&key("test", "exports_all")], LineStatus::Pass);
    assert_eq!(
        found[&key("test", "exports_nothing")],
        LineStatus::Fail("test result: FAILED. 0 passed; 1 failed".to_owned())
    );
    let ran = shell.ran.lock().expect("lock").clone();
    assert_eq!(
        ran,
        [
            "cargo test exports_all -- --exact",
            "cargo test exports_nothing -- --exact"
        ]
    );
    assert_eq!(gate.runs(), [GateLevel::Light]);
    assert_eq!(check.verdict(), Verdict::MustFix);
    assert_eq!(check.verdict().code(), 1);
}

#[test]
fn an_interface_id_present_passes_and_one_missing_fails() {
    let dir = project();
    let gate = passing_gate();
    let shell = ScriptedShell::default();
    let inputs = TaskInputs {
        gate: Some(&gate),
        tests: None,
        shell: &shell,
        root: dir.path(),
    };
    let architecture =
        "module `export`; interfaces `export::Exporter`, `write_all`, `Importer`, `Export`";
    let check = check_task(&template("—", architecture), &inputs);
    let found = statuses(&check);
    assert_eq!(
        found[&key("interface", "export::Exporter")],
        LineStatus::Pass
    );
    assert_eq!(found[&key("interface", "write_all")], LineStatus::Pass);
    let missing = LineStatus::Fail("not found in the owned paths".to_owned());
    // In the tree, but outside the owned paths.
    assert_eq!(found[&key("interface", "Importer")], missing);
    // Only as part of a longer word.
    assert_eq!(found[&key("interface", "Export")], missing);
    assert_eq!(check.verdict(), Verdict::MustFix);
}

#[test]
fn prose_lines_are_listed_as_manual_and_do_not_fail() {
    let dir = project();
    let gate = passing_gate();
    let shell = ScriptedShell::default();
    let inputs = TaskInputs {
        gate: Some(&gate),
        tests: None,
        shell: &shell,
        root: dir.path(),
    };
    let check = check_task(
        &template("beside each module", "app over ports; one module per kind"),
        &inputs,
    );
    let found = statuses(&check);
    assert_eq!(
        found[&key("acceptance", "an empty list exports as an empty file")],
        LineStatus::Manual
    );
    assert_eq!(
        found[&key("acceptance", "the four cargo commands pass")],
        LineStatus::Manual
    );
    assert_eq!(
        found[&key("test", "beside each module")],
        LineStatus::Manual
    );
    assert_eq!(
        found[&key("interface", "app over ports")],
        LineStatus::Manual
    );
    assert_eq!(
        found[&key("interface", "one module per kind")],
        LineStatus::Manual
    );
    assert!(shell.ran.lock().expect("lock").is_empty());
    assert_eq!(check.verdict(), Verdict::Clean);
    assert_eq!(check.verdict().code(), 0);
}

#[test]
fn what_the_project_does_not_declare_is_skipped() {
    let dir = project();
    let shell = ScriptedShell::default();
    let no_gate = TaskInputs {
        gate: None,
        tests: None,
        shell: &shell,
        root: dir.path(),
    };
    let check = check_task(&template("exports_all", "—"), &no_gate);
    let found = statuses(&check);
    assert!(matches!(
        found[&key("gate", "light")],
        LineStatus::Skipped(_)
    ));
    assert!(matches!(
        found[&key("test", "exports_all")],
        LineStatus::Skipped(_)
    ));

    let full_only = FakeGate::default();
    let undeclared_light = TaskInputs {
        gate: Some(&full_only),
        ..no_gate
    };
    let check = check_task(&template("—", "—"), &undeclared_light);
    assert!(matches!(check.lines[0].status, LineStatus::Skipped(_)));
    assert_eq!(check.verdict(), Verdict::Clean);
}

#[test]
fn a_failing_light_gate_fails_its_line() {
    let dir = project();
    let gate = FakeGate::default();
    let failed = GateRun {
        verdict: GateVerdict::Failed,
        output: "lint: 2 warnings\n\n".to_owned(),
    };
    gate.declare(GateLevel::Light, failed);
    let shell = ScriptedShell::default();
    let inputs = TaskInputs {
        gate: Some(&gate),
        tests: None,
        shell: &shell,
        root: dir.path(),
    };
    let check = check_task(&template("—", "—"), &inputs);
    assert_eq!(
        check.lines[0].status,
        LineStatus::Fail("lint: 2 warnings".to_owned())
    );
    assert_eq!(check.verdict(), Verdict::MustFix);
}

#[test]
fn whole_words_only() {
    assert!(symbols::holds_word("pub struct Exporter;", "Exporter"));
    assert!(!symbols::holds_word("pub struct Exporter;", "Export"));
    assert!(!symbols::holds_word("my_Exporter", "Exporter"));
    assert!(symbols::holds_word("Exporter", "Exporter"));
}
