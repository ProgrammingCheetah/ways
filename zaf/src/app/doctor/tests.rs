use std::fs;
use std::path::{Path, PathBuf};

use super::fake::FakeCommands;
use super::*;

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{HEAD}{extra}").parse().expect("a valid manifest")
}

fn status_of(report: &Report, tool: &str) -> ToolStatus {
    report
        .tools
        .iter()
        .find(|check| check.tool == tool)
        .unwrap_or_else(|| panic!("{tool} is checked"))
        .status
        .clone()
}

#[test]
fn a_tool_that_satisfies_its_requirement_is_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \">=0.20\"\n");
    let commands = FakeCommands::new().with("jj", "jj 0.23.0-a1b2c3\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert_eq!(
        status_of(&report, "jj"),
        ToolStatus::Ok {
            found: "0.23.0".into()
        }
    );
    assert!(report.passed());
    assert_eq!(commands.calls(), [["jj", "--version"]]);
}

#[test]
fn a_tool_that_cannot_start_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \"*\"\n");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert_eq!(status_of(&report, "jj"), ToolStatus::Missing);
    assert!(!report.passed());
}

#[test]
fn an_old_tool_says_found_and_wanted() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\ngh = \">=2.40\"\n");
    let commands = FakeCommands::new().with("gh", "gh version 2.31.0 (2024-01-01)\n");
    let report = doctor(dir.path(), &manifest, &commands);
    let expected = ToolStatus::TooOld {
        found: "2.31.0".into(),
        wanted: ">=2.40".into(),
    };
    assert_eq!(status_of(&report, "gh"), expected);
    assert_eq!(
        report.lines()[0],
        "gh: too old 2.31.0 < >=2.40",
        "{:?}",
        report.lines()
    );
    assert!(!report.passed());
}

#[test]
fn a_star_requirement_takes_any_version() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nnodes = \"*\"\n");
    let commands = FakeCommands::new().with("nodes", "nodes v0.0.1\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert!(status_of(&report, "nodes").is_ok());
    assert_eq!(report.lines()[0], "nodes: ok");
}

#[test]
fn a_version_line_without_semver_is_unknown_and_failing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nagy = \"*\"\n");
    let commands = FakeCommands::new().with("agy", "agy nightly build\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert_eq!(status_of(&report, "agy"), ToolStatus::Unknown);
    assert!(!report.passed());
}

#[test]
fn a_pinned_release_tag_must_match_exactly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nteams = \"teams-v2.0.0\"\n");
    let same = FakeCommands::new().with("teams", "teams 2.0.0");
    assert!(doctor(dir.path(), &manifest, &same).passed());
    let other = FakeCommands::new().with("teams", "teams 2.0.1");
    assert!(!doctor(dir.path(), &manifest, &other).passed());
}

#[test]
fn the_first_semver_skips_words_and_dates() {
    let found = |text: &str| first_semver(text).map(|version| version.to_string());
    assert_eq!(found("jj 0.23.0"), Some("0.23.0".into()));
    assert_eq!(found("v1.2.3"), Some("1.2.3".into()));
    assert_eq!(
        found("gh version 2.40.1 (2024-01-01)"),
        Some("2.40.1".into())
    );
    assert_eq!(found("tool 2024-01-01 then 3.4.5."), Some("3.4.5".into()));
    assert_eq!(found("only 1.2 here"), None);
    assert_eq!(found(""), None);
}

#[test]
fn a_declared_design_dir_must_exist() {
    let parent = tempfile::tempdir().expect("temp dir");
    let root = parent.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    let before = doctor(&root, &manifest, &FakeCommands::new());
    assert_eq!(before.design.as_ref().map(|check| check.ok), Some(false));
    assert!(!before.passed());
    assert!(before.lines()[0].starts_with("design dir: missing"));
    fs::create_dir(parent.path().join("b-design")).expect("design dir");
    assert!(doctor(&root, &manifest, &FakeCommands::new()).passed());
}

#[test]
fn no_design_kind_checks_no_design_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert_eq!(report.design, None);
    assert!(report.passed());
}

#[test]
fn the_workspaces_dir_must_be_creatable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert!(report.workspaces.ok, "a missing dir under a writable root");
    fs::write(dir.path().join("blocker"), "a file").expect("file");
    let blocked = self::manifest("[workspaces]\npath = \"blocker/ws\"\n");
    let report = doctor(dir.path(), &blocked, &FakeCommands::new());
    assert!(!report.workspaces.ok, "a file in the way");
    assert!(
        report
            .lines()
            .last()
            .expect("a line")
            .contains("not creatable")
    );
}

#[test]
fn the_report_serializes_as_a_table() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \"*\"\ngh = \"*\"\n");
    let commands = FakeCommands::new().with("jj", "jj 0.23.0");
    let report = doctor(dir.path(), &manifest, &commands);
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["tools"][0]["tool"], "gh");
    assert_eq!(json["tools"][0]["status"], "missing");
    assert_eq!(json["tools"][1]["status"], "ok");
    assert_eq!(json["tools"][1]["found"], "0.23.0");
    assert_eq!(json["workspaces"]["ok"], true);
}

#[test]
fn a_sibling_design_dir_follows_the_project_root_rule() {
    let root = Path::new("/work/project");
    let design = |text: &str| manifest(text).design;
    let beside = design(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    assert_eq!(
        design_dir(root, &beside),
        Some(PathBuf::from("/work/project/../b-design"))
    );
    let defaulted = design("[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\n");
    assert_eq!(
        design_dir(root, &defaulted),
        Some(PathBuf::from("/work/b-design"))
    );
    let inside = design("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    assert_eq!(
        design_dir(root, &inside),
        Some(PathBuf::from("/work/project/docs"))
    );
    assert_eq!(design_dir(root, &design("")), None);
}
