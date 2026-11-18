use std::collections::BTreeSet;
use std::time::Duration;

use super::*;
use crate::app::schedule::gather::Unreadable;
use crate::domain::schedule::{
    ActionName, EpicFacts, HeadEnd, Phase, PhaseEnd, PullFacts, RunRecord, TicketFacts,
};
use crate::domain::{CommitId, TaskSize, TicketId};

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// Ticket `number` with implement (opening the pull request), ci and merge, and nothing else.
fn facts(number: u64) -> TicketFacts {
    let phase = |name: &str, end: PhaseEnd| Phase {
        name: ActionName::from(name),
        ends: vec![end],
        on_fail: None,
    };
    TicketFacts {
        ticket: TicketId::from(number),
        epic: None,
        open_questions: BTreeSet::new(),
        dependencies: Vec::new(),
        pull: None,
        runs: Vec::new(),
        phases: vec![
            phase("implement", PhaseEnd::PrOpen),
            phase("ci", PhaseEnd::CiDone),
            phase("merge", PhaseEnd::Merged),
        ],
        size: TaskSize::S,
        priority: None,
        ruled: Vec::new(),
        ready_since: Some(time(60)),
        merging: Default::default(),
        no_check_ruled: None,
    }
}

fn pull(head: &str) -> PullFacts {
    PullFacts {
        opened_at: time(20),
        merged_at: None,
        head: Some(CommitId::from(head)),
        changed_file: Some(crate::domain::RepoPath::from("src/lib.rs")),
        checks: Vec::new(),
        reviews: Vec::new(),
    }
}

#[test]
fn each_place_is_written_with_its_ticket_in_ticket_order() {
    let running = TicketFacts {
        runs: vec![RunRecord {
            action: ActionName::from("implement"),
            head: None,
            head_end: HeadEnd::Unknown,
            started_at: time(70),
            alive: true,
            exit: None,
            questions: BTreeSet::new(),
            last_line: None,
        }],
        ..facts(7)
    };
    let waiting = TicketFacts {
        epic: Some(EpicFacts {
            id: TicketId::from(2),
            signed: false,
            challenged: false,
        }),
        open_questions: [TicketId::from(30)].into(),
        pull: Some(pull("h1")),
        ..facts(8)
    };
    let lane = facts(9);
    let done = TicketFacts {
        pull: Some(PullFacts {
            merged_at: Some(time(90)),
            ..pull("h2")
        }),
        ..facts(10)
    };
    let gathered = Gathered {
        tickets: vec![running, waiting, lane, done],
        unreadable: vec![Unreadable {
            ticket: TicketId::from(11),
            reason: "pull request #12: not found".to_owned(),
        }],
        ..Gathered::default()
    };
    assert_eq!(
        to_json(&gathered, time(100)),
        json!({
            "tickets": [
                {
                    "ticket": 7,
                    "place": "running",
                    "phase": "implement",
                    "head": null,
                    "started_at": "1970-01-01T00:01:10Z",
                },
                {
                    "ticket": 8,
                    "place": "waiting",
                    "on": [
                        {"epic": 2},
                        {"question": 30},
                        {"check": {"ticket": 8, "head": "h1"}},
                    ],
                },
                {
                    "ticket": 9,
                    "place": "lane",
                    "next": "implement",
                    "since": "1970-01-01T00:01:00Z",
                    "weight": 40,
                },
                {"ticket": 10, "place": "done"},
            ],
            "unreadable": [{"ticket": 11, "reason": "pull request #12: not found"}],
        })
    );
}

#[test]
fn nothing_gathered_is_no_ticket() {
    assert_eq!(
        to_json(&Gathered::default(), time(100)),
        json!({ "tickets": [], "unreadable": [] })
    );
}

#[test]
fn a_ticket_in_the_lane_carries_its_weight_and_no_other_does() {
    use crate::domain::schedule::{PRIORITY_AGE, Priority};
    let urgent = TicketFacts {
        priority: Priority::new(2),
        ..facts(9)
    };
    let done = TicketFacts {
        pull: Some(PullFacts {
            merged_at: Some(time(90)),
            ..pull("h2")
        }),
        ..facts(10)
    };
    let gathered = Gathered {
        tickets: vec![urgent, done],
        ..Gathered::default()
    };
    let placed = places(&gathered, time(100));
    assert_eq!(placed.len(), 2);
    assert_eq!(placed[0].ticket, TicketId::from(9));
    assert_eq!(
        placed[0].weight.map(|weight| weight.age()),
        Some(Duration::from_secs(40) + PRIORITY_AGE * 2),
        "40 s ready, and a priority of 2"
    );
    assert_eq!(placed[1].place, Place::Done);
    assert_eq!(placed[1].weight, None);
    assert_eq!(
        placed_json(&placed[1]),
        json!({"ticket": 10, "place": "done"})
    );
}

#[test]
fn the_waits_of_a_merge_are_written_with_what_each_names() {
    use crate::domain::CommitId;
    use crate::domain::schedule::{Place, WaitingOn};

    let placed = Placed {
        ticket: TicketId::from(12),
        place: Place::Waiting(
            [
                WaitingOn::Base(CommitId::from("b1")),
                WaitingOn::Merge(TicketId::from(11)),
            ]
            .into(),
        ),
        weight: None,
    };
    assert_eq!(
        placed_json(&placed),
        json!({
            "ticket": 12,
            "place": "waiting",
            "on": [{"merge": 11}, {"base": {"head": "b1"}}],
        })
    );
}
