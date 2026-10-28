use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::RunnerName;

use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zaf-composition-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create a scratch dir");
    dir
}

#[test]
fn loads_the_manifest_from_the_project_root() {
    let root = scratch_dir("load");
    let manifest_text = "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";
    fs::write(root.join(MANIFEST_FILE), manifest_text).expect("write the manifest");
    let composition = Composition::load(&root).expect("loads");
    assert_eq!(composition.manifest().project.name.as_ref(), "bookshelf");
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn a_missing_manifest_is_a_read_error() {
    let root = scratch_dir("missing");
    assert!(matches!(
        Composition::load(&root),
        Err(ManifestError::Read { .. })
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

fn write_manifest(root: &Path, text: &str) {
    fs::write(root.join(MANIFEST_FILE), text).expect("write the manifest");
}

const FULL: &str = "[project]\nname = \"bookshelf\"\n\
[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n\
[vcs]\nkind = \"jj\"\n\
[forge]\nkind = \"github\"\nrepo = \"example/b\"\n\
[design]\nkind = \"repo-dir\"\npath = \"docs/design\"\n\
[gate]\nfull = \"true\"\n\
[runners.local]\ncommand = \"cat\"\n";

#[test]
fn every_declared_port_is_built_from_its_kind() {
    let root = scratch_dir("full");
    write_manifest(&root, FULL);
    let composition = Composition::load(&root).expect("loads");
    assert!(composition.board().is_ok());
    assert!(composition.vcs().is_ok());
    assert!(
        composition.forge().is_ok(),
        "the forge repo falls back to the board's"
    );
    assert!(composition.design().expect("builds").is_some());
    assert!(composition.gate().is_ok());
    assert!(composition.runner(&RunnerName::from("local")).is_ok());
    assert_eq!(composition.state_dir(), root.join(STATE_DIR));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn an_undeclared_port_is_absent_not_a_crash() {
    let root = scratch_dir("minimal");
    write_manifest(
        &root,
        "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n",
    );
    let composition = Composition::load(&root).expect("loads");
    assert!(matches!(
        composition.vcs(),
        Err(CompositionError::Absent { port: "Vcs", .. })
    ));
    assert!(matches!(
        composition.forge(),
        Err(CompositionError::Absent { port: "Forge", .. })
    ));
    assert!(matches!(
        composition.gate(),
        Err(CompositionError::Absent { port: "Gate", .. })
    ));
    assert!(composition.design().expect("none is fine").is_none());
    assert!(matches!(
        composition.runner(&RunnerName::from("gemini")),
        Err(CompositionError::NoRunner(name)) if name.as_ref() == "gemini"
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn discover_walks_up_to_the_manifest() {
    let root = scratch_dir("discover");
    write_manifest(&root, FULL);
    let deep = root.join("src").join("inner");
    fs::create_dir_all(&deep).expect("create nested dirs");
    let composition = Composition::discover(&deep).expect("finds the root");
    assert_eq!(composition.root(), root.as_path());
    let elsewhere = scratch_dir("nowhere");
    assert!(matches!(
        Composition::discover(&elsewhere),
        Err(CompositionError::NoProject { .. })
    ));
    fs::remove_dir_all(&root).expect("clean up");
    fs::remove_dir_all(&elsewhere).expect("clean up");
}
