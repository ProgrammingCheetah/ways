use std::path::{Path, PathBuf};

use super::*;
use crate::app::slice::{SliceQuery, open, resolve};
use crate::domain::{IssueNumber, TaskLabel};
use crate::fakes::{FakeBoard, FakeVcs};
use crate::manifest::BranchesSection;
use crate::ports::{BaseName, FileStat, PortError, Ticket};

fn workspace() -> PathBuf {
    PathBuf::from("/project/.zaf/workspaces/feature/v1-1-2")
}

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
    vcs.place(&target.slice, &workspace()).expect("place");
    target
}

fn ask(target: &SliceTarget, path: Option<&str>) -> ChangesQuery {
    ChangesQuery {
        target: target.clone(),
        workspace: workspace(),
        path: path.map(str::to_owned),
    }
}

#[test]
fn the_stat_comes_first() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    let query = ask(&target, None);
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
    let query = ask(&target, Some("src/export.rs"));
    let hunks = Changes::Hunks {
        path: "src/export.rs".to_owned(),
        hunks: "@@ -1,3 +1,12 @@".to_owned(),
    };
    assert_eq!(changes(&vcs, &query), Ok(hunks));
    let unchanged = ask(&target, Some("README.md"));
    assert!(matches!(
        changes(&vcs, &unchanged),
        Err(SliceError::Port {
            step: Step::Diff,
            error: PortError::NotFound(_)
        })
    ));
}

#[test]
fn changes_reads_the_recorded_workspace() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    vcs.edit(&target.slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
        .expect("edit");
    let not_found = |result: Result<Changes, SliceError>| {
        matches!(
            result,
            Err(SliceError::Port {
                step: Step::Diff,
                error: PortError::NotFound(_)
            })
        )
    };

    let elsewhere = ChangesQuery {
        workspace: Path::new("/project/.zaf/workspaces/feature/v1-1-3").to_owned(),
        ..ask(&target, None)
    };
    assert!(
        not_found(changes(&vcs, &elsewhere)),
        "the slice name alone finds nothing: only the recorded workspace is read"
    );
    let elsewhere_hunks = ChangesQuery {
        path: Some("src/export.rs".to_owned()),
        ..elsewhere
    };
    assert!(not_found(changes(&vcs, &elsewhere_hunks)));

    let recorded = changes(&vcs, &ask(&target, None)).expect("the recorded workspace");
    assert!(matches!(recorded, Changes::Stat(stat) if stat.files.len() == 1));
}

#[test]
fn changes_diffs_against_the_base_from_the_pattern() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    assert_eq!(target.base, BaseName::from("feature/v1"));
    let query = ChangesQuery {
        target: SliceTarget {
            base: BaseName::from("feature/v2"),
            ..target
        },
        workspace: workspace(),
        path: None,
    };
    assert!(
        matches!(
            changes(&vcs, &query),
            Err(SliceError::Port {
                error: PortError::NotFound(_),
                ..
            })
        ),
        "a base other than the pattern's is not the slice's"
    );
}

#[test]
fn a_workspace_never_opened_is_not_found() {
    let vcs = FakeVcs::default();
    let target = opened(&vcs);
    let elsewhere = FakeVcs::default();
    let query = ask(&target, None);
    assert!(matches!(
        changes(&elsewhere, &query),
        Err(SliceError::Port {
            error: PortError::NotFound(_),
            ..
        })
    ));
}
