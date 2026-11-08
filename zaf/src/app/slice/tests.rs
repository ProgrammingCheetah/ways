use super::*;
use crate::domain::TaskLabel;
use crate::fakes::FakeBoard;

const SLICE: &str = "feature/v1-1-2";
const BASE: &str = "feature/v1";

fn ticket(label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(2),
        epic: Some(IssueNumber::from(1)),
        title: "Export the list".to_owned(),
        body: String::new(),
        label: Some(label),
    }
}

fn query() -> SliceQuery {
    SliceQuery {
        task: IssueNumber::from(2),
        version: Some("v1".to_owned()),
    }
}

/// A board holding task 2 of epic 1.
fn board(label: TaskLabel) -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(label));
    board
}

#[test]
fn the_slice_name_and_base_come_from_the_manifest_patterns() {
    let board = board(TaskLabel::Claimed);
    let target = resolve(&board, &BranchesSection::default(), &query()).expect("resolve");
    assert_eq!(target.slice, SliceName::from(SLICE));
    assert_eq!(target.base, BaseName::from(BASE));
    let branches = BranchesSection {
        integration: BranchPattern::from("main"),
        task: BranchPattern::from("task-{task}"),
    };
    let unversioned = SliceQuery {
        version: None,
        ..query()
    };
    let target = resolve(&board, &branches, &unversioned).expect("resolve");
    assert_eq!(target.slice, SliceName::from("task-2"));
    assert_eq!(target.base, BaseName::from("main"));
}

#[test]
fn a_pattern_without_its_values_is_refused() {
    let board = board(TaskLabel::Claimed);
    let unversioned = SliceQuery {
        version: None,
        ..query()
    };
    assert_eq!(
        resolve(&board, &BranchesSection::default(), &unversioned),
        Err(SliceError::NoVersion)
    );
    let orphan = Ticket {
        epic: None,
        ..ticket(TaskLabel::Claimed)
    };
    board.add_ticket(orphan);
    assert_eq!(
        resolve(&board, &BranchesSection::default(), &query()),
        Err(SliceError::NoEpic {
            task: IssueNumber::from(2)
        })
    );
    let missing = SliceQuery {
        task: IssueNumber::from(9),
        ..query()
    };
    let error = resolve(&board, &BranchesSection::default(), &missing).unwrap_err();
    assert_eq!(error.step(), Some(Step::Ticket));
}
