//! `zaf slice open|pr|merge|drop` are aliases of `zaf do slice-open|slice-pr|slice-merge|
//! slice-drop`; `zaf check` and `zaf changes` name a missing `slice-open` the same way. The built
//! binary on a temp manifest, the fake `gh` standing in for the board, and actions written by the
//! test: zaf ships none.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[vcs]
kind = "jj"

[forge]
kind = "github"
repo = "example/tracker"

[gate]
full = "true"
light = "true"
"#;

/// Task 2 of epic 1, claimed, as `gh issue view` prints it.
const TASK: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// The line every verb prints for a missing action `name`.
fn missing(name: &str) -> String {
    format!(
        "missing action {name}: no {name} action in .zaf/actions or the machine's actions \
         (zaf ships none; see docs/zaf-actions.md §4)"
    )
}

/// A project with no actions, and a fake `gh` that knows task 2.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project.on("issue view 2 *", TASK).env("ZAF_VERSION", "v1");
    project
}

/// Writes an executable action into the project's `.zaf/actions`.
fn provide(project: &Project, name: &str, body: &str) {
    let actions = project.root.join(".zaf/actions");
    write(
        &actions,
        name,
        &format!("#!/bin/sh\n# zaf description: a test action\n# zaf 1: failed\n{body}\n"),
    );
    fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o755)).expect("mode");
}

/// The workspace a test `slice-open` hands back.
fn workspace(project: &Project) -> PathBuf {
    project.root.join(".zaf/workspaces/feature/v1-1-2")
}

#[test]
fn slice_open_names_the_missing_action() {
    let project = project();
    for (verb, name) in [
        ("open", "slice-open"),
        ("pr", "slice-pr"),
        ("merge", "slice-merge"),
        ("drop", "slice-drop"),
    ] {
        let (code, stdout, stderr) = outcome(&project.zaf(&["slice", verb, "2"]));
        assert_eq!(code, 2, "slice {verb}: {stderr}");
        assert_eq!(stdout, "", "slice {verb}");
        assert!(
            stderr.contains(&missing(name)),
            "slice {verb} names {name}: {stderr}"
        );
    }

    let (code, stdout, _) = outcome(&project.zaf(&["--json", "slice", "open", "2"]));
    assert_eq!(code, 2);
    let error: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(error["error"], missing("slice-open"));
}

#[test]
fn check_and_changes_name_the_missing_action() {
    let project = project();
    for verb in ["check", "changes"] {
        let (code, _, stderr) = outcome(&project.zaf(&[verb, "2"]));
        assert_eq!(code, 2, "{verb}: {stderr}");
        assert!(
            stderr.contains(&missing("slice-open")),
            "{verb} names slice-open: {stderr}"
        );
    }

    // With `slice-open` provided, the task is merely not open yet.
    provide(&project, "slice-open", "true");
    for verb in ["check", "changes"] {
        let (code, _, stderr) = outcome(&project.zaf(&[verb, "2"]));
        assert_eq!(code, 1, "{verb}: {stderr}");
        assert!(
            stderr.contains("slice for #2 is not open; run zaf do slice-open 2"),
            "{verb}: {stderr}"
        );
        assert!(!stderr.contains("missing action"), "{verb}: {stderr}");
    }
}

#[test]
fn slice_open_alias_writes_the_record() {
    let project = project();
    let dir = workspace(&project);
    fs::create_dir_all(&dir).expect("workspace");
    provide(
        &project,
        "slice-open",
        &format!("echo opening\necho '{}'", dir.display()),
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["slice", "open", "2"]));

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("{}\n", dir.display()));
    let record = fs::read_to_string(project.root.join(".zaf/slices/2")).expect("the record");
    assert_eq!(record.trim(), dir.to_string_lossy());

    // The slice is found: neither the missing action nor the not-open refusal.
    let (code, _, stderr) = outcome(&project.zaf(&["check", "2"]));
    assert!(
        !stderr.contains("is not open") && !stderr.contains("missing action"),
        "{stderr}"
    );
    assert_ne!(code, 2, "{stderr}");
}

#[test]
fn a_slice_alias_exits_with_the_actions_own_code() {
    let project = project();
    provide(&project, "slice-drop", "echo gone\nexit 1");

    let (code, _, stderr) = outcome(&project.zaf(&["slice", "drop", "2"]));

    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("FAIL slice-drop: exit 1"), "{stderr}");
}
