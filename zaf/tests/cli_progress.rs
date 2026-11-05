//! `zaf progress` through the built binary: the phases a model declares and reports, and the hub
//! report each call leaves.

mod common;

use std::fs;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const INSTANCE: &str = "programmer instance 1 on #103";

fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project.env("ZAF_INSTANCE", INSTANCE);
    project
}

fn reports(project: &Project) -> Vec<String> {
    fs::read_to_string(project.root.join(".zaf/hub/reports"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn plan_file(project: &Project, name: &str) -> String {
    fs::read_to_string(project.root.join(".zaf/hub/plans").join(name)).unwrap_or_default()
}

#[test]
fn progress_plan_stores_the_phases() {
    let project = project();
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["progress", "plan", "330", "read", "write", "test"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        plan_file(&project, "330.programmer"),
        "plan read write test\n"
    );
    let reports = reports(&project);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].contains(" 330 programmer "), "{reports:?}");
    // Another role on the same task keeps its own plan.
    let (code, _, stderr) = outcome(&project.zaf(&[
        "progress", "plan", "330", "--role", "keeper", "search", "answer",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(plan_file(&project, "330.keeper"), "plan search answer\n");
    assert_eq!(
        plan_file(&project, "330.programmer"),
        "plan read write test\n"
    );
    // A plan with no phase, or a phase twice, stores nothing.
    let (code, _, stderr) = outcome(&project.zaf(&["progress", "plan", "331", "read", "read"]));
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(plan_file(&project, "331.programmer"), "");
}

#[test]
fn progress_at_leaves_a_report_naming_the_phase() {
    let project = project();
    project.zaf(&["progress", "plan", "330", "read", "write"]);
    let (code, stdout, stderr) = outcome(&project.zaf(&["progress", "at", "330", "write"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("write"), "{stdout}");
    let reports = reports(&project);
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert!(reports[1].ends_with(" 330 programmer write"), "{reports:?}");
}

#[test]
fn progress_at_an_undeclared_phase_refuses() {
    let project = project();
    project.zaf(&["progress", "plan", "330", "read", "write"]);
    let before = plan_file(&project, "330.programmer");
    let reports_before = reports(&project);
    let (code, stdout, stderr) = outcome(&project.zaf(&["progress", "at", "330", "deploy"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("deploy"), "{stderr}");
    assert_eq!(plan_file(&project, "330.programmer"), before);
    assert_eq!(
        reports(&project),
        reports_before,
        "a refused phase leaves no report"
    );
    // Nor does a phase on a task with no plan.
    let (code, _, stderr) = outcome(&project.zaf(&["progress", "at", "999", "read"]));
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(plan_file(&project, "999.programmer"), "");
}

#[test]
fn a_repeated_phase_counts_an_iteration_through_the_verb() {
    let project = project();
    project.zaf(&["progress", "plan", "330", "write", "test"]);
    for phase in ["write", "test", "write"] {
        let (code, _, stderr) = outcome(&project.zaf(&["progress", "at", "330", phase]));
        assert_eq!(code, 0, "{stderr}");
    }
    let (code, stdout, _) = outcome(&project.zaf(&["--json", "progress", "at", "330", "write"]));
    assert_eq!(code, 0);
    assert!(stdout.contains("\"iteration\":2"), "{stdout}");
    let (_, stdout, _) = outcome(&project.zaf(&["progress", "at", "330", "test"]));
    assert!(stdout.contains("test ×2"), "{stdout}");
}

#[test]
fn progress_role_defaults_to_the_instance() {
    let project = project();
    // The instance names the role.
    let (code, _, stderr) = outcome(&project.zaf(&["progress", "plan", "330", "read"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(plan_file(&project, "330.programmer"), "plan read\n");
    // --role overrides it.
    let (code, _, stderr) =
        outcome(&project.zaf(&["progress", "plan", "330", "--role", "keeper", "look"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(plan_file(&project, "330.keeper"), "plan look\n");
    // With neither, the verb exits 1 and stores nothing.
    let bare = Project::new(MANIFEST);
    let (code, _, stderr) = outcome(&bare.zaf(&["progress", "plan", "330", "read"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("ZAF_INSTANCE"), "{stderr}");
    assert!(!bare.root.join(".zaf/hub/plans").exists());
    let (code, _, stderr) = outcome(&bare.zaf(&["progress", "at", "330", "read"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(reports(&bare).is_empty());
    // A blank instance is refused as well, naming the value.
    let mut blank = Project::new(MANIFEST);
    blank.env("ZAF_INSTANCE", "  ");
    let (code, _, stderr) = outcome(&blank.zaf(&["progress", "plan", "330", "read"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("ZAF_INSTANCE"), "{stderr}");
}
