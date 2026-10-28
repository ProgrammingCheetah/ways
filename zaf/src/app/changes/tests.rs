use super::*;
use crate::app::slice::{SliceQuery, open, resolve};
use crate::domain::{IssueNumber, TaskLabel};
use crate::fakes::{FakeBoard, FakeVcs};
use crate::manifest::BranchesSection;
use crate::ports::{FileStat, PortError, Ticket};

fn opened(vcs: &FakeVcs) -> SliceTarget {
    let board = FakeBoard::default();
    let ticket = Ticket {
        number: IssueNumber::from(2),
        epic: Some(IssueNumber::from(1)),
        title: "Export the list".to_owned(),
        body: String::new(),
        label: Some(TaskLabel::Claimed),
    };
    board.add_ticket(ticket);
    let query = SliceQuery {
        task: IssueNumber::from(2),
        version: Some("v1".to_owned()),
    };
    let target = resolve(&board, &BranchesSection::default(), &query).expect("resolve");
    open(vcs, &target).expect("open");
    target
}

#[test]
fn the_stat_comes_first() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    let query = ChangesQuery {
        target: target.clone(),
        path: None,
    };
    assert_eq!(
        changes(&vcs, &query),
        Ok(Changes::Stat(DiffStat::default()))
    );
    vcs.edit(&target.slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
        .expect("edit");
    let stat = DiffStat {
        files: vec![FileStat {
            path: "src/export.rs".to_owned(),
            added: 12,
            removed: 3,
        }],
    };
    assert_eq!(changes(&vcs, &query), Ok(Changes::Stat(stat)));
    assert_eq!(
        changes(&vcs, &query),
        changes(&vcs, &query),
        "reading twice is the same"
    );
}

#[test]
fn hunks_come_on_request() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    vcs.edit(&target.slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
        .expect("edit");
    let query = ChangesQuery {
        target: target.clone(),
        path: Some("src/export.rs".to_owned()),
    };
    let hunks = Changes::Hunks {
        path: "src/export.rs".to_owned(),
        hunks: "@@ -1,3 +1,12 @@".to_owned(),
    };
    assert_eq!(changes(&vcs, &query), Ok(hunks));
    let unchanged = ChangesQuery {
        path: Some("README.md".to_owned()),
        ..query
    };
    assert!(matches!(
        changes(&vcs, &unchanged),
        Err(SliceError::Port {
            step: Step::Diff,
            error: PortError::NotFound(_)
        })
    ));
}

#[test]
fn a_slice_never_opened_is_not_found() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    let elsewhere = FakeVcs::default();
    let query = ChangesQuery { target, path: None };
    assert!(matches!(
        changes(&elsewhere, &query),
        Err(SliceError::Port {
            error: PortError::NotFound(_),
            ..
        })
    ));
}
