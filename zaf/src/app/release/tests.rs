use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{Signature, Ticket, TicketId, TicketState, all_signed, with_release};
use crate::fakes::FakeBoard;
use crate::ports::Board;

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
fn signed_design_without_an_epic_is_nothing_due() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
    let other = [epic_of("0.7.0")];
    assert_eq!(phase_due(&folder, &[], "0.6.0").phase, Phase::Nothing);
    assert_eq!(phase_due(&folder, &other, "0.6.0").phase, Phase::Nothing);
}

const VERSION: &str = "0.6.0";

/// A folder under `root` with both documents signed.
fn signed_folder(root: &Path) {
    let folder = folder_in(root, "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
}

/// Puts an open epic of the version on the board, signed or not; `done` gives it a done task and
/// the challenger's report, so that it reads as done.
fn put_epic(board: &FakeBoard, id: u64, signed: bool, done: bool) {
    let id = TicketId::from(id);
    if done {
        board.add_challenged_epic(id, "epic");
    } else {
        board.add_epic(id, "epic");
    }
    let body = with_release(VERSION, "Goal: build it.");
    board.set_body(id, &body);
    if signed || done {
        board
            .sign(id, &Signature::of(&body.as_str().into(), SIGNED_AT))
            .expect("signs");
    }
    if done {
        board.add_ticket(Ticket {
            id: TicketId::from(id.get() + 100),
            epic: Some(id),
            title: "task".into(),
            body: "".into(),
            state: Some(TicketState::Done),
            url: "".into(),
            kind: None,
        });
    }
}

fn stage_at(root: &Path, board: &FakeBoard) -> Stage {
    stage(root, &manifest(""), board, VERSION).expect("the stage reads")
}

#[test]
fn stage_objectives_unsigned() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert_eq!(
        stage_at(temp.path(), &FakeBoard::default()),
        Stage::Objectives
    );
}

#[test]
fn stage_design_unsigned() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(temp.path(), "");
    write_signed(&folder, "objectives.md", "Objectives.\n");
    assert_eq!(stage_at(temp.path(), &FakeBoard::default()), Stage::Design);
}

#[test]
fn stage_epics_while_one_unsigned() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_folder(temp.path());
    let board = FakeBoard::default();
    put_epic(&board, 7, false, false);
    let Stage::Epics(epics) = stage_at(temp.path(), &board) else {
        panic!("expected the epics stage");
    };
    assert_eq!(epics.len(), 1);
    assert!(epics[0].unsigned);
    assert_eq!(epics[0].epic.number, TicketId::from(7));
}

#[test]
fn stage_build_when_all_signed() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_folder(temp.path());
    let board = FakeBoard::default();
    put_epic(&board, 7, true, false);
    put_epic(&board, 8, true, true);
    assert_eq!(stage_at(temp.path(), &board), Stage::Build);
}

#[test]
fn stage_built_when_all_done() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_folder(temp.path());
    let board = FakeBoard::default();
    put_epic(&board, 7, true, true);
    assert_eq!(stage_at(temp.path(), &board), Stage::Built);
}

#[test]
fn stage_built_when_no_open_epic_after_design() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_folder(temp.path());
    assert_eq!(stage_at(temp.path(), &FakeBoard::default()), Stage::Built);
}

#[test]
fn due_releases_leaves_out_a_built_version() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_folder(temp.path());
    let due = due_releases(temp.path(), &manifest(""), &[], &|_| false);
    assert_eq!(due, Vec::new());
}

#[test]
fn all_signed_false_without_epics() {
    assert!(!all_signed(&[], VERSION));
    assert!(!all_signed(&[epic_of("0.7.0")], VERSION));
}

#[test]
fn all_signed_false_with_one_unsigned() {
    let mut unsigned = epic_of(VERSION);
    unsigned.signature = None;
    assert!(!all_signed(&[epic_of(VERSION), unsigned], VERSION));
}

#[test]
fn all_signed_true_when_every_epic_signed() {
    assert!(all_signed(&[epic_of(VERSION), epic_of(VERSION)], VERSION));
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

fn release_folders(root: &Path, names: &[&str]) {
    for name in names {
        std::fs::create_dir_all(root.join(".teams/releases").join(name)).expect("folder");
    }
}

fn next(root: &Path, extra: &str, vcs: &crate::fakes::FakeVcs) -> String {
    next_version(root, &manifest(extra), Some(vcs)).to_string()
}

const TAGGED: &str = "[release]\ntag = \"zaf-v{version}\"\n";

#[test]
fn next_version_with_nothing_is_0_1_0() {
    let temp = tempfile::tempdir().expect("temp dir");
    let vcs = crate::fakes::FakeVcs::default();
    assert_eq!(next(temp.path(), "", &vcs), "0.1.0");
    assert_eq!(next(temp.path(), TAGGED, &vcs), "0.1.0");
    assert_eq!(
        next_version(temp.path(), &manifest(""), None).to_string(),
        "0.1.0"
    );
}

#[test]
fn next_version_from_folders_alone() {
    let temp = tempfile::tempdir().expect("temp dir");
    release_folders(temp.path(), &["0.4.3", "0.5.2"]);
    let vcs = crate::fakes::FakeVcs::default();
    vcs.tag("zaf-v9.0.0");
    // No `[release] tag`: the tags are not read.
    assert_eq!(next(temp.path(), "", &vcs), "0.6.0");
    assert_eq!(
        next_version(temp.path(), &manifest(""), None).to_string(),
        "0.6.0"
    );
}

#[test]
fn next_version_takes_the_highest_of_tags_and_folders() {
    let temp = tempfile::tempdir().expect("temp dir");
    release_folders(temp.path(), &["0.5.2"]);
    let vcs = crate::fakes::FakeVcs::default();
    vcs.tag("zaf-v0.5.1");
    assert_eq!(next(temp.path(), TAGGED, &vcs), "0.6.0");
    vcs.tag("zaf-v0.7.0");
    assert_eq!(next(temp.path(), TAGGED, &vcs), "0.8.0");
    // A tag of another pattern is no release tag.
    vcs.tag("other-v3.0.0");
    assert_eq!(next(temp.path(), TAGGED, &vcs), "0.8.0");
}

#[test]
fn next_version_skips_names_that_are_not_versions() {
    let temp = tempfile::tempdir().expect("temp dir");
    release_folders(temp.path(), &["0.4.3", "0.5.2", "notes", "1.x"]);
    let vcs = crate::fakes::FakeVcs::default();
    vcs.tag("zaf-vnext");
    vcs.tag("zaf-v2");
    assert_eq!(next(temp.path(), TAGGED, &vcs), "0.6.0");
}
