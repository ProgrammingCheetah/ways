//! This repo commits its `.zaf/` under the allow-list `zaf setup` writes: the root
//! `.gitignore` no longer ignores the folder, and `.zaf/.gitignore` is what setup writes.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

#[test]
fn ways_gitignore_matches_setup() {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let written = fs::read_to_string(project.root.join(".zaf/.gitignore")).expect("setup's file");

    let committed =
        fs::read_to_string(ways_root().join(".zaf/.gitignore")).expect("ways' .zaf/.gitignore");
    assert_eq!(committed, written, "ways' .zaf/.gitignore is not setup's");

    // A root rule over the whole folder would stop git from ever reading the allow-list.
    let root_ignore = fs::read_to_string(ways_root().join(".gitignore")).unwrap_or_default();
    for line in root_ignore.lines().map(str::trim) {
        assert!(
            !matches!(line, ".zaf" | ".zaf/" | "/.zaf" | "/.zaf/"),
            "the root .gitignore still ignores the whole .zaf/ folder: {line}"
        );
    }
}
