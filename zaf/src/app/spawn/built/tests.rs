use std::time::UNIX_EPOCH;

use crate::domain::{EpicTask, Signature, Standing, TicketId, TicketState, with_release};

use super::*;

/// A signed, planned and challenged epic of `version`, its one task done or open.
fn epic(id: u64, version: &str, standing: Standing) -> Epic {
    let body = with_release(version, "Goal: build it.");
    Epic {
        id: id.into(),
        title: format!("epic {id}").into(),
        signature: Some(Signature::of(&body.as_str().into(), UNIX_EPOCH)),
        body: body.as_str().into(),
        tasks: vec![EpicTask {
            number: TicketId::from(id * 10),
            title: "task".into(),
            state: Some(TicketState::Done),
            standing,
            group: None,
            kind: None,
        }],
        planned_at: None,
        challenged: true,
    }
}

#[test]
fn every_epic_of_the_version_done_is_built() {
    let epics = [
        epic(1, "0.5.2", Standing::Done),
        epic(2, "0.5.2", Standing::Done),
    ];
    assert!(is_built(&epics, "0.5.2"));
}

#[test]
fn an_open_epic_of_the_version_is_not_built() {
    let epics = [
        epic(1, "0.5.2", Standing::Done),
        epic(2, "0.5.2", Standing::Open),
    ];
    assert!(!is_built(&epics, "0.5.2"));
}

#[test]
fn an_epic_of_another_version_counts_for_nothing() {
    let epics = [
        epic(1, "0.5.2", Standing::Done),
        epic(2, "0.6.0", Standing::Open),
    ];
    assert!(is_built(&epics, "0.5.2"));
    assert!(!is_built(&epics, "0.6.0"));
}

#[test]
fn no_epic_of_the_version_is_not_built() {
    assert!(!is_built(&[], "0.5.2"));
    assert!(!is_built(&[epic(1, "0.6.0", Standing::Done)], "0.5.2"));
}
