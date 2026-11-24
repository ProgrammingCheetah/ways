use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::app::schedule::scheduler::{Asked, Found, Host, Memory, run};
use crate::domain::{EpicTask, Signature, Standing, TicketId, TicketState, with_release};

use super::*;
use crate::ports::PrNumber;

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
            body: "".into(),
            url: "".into(),
            state: Some(TicketState::Done),
            standing,
            group: None,
            kind: None,
            closed_at: None,
        }],
        planned_at: None,
        records: Vec::new(),
        challengers: vec![None],
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

/// A signed, planned and challenged epic of `version` with one task per standing given.
fn epic_of(id: u64, version: &str, standings: &[Standing]) -> Epic {
    let mut epic = epic(id, version, Standing::Done);
    epic.tasks = standings
        .iter()
        .enumerate()
        .map(|(index, standing)| EpicTask {
            number: TicketId::from(id * 10 + index as u64),
            title: "task".into(),
            body: "".into(),
            url: "".into(),
            state: None,
            standing: *standing,
            group: None,
            kind: None,
            closed_at: None,
        })
        .collect();
    epic
}

#[test]
fn version_with_all_dropped_epic_is_not_built() {
    let epics = [epic_of(
        1,
        "0.5.2",
        &[Standing::ClosedNotPlanned, Standing::ClosedNotPlanned],
    )];
    assert!(!is_built(&epics, "0.5.2"));
    let with_a_done_epic = [
        epic(2, "0.5.2", Standing::Done),
        epic_of(1, "0.5.2", &[Standing::ClosedNotPlanned]),
    ];
    assert!(!is_built(&with_a_done_epic, "0.5.2"));
}

#[test]
fn version_with_one_done_rest_dropped_is_built() {
    let epics = [epic_of(
        1,
        "0.5.2",
        &[
            Standing::ClosedNotPlanned,
            Standing::Done,
            Standing::ClosedNotPlanned,
        ],
    )];
    assert!(is_built(&epics, "0.5.2"));
}

/// The scheduler's host over a board held in memory: built is read from its epics, and the
/// release is only counted.
struct Board {
    epics: Vec<Epic>,
    started: Instant,
    ticked: bool,
    releases: usize,
}

impl Host for Board {
    fn catch_stop(&mut self) {}
    fn left(&mut self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    fn up(&mut self) {}
    fn now(&mut self) -> Instant {
        self.started
    }
    fn stop(&mut self) -> Option<String> {
        self.ticked.then(|| "SIGTERM".to_owned())
    }
    fn binary(&mut self) -> Found {
        Found::Nothing
    }
    fn hold_stop(&mut self) {}
    fn let_stop_through(&mut self) {}
    fn restart(&mut self) -> String {
        String::new()
    }
    fn requests(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn held(&mut self) -> bool {
        false
    }
    fn tick(&mut self, _number: u64, _asked: &Asked, _memory: &mut Memory) -> u8 {
        self.ticked = true;
        0
    }
    fn version_built(&mut self) -> bool {
        is_built(&self.epics, "0.5.2")
    }
    fn open_release(&mut self) -> Result<Opened, String> {
        self.releases += 1;
        Ok(Opened {
            version: Version::new(0, 5, 2),
            number: PrNumber::from(12),
            link: None,
        })
    }
    fn next_tick(&mut self, _interval: Duration) {}
    fn waits_for_lock(&mut self) {}
    fn pause(&mut self) {}
    fn say(&mut self, _line: &str) {}
    fn warn(&mut self, _line: &str) {}
}

/// How many release pull requests the loop opened over one tick on `epics`.
fn releases_opened(epics: Vec<Epic>) -> usize {
    let mut board = Board {
        epics,
        started: Instant::now(),
        ticked: false,
        releases: 0,
    };
    run(
        &mut board,
        Duration::from_secs(60),
        vec!["start".to_owned()],
    );
    board.releases
}

#[test]
fn all_dropped_epic_opens_no_release() {
    let dropped = epic_of(1, "0.5.2", &[Standing::ClosedNotPlanned]);
    assert_eq!(releases_opened(vec![dropped]), 0);
    // The same loop does open one for an epic with a task done.
    assert_eq!(releases_opened(vec![epic(2, "0.5.2", Standing::Done)]), 1);
}
