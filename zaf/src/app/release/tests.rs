use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{Signature, with_release};

use super::*;

const MINIMAL: &str =
    "[project]\nname = \"bookshelf\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{MINIMAL}\n{extra}")
        .parse()
        .expect("the manifest loads")
}

fn folder_in(root: &Path, extra: &str) -> ReleaseFolder {
    ReleaseFolder::of(root, &manifest(extra), "0.6.0")
}

#[test]
fn release_folder_defaults_to_teams_releases() {
    let root = Path::new("/project");
    let folder = folder_in(root, "");
    assert_eq!(folder.path(), root.join(".teams/releases/0.6.0"));
    assert_eq!(
        folder.design_document(),
        root.join(".teams/releases/0.6.0/design.md")
    );
}

#[test]
fn release_folder_reads_the_manifest_pattern() {
    let root = Path::new("/project");
    let folder = folder_in(root, "[release]\nfolder = \"papers/{version}\"\n");
    assert_eq!(folder.path(), root.join("papers/0.6.0"));
}

#[test]
fn a_release_folder_pattern_must_hold_the_version_and_stay_relative() {
    for folder in ["papers/next", "/papers/{version}"] {
        let text = format!("{MINIMAL}\n[release]\nfolder = \"{folder}\"\n");
        assert!(text.parse::<Manifest>().is_err(), "{folder} is refused");
    }
}

#[test]
fn objectives_md_wins_over_scope_md() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    folder.ensure().expect("ensure");
    std::fs::write(folder.path().join("scope.md"), "scope").expect("scope");
    std::fs::write(folder.path().join("objectives.md"), "objectives").expect("objectives");
    assert_eq!(
        folder.objectives_document(),
        Some(folder.path().join("objectives.md"))
    );
}

#[test]
fn scope_md_is_read_when_alone() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    folder.ensure().expect("ensure");
    assert_eq!(folder.objectives_document(), None, "an empty folder");
    std::fs::write(folder.path().join("scope.md"), "scope").expect("scope");
    assert_eq!(
        folder.objectives_document(),
        Some(folder.path().join("scope.md"))
    );
}

#[test]
fn a_missing_release_folder_names_no_objectives() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert_eq!(folder_in(temp.path(), "").objectives_document(), None);
}

const SIGNED_AT: SystemTime = UNIX_EPOCH;

fn write(folder: &ReleaseFolder, name: &str, text: &str) {
    folder.ensure().expect("ensure");
    std::fs::write(folder.path().join(name), text).expect("write");
}

fn write_signed(folder: &ReleaseFolder, name: &str, body: &str) {
    write(folder, name, &DocumentSignature::sign(body, SIGNED_AT));
}

fn epic_of(version: &str) -> Epic {
    let body = with_release(version, "Goal: build it.");
    Epic {
        id: 7.into(),
        title: "release".into(),
        signature: Some(Signature::of(&body.as_str().into(), SIGNED_AT)),
        body: body.as_str().into(),
        tasks: Vec::new(),
        planned_at: None,
        challenged: false,
    }
}

#[test]
fn empty_folder_is_objectives_due() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    assert_eq!(
        phase_due(&folder, &[], "0.6.0"),
        PhaseDue {
            phase: Phase::Objectives,
            malformed: Vec::new()
        }
    );
    folder.ensure().expect("ensure");
    assert_eq!(phase_due(&folder, &[], "0.6.0").phase, Phase::Objectives);
}

#[test]
fn signed_scope_without_design_is_design_due() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "scope.md", "Scope.\n");
    assert_eq!(phase_due(&folder, &[], "0.6.0").phase, Phase::Design);
}

#[test]
fn signed_design_without_an_epic_is_design_due() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
    let other = [epic_of("0.7.0")];
    assert_eq!(phase_due(&folder, &[], "0.6.0").phase, Phase::Design);
    assert_eq!(phase_due(&folder, &other, "0.6.0").phase, Phase::Design);
}

#[test]
fn signed_design_with_an_epic_is_nothing_due() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
    let due = phase_due(&folder, &[epic_of("0.6.0")], "0.6.0");
    assert_eq!(due.phase, Phase::Nothing);
    assert!(due.malformed.is_empty());
}

#[test]
fn edited_objectives_make_objectives_due_again() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
    let epics = [epic_of("0.6.0")];
    assert_eq!(phase_due(&folder, &epics, "0.6.0").phase, Phase::Nothing);
    let path = folder.path().join("objectives.md");
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, format!("{text}One more objective.\n")).expect("edit");
    assert_eq!(phase_due(&folder, &epics, "0.6.0").phase, Phase::Objectives);
}

#[test]
fn malformed_block_is_objectives_due_with_its_line() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write(
        &folder,
        "objectives.md",
        "---\nsigned: yesterday\nhash: nope\n---\nBody.\n",
    );
    let due = phase_due(&folder, &[], "0.6.0");
    assert_eq!(due.phase, Phase::Objectives);
    assert_eq!(due.malformed.len(), 1);
    assert_eq!(due.malformed[0].path, folder.path().join("objectives.md"));
    assert_eq!(due.malformed[0].line, 2);
}

#[test]
fn ensure_creates_a_missing_release_folder() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    assert!(!folder.path().exists());
    folder.ensure().expect("ensure creates it");
    assert!(folder.path().is_dir());
    std::fs::write(folder.path().join("design.md"), "kept").expect("write");
    folder.ensure().expect("ensure leaves it");
    assert_eq!(
        std::fs::read_to_string(folder.design_document()).expect("read"),
        "kept"
    );
}
