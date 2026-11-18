//! A profile older than the manifest or zaf, through the binary: `zaf spawn` refuses it, `zaf
//! doctor` fails on it and `zaf status` names it, each with the same line; `zaf profile init`
//! makes all three pass again.

mod common;

use std::fs;
use std::path::Path;

use common::{Project, outcome};

const HEAD: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n";

const PUSH: &str = "[forbidden]\ncommands = [\"git push\"]\n";
const MERGE: &str = "[forbidden]\ncommands = [\"gh pr merge\"]\n";

/// Every role's profile, lean and build, is stale in these projects, so they read as one sentence.
const STALE_FORBIDDEN: &str =
    "profiles lean and build are older than the manifest (forbidden changed)";
const NO_STAMP: &str = "profile build has no stamp: run zaf profile init";

const SPAWN: [&str; 4] = ["spawn", "--once", "--version", "0.2.1"];

/// A project whose profiles were generated from the `PUSH` manifest, with the board empty.
fn generated() -> Project {
    let mut project = Project::new(&format!("{HEAD}{PUSH}"));
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// Edits `[forbidden]` without running `zaf profile init`.
fn edit_forbidden(project: &Project) {
    fs::write(project.root.join(".zaf.toml"), format!("{HEAD}{MERGE}")).expect("manifest");
}

fn stamp(project: &Project) -> std::path::PathBuf {
    project.root.join(".zaf/profiles/build/.zaf-stamp.json")
}

fn launched(root: &Path) -> bool {
    root.join(".zaf/spawn.state").exists()
}

/// A project with two roles, so two generated profiles, both stamped from the `PUSH` manifest and
/// then made stale by the `MERGE` one.
fn generated_two() -> Project {
    let head = format!(
        "{HEAD}[roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\nprofile = \"lean\"\nturns = 5\nbatch = 2\n\n"
    );
    let mut project = Project::new(&format!("{head}{PUSH}"));
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    fs::write(project.root.join(".zaf.toml"), format!("{head}{MERGE}")).expect("manifest");
    project
}

const BOTH: &str = STALE_FORBIDDEN;

fn count(text: &str, needle: &str) -> usize {
    text.lines().filter(|line| line.contains(needle)).count()
}

#[test]
fn spawn_refuses_with_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(count(&stderr, "older than"), 1, "{stderr}");
    assert!(stderr.contains(BOTH), "{stderr}");
    assert!(!launched(&project.root), "nothing was launched");
}

#[test]
fn doctor_prints_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(count(&stdout, "older than"), 1, "{stdout}");
    assert!(stdout.contains(BOTH), "{stdout}");
}

#[test]
fn status_prints_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(count(&stdout, "older than"), 1, "{stdout}");
    assert!(stdout.contains(BOTH), "{stdout}");
}

#[test]
fn a_current_profile_spawns() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn spawn_refuses_a_stale_profile() {
    let project = generated();
    edit_forbidden(&project);

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains(STALE_FORBIDDEN), "{stderr}");
    assert!(!launched(&project.root), "nothing was launched");

    let dry = ["spawn", "--once", "--version", "0.2.1", "--dry-run"];
    let (code, stdout, stderr) = outcome(&project.zaf(&dry));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains(STALE_FORBIDDEN), "{stderr}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&dry));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn spawn_refuses_a_profile_with_no_stamp_or_written_by_another_zaf() {
    let project = generated();
    let text = fs::read_to_string(stamp(&project)).expect("stamp");

    let other_zaf = text.replace(env!("CARGO_PKG_VERSION"), "0.0.1");
    fs::write(stamp(&project), &other_zaf).expect("other zaf");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "the same generation is current: {stderr}");

    assert!(other_zaf.contains("\"generation\": 1"), "{other_zaf}");
    fs::write(
        stamp(&project),
        other_zaf.replace("\"generation\": 1", "\"generation\": 0"),
    )
    .expect("old");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("profile build is of another profile generation (written by zaf 0.0.1)"),
        "{stderr}"
    );

    fs::remove_file(stamp(&project)).expect("remove");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains(NO_STAMP), "{stderr}");
}

#[test]
fn spawn_refuses_a_profile_that_was_never_generated() {
    let project = generated();
    fs::remove_dir_all(project.root.join(".zaf/profiles/build")).expect("remove");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains(NO_STAMP), "{stderr}");
}

#[test]
fn doctor_names_a_stale_profile() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");

    edit_forbidden(&project);
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.contains(STALE_FORBIDDEN), "{stdout}");

    fs::remove_file(stamp(&project)).expect("remove");
    let (code, stdout, _) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1);
    assert!(stdout.contains("profile build has no stamp"), "{stdout}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn status_names_a_stale_profile() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");

    edit_forbidden(&project);
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains(STALE_FORBIDDEN), "{stdout}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");
}

#[test]
fn a_second_init_reports_nothing_written() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), "nothing changed");
}
