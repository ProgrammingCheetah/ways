//! The zaf home's list of projects through the built binary: a project joins it the first time
//! any verb runs from its root, once, and `zaf setup` adds it too. Every run gets a temp
//! project and a temp zaf home (`ZAF_HOME`).

mod common;

use std::fs;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

fn listed(project: &Project) -> Vec<String> {
    let text = fs::read_to_string(project.home.join("projects.toml")).unwrap_or_default();
    text.lines()
        .filter_map(|line| line.strip_prefix("folder = "))
        .map(|value| value.trim_matches('"').to_owned())
        .collect()
}

#[test]
fn verb_run_joins_the_list() {
    let project = Project::new(MANIFEST);
    let root = fs::canonicalize(&project.root).expect("canonical root");

    assert!(listed(&project).is_empty());
    for _ in 0..2 {
        outcome(&project.zaf(&["status"]));
    }
    assert_eq!(listed(&project), [root.display().to_string()]);

    // From a folder below the root the project is still the one root.
    let below = project.root.join("sub");
    fs::create_dir(&below).expect("subfolder");
    outcome(&project.zaf_in(&below, &["status"]));
    assert_eq!(listed(&project), [root.display().to_string()]);
}

#[test]
fn setup_adds_the_project() {
    let mut project = Project::new(MANIFEST);
    project.provide_slice_actions();
    // `zaf setup` links the host's Claude login: the machine's own must never reach a test.
    let user_home = project.temp.path().join("user-home");
    project.env("HOME", &user_home.to_string_lossy());
    let root = fs::canonicalize(&project.root).expect("canonical root");

    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(listed(&project), [root.display().to_string()]);
}
