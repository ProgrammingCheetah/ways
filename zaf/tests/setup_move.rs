//! Moving a root `ZAF.toml` to `.zaf.toml` through the built binary: only `zaf setup` run by
//! the Engineer (no `ZAF_INSTANCE`) moves it, once and said so; every other verb reads it and
//! warns on stderr. Every run gets a temp project and a temp zaf home (`ZAF_HOME`).

mod common;

use std::fs;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const MOVED: &str = "moved ZAF.toml to .zaf.toml";

/// A project whose manifest is the legacy root `ZAF.toml`: `Project::new` writes
/// `.zaf.toml`, so this writes the legacy file itself.
fn legacy_project() -> Project {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    fs::rename(
        project.root.join(".zaf.toml"),
        project.root.join("ZAF.toml"),
    )
    .expect("the legacy name");
    project
}

#[test]
fn setup_moves_the_legacy_manifest() {
    let project = legacy_project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.lines().any(|line| line == MOVED), "{stdout}");
    assert!(!project.root.join("ZAF.toml").exists());
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf.toml")).expect(".zaf.toml"),
        MANIFEST,
        "the content moves as it is"
    );
    assert!(
        !stderr.contains("zaf setup"),
        "setup does not warn about what it just moved: {stderr}"
    );

    // A second run does nothing: no move, no line about one.
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains(MOVED), "{stdout}");
    assert_eq!(stderr, "", "nothing is made, nothing is said");
    assert!(!project.root.join("ZAF.toml").exists());
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf.toml")).expect(".zaf.toml"),
        MANIFEST
    );
}

#[test]
fn setup_as_an_instance_moves_nothing() {
    let mut project = legacy_project();
    project.env("ZAF_INSTANCE", "team instance 1 on #1");
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("Engineer"), "{stdout}");
    assert!(!stdout.contains(MOVED), "{stdout}");
    assert!(project.root.join("ZAF.toml").is_file());
    assert!(!project.root.join(".zaf.toml").exists());

    // A blank ZAF_INSTANCE is not an instance.
    let mut blank = legacy_project();
    blank.env("ZAF_INSTANCE", " ");
    let (code, stdout, stderr) = outcome(&blank.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.lines().any(|line| line == MOVED), "{stdout}");
    assert!(blank.root.join(".zaf.toml").is_file());
}

#[test]
fn setup_refuses_both_manifests() {
    for new in [".zaf.toml", ".zaf/config.toml"] {
        let project = legacy_project();
        write(&project.root, new, "# the new one\n");
        let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
        assert_eq!(code, 1, "{new}: {stdout}{stderr}");
        assert!(
            stderr.contains("ZAF.toml") && stderr.contains(new),
            "{new}: it names both: {stderr}"
        );
        assert!(!stdout.contains(MOVED), "{new}: {stdout}");
        assert_eq!(
            fs::read_to_string(project.root.join("ZAF.toml")).expect("ZAF.toml"),
            MANIFEST,
            "{new}"
        );
        assert_eq!(
            fs::read_to_string(project.root.join(new)).expect("the new file"),
            "# the new one\n",
            "{new}: never overwritten"
        );
    }
}

#[test]
fn other_verbs_never_move_the_manifest() {
    for verb in [&["doctor"][..], &["asklog"], &["guard"]] {
        let project = legacy_project();
        // The first run makes the homes and says so; the second says only the warning.
        project.zaf(verb);
        let (code, stdout, stderr) = outcome(&project.zaf(verb));
        assert!(project.root.join("ZAF.toml").is_file(), "{verb:?}");
        assert!(!project.root.join(".zaf.toml").exists(), "{verb:?}");
        assert_eq!(
            stderr.lines().count(),
            1,
            "{verb:?}: one line on stderr: {stderr:?}"
        );
        assert!(
            stderr.contains("`zaf setup`") && stderr.contains("ZAF.toml"),
            "{verb:?}: {stderr}"
        );

        // The warning is stderr only: stdout and the exit code are what the new name gives.
        fs::rename(
            project.root.join("ZAF.toml"),
            project.root.join(".zaf.toml"),
        )
        .expect("the Engineer's own rename");
        let (new_code, new_stdout, new_stderr) = outcome(&project.zaf(verb));
        assert_eq!(new_stderr, "", "{verb:?}: no warning once it is moved");
        assert_eq!((code, stdout), (new_code, new_stdout), "{verb:?}");
    }
}

#[test]
fn first_run_keeps_the_legacy_manifest() {
    let project = legacy_project();
    let (_, _, stderr) = outcome(&project.zaf(&["doctor"]));
    assert!(
        project.root.join(".zaf/roles").is_dir(),
        "the homes are made: {stderr}"
    );
    assert!(project.home.join("actions").is_dir());
    assert_eq!(
        fs::read_to_string(project.root.join("ZAF.toml")).expect("ZAF.toml"),
        MANIFEST
    );
    assert!(!project.root.join(".zaf.toml").exists());
}
