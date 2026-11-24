use std::path::Path;
use std::time::UNIX_EPOCH;

use super::*;
use crate::domain::DocumentSignature;
use crate::fakes::{FakeBoard, FakeVcs};

const PLAN: &str = "Task: First\nNode: A\nGoal: a works\nScope: s\nOut of scope: o\n\
    Interfaces: none\nDesign pages: D2\nAcceptance:\n- a\nTests: t\nSecurity trigger: no\n\
    Model: sonnet\nSize: S\nWrites: a\nProvides: a.api\nNeeds: none\n";

const DESIGN: &str =
    "```toml vocabulary\n[areas]\n\"a\" = \"a\"\n[names.\"a.api\"]\nsignature = \"fn a()\"\n```\n";

fn manifest() -> Manifest {
    "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n"
        .parse()
        .expect("manifest")
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_epic(7.into(), "release");
    board.set_body(7.into(), "Release: 0.6.0\n\nThe epic.");
    board
}

fn run(root: &Path, board: &FakeBoard, epic: u64, plan: &str) -> Result<Checked, CheckError> {
    let vcs = FakeVcs::default();
    check(
        root,
        &manifest(),
        board,
        &vcs,
        TicketId::from(epic),
        plan.as_bytes(),
    )
}

fn signed_design(root: &Path) {
    let folder = root.join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(DESIGN, UNIX_EPOCH),
    )
    .expect("design");
}

#[test]
fn a_good_plan_gives_its_measures() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_design(temp.path());
    let checked = run(temp.path(), &board(), 7, PLAN).expect("checked");
    assert_eq!(checked.before.nodes, 1);
    assert_eq!(checked.after.nodes, 1);
}

#[test]
fn a_ticket_that_is_no_open_epic_is_refused() {
    let temp = tempfile::tempdir().expect("temp dir");
    let error = run(temp.path(), &board(), 99, PLAN).expect_err("refused");
    assert!(matches!(error, CheckError::NotAnEpic(_)), "{error}");
}

#[test]
fn an_unsigned_design_is_refused_before_the_plan_is_read() {
    let temp = tempfile::tempdir().expect("temp dir");
    let error = run(temp.path(), &board(), 7, "not a plan").expect_err("refused");
    assert!(matches!(error, CheckError::Vocabulary(_)), "{error}");
}

#[test]
fn a_name_the_vocabulary_gives_an_older_epic_is_refused_by_the_check() {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = temp.path().join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    let design = DESIGN.replace("signature = \"fn a()\"", "signature = \"fn a()\"\nepic = 8");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(&design, UNIX_EPOCH),
    )
    .expect("design");
    let board = board();
    board.add_epic(8.into(), "older");
    board.set_body(8.into(), "Release: 0.6.0\n\nThe older epic.");
    board.add_ticket(crate::domain::Ticket {
        id: 9.into(),
        epic: Some(8.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: None,
        url: "".into(),
        kind: None,
    });
    let plan = PLAN.replace(
        "Provides: a.api\nNeeds: none",
        "Provides: none\nNeeds: a.api",
    );
    let error = run(temp.path(), &board, 7, &plan).expect_err("refused");
    assert!(matches!(error, CheckError::OlderEpic(_)), "{error}");
    assert!(error.to_string().contains("#8"), "{error}");
}

#[test]
fn a_plan_that_does_not_read_gives_its_faults() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_design(temp.path());
    let error = run(temp.path(), &board(), 7, "not a plan").expect_err("refused");
    assert!(matches!(error, CheckError::Unreadable(_)), "{error}");
}

/// D4, D13: a need on a filed task of another epic, written `#<n>`, is a kept wait and checks.
#[test]
fn a_need_on_a_filed_task_by_number_checks() {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_design(temp.path());
    let board = board();
    board.add_epic(8.into(), "older");
    board.set_body(8.into(), "Release: 0.6.0\n\nThe older epic.");
    board.add_ticket(crate::domain::Ticket {
        id: 9.into(),
        epic: Some(8.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: None,
        url: "".into(),
        kind: None,
    });
    let plan = PLAN.replace("Needs: none", "Needs: #9");

    let checked = run(temp.path(), &board, 7, &plan).expect("checked");
    assert_eq!(checked.after.nodes, 1);

    // A number no task has is no name anybody knows.
    let plan = PLAN.replace("Needs: none", "Needs: #404");
    let error = run(temp.path(), &board, 7, &plan).expect_err("refused");
    assert!(format!("{error:?}").contains("#404"), "{error:?}");
}
