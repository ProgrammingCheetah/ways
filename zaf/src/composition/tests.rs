use std::fs;
use std::path::PathBuf;

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
