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

const STALE_FORBIDDEN: &str = "profile build is older than the manifest (forbidden changed)";
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

    fs::write(
        stamp(&project),
        text.replace(env!("CARGO_PKG_VERSION"), "0.0.1"),
    )
    .expect("old");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("profile build is older than zaf (written by zaf 0.0.1)"),
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
    assert!(stdout.contains(NO_STAMP), "{stdout}");

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
