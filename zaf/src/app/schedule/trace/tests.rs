use std::time::{Duration, SystemTime};

use super::*;
use crate::app::schedule::labels::Unblocked;
use crate::app::spawn::outcome::Launched;
use crate::app::spawn::{PlannedLaunch, Ran, Released, Started, TickOutcome};
use crate::domain::schedule::{ActionName, Run, Weight};
use crate::domain::{CommitId, Instance, InstanceNumber, RoleName};

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

const DAY: u64 = 24 * 60 * 60;

fn running(phase: &str) -> Place {
    Place::Running(Run {
        phase: ActionName::from(phase),
        head: None,
        started_at: time(10),
    })
}

fn lane(next: &str) -> Place {
    Place::InLane {
        next: ActionName::from(next),
        since: time(20),
    }
}

fn waiting(on: &[WaitingOn]) -> Place {
    Place::Waiting(on.iter().cloned().collect())
}

fn placed(ticket: u64, place: Place) -> Placed {
    let weight = matches!(place, Place::InLane { .. }).then(Weight::default);
    Placed {
        ticket: id(ticket),
        place,
        weight,
    }
}

fn short(text: &str) -> ShortPlace {
    ShortPlace::from(text)
}

fn places(entries: &[(u64, &str)]) -> Places {
    entries
        .iter()
        .map(|(ticket, place)| (id(*ticket), short(place)))
        .collect()
}

#[test]
fn each_place_has_its_short_form() {
    assert_eq!(
        ShortPlace::of(&running("implement")).as_str(),
        "running(implement)"
    );
    assert_eq!(
        ShortPlace::of(&lane("implement")).as_str(),
        "lane(implement)"
    );
    assert_eq!(ShortPlace::of(&Place::Done).as_str(), "done");
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Question(id(45))])).as_str(),
        "waiting(question #45)"
    );
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Ticket(id(5))])).as_str(),
        "waiting(ticket #5)"
    );
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Epic(id(1))])).as_str(),
        "waiting(epic #1)"
    );
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Check(
            id(7),
            CommitId::from("a1b2c3d4e5f60718")
        )]))
        .as_str(),
        "waiting(check a1b2c3d)",
        "a check is named by the first seven characters of its head"
    );
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Merge(id(12))])).as_str(),
        "waiting(merge of #12)"
    );
    assert_eq!(
        ShortPlace::of(&waiting(&[WaitingOn::Base(CommitId::from(
            "a1b2c3d4e5f60718"
        ))]))
        .as_str(),
        "waiting(base a1b2c3d)",
        "the integration branch is named by the first seven characters of its head"
    );
    assert_eq!(ShortPlace::unreadable().as_str(), "unreadable");
}

#[test]
fn a_ticket_waiting_on_several_things_names_each_in_the_cores_order() {
    let place = waiting(&[
        WaitingOn::Base(CommitId::from("def")),
        WaitingOn::Merge(id(12)),
        WaitingOn::Check(id(7), CommitId::from("abc")),
        WaitingOn::Ticket(id(5)),
        WaitingOn::Question(id(45)),
        WaitingOn::Epic(id(1)),
    ]);
    assert_eq!(
        ShortPlace::of(&place).as_str(),
        "waiting(epic #1, question #45, ticket #5, check abc, merge of #12, base def)"
    );
}

#[test]
fn a_ticket_that_changed_place_is_a_move_with_from_and_to() {
    let moved = moves(
        &places(&[(7, "lane(implement)")]),
        &places(&[(7, "running(implement)")]),
    );
    assert_eq!(
        moved,
        [Move {
            ticket: id(7),
            from: Some(short("lane(implement)")),
            to: Some(short("running(implement)")),
        }]
    );
    assert_eq!(moved[0].line(), "#7 lane(implement) -> running(implement)");
}

#[test]
fn a_ticket_that_stayed_is_no_move() {
    let before = places(&[(7, "running(implement)"), (8, "waiting(ticket #7)")]);
    assert_eq!(moves(&before, &before), []);
}

#[test]
fn a_new_ticket_and_a_vanished_one_are_moves() {
    let moved = moves(
        &places(&[(5, "done"), (7, "lane(merge)")]),
        &places(&[(7, "lane(merge)"), (9, "lane(brief)")]),
    );
    assert_eq!(
        moved,
        [
            Move {
                ticket: id(5),
                from: Some(short("done")),
                to: None,
            },
            Move {
                ticket: id(9),
                from: None,
                to: Some(short("lane(brief)")),
            },
        ]
    );
    let lines: Vec<String> = moved.iter().map(Move::line).collect();
    assert_eq!(lines, ["#5 done -> gone", "#9 new -> lane(brief)"]);
}

#[test]
fn the_count_line_counts_each_place_and_what_could_not_be_read() {
    let seen = Seen {
        placed: vec![
            placed(1, running("implement")),
            placed(2, lane("review")),
            placed(3, waiting(&[WaitingOn::Ticket(id(1))])),
            placed(4, Place::Done),
            placed(5, Place::Done),
        ],
        unreadable: Vec::new(),
        done: 0,
    };
    assert_eq!(
        seen.count_line().as_deref(),
        Some("5 tickets: 1 running, 1 in the lane, 1 waiting, 2 done")
    );
    let one = Seen {
        placed: vec![placed(1, lane("brief"))],
        unreadable: vec![Unreadable {
            ticket: id(2),
            reason: "pull request #3: not found".to_owned(),
        }],
        done: 0,
    };
    assert_eq!(
        one.count_line().as_deref(),
        Some("2 tickets: 0 running, 1 in the lane, 0 waiting, 0 done, 1 not read")
    );
    let single = Seen {
        placed: vec![placed(1, lane("brief"))],
        unreadable: Vec::new(),
        done: 0,
    };
    assert_eq!(
        single.count_line().as_deref(),
        Some("1 ticket: 0 running, 1 in the lane, 0 waiting, 0 done")
    );
    assert_eq!(Seen::default().count_line(), None, "no ticket, no line");
}

#[test]
fn the_block_prints_each_move_then_the_count() {
    let seen = Seen {
        placed: vec![
            placed(7, running("implement")),
            placed(8, waiting(&[WaitingOn::Ticket(id(7))])),
        ],
        unreadable: Vec::new(),
        done: 0,
    };
    let moved = moves(
        &places(&[(7, "lane(implement)"), (8, "waiting(ticket #7)")]),
        &seen.places(),
    );
    assert_eq!(
        place_lines(&seen, &moved, false),
        [
            "#7 lane(implement) -> running(implement)",
            "2 tickets: 1 running, 0 in the lane, 1 waiting, 0 done",
        ]
    );
    assert_eq!(
        place_lines(&Seen::default(), &[], false),
        Vec::<String>::new(),
        "nothing seen and nothing moved says nothing"
    );
}

#[test]
fn more_prints_every_ticket_once_a_moved_one_as_its_move() {
    let seen = Seen {
        placed: vec![
            placed(7, running("implement")),
            placed(8, waiting(&[WaitingOn::Ticket(id(7))])),
        ],
        unreadable: vec![Unreadable {
            ticket: id(9),
            reason: "its blocker #2: down".to_owned(),
        }],
        done: 0,
    };
    let moved = moves(
        &places(&[
            (5, "done"),
            (7, "lane(implement)"),
            (8, "waiting(ticket #7)"),
        ]),
        &seen.places(),
    );
    assert_eq!(
        place_lines(&seen, &moved, true),
        [
            "#5 done -> gone",
            "#7 lane(implement) -> running(implement)",
            "#8 waiting(ticket #7)",
            "#9 new -> unreadable",
            "3 tickets: 1 running, 0 in the lane, 1 waiting, 0 done, 1 not read",
        ]
    );
}

fn instance(role: &str, epic: u64, number: u32) -> Instance {
    Instance {
        role: RoleName::from(role),
        epic: id(epic),
        number: InstanceNumber::from(number),
    }
}

fn busy_did() -> Did {
    let outcome = TickOutcome {
        started: Some(Started {
            task: id(9),
            action: ActionName::from("review"),
            units: 2,
        }),
        waits: vec![Launched {
            task: id(4),
            action: ActionName::from("ci"),
        }],
        ran: vec![Ran {
            task: id(3),
            action: ActionName::from("merge"),
            exit: 1,
        }],
        questions: vec![id(5)],
        asked: Vec::new(),
    };
    Did::of(
        &outcome,
        &[Unblocked {
            task: id(26),
            blockers: vec![id(25)],
        }],
        &[PlannedLaunch {
            instance: instance("planning", 1, 3),
        }],
        &[Released {
            task: id(4),
            instance: instance("team", 1, 2),
        }],
        &["challenger instance 1 on #2".to_owned()],
    )
}

#[test]
fn what_a_tick_did_is_worded_as_its_block_words_it() {
    assert_eq!(
        busy_did().lines(),
        [
            "lost challenger instance 1 on #2",
            "released #4 from team instance 2 on #1",
            "#26 unblocked: #25 is done",
            "ran merge #3: exit 1",
            "started review #9: 2 units",
            "wait ci #4",
            "question on #5",
            "launching planning instance 3 on #1",
        ]
    );
    assert_eq!(Did::default().lines(), Vec::<String>::new());
}

#[test]
fn what_a_tick_did_is_kept_as_json_and_reads_back() {
    let did = busy_did();
    let json = serde_json::to_value(&did).expect("serializes");
    assert_eq!(
        json,
        json!({
            "started": {"task": 9, "action": "review", "units": 2},
            "waits": [{"task": 4, "action": "ci"}],
            "ran": [{"task": 3, "action": "merge", "exit": 1}],
            "questions": [5],
            "unblocked": [{"task": 26, "blockers": [25]}],
            "launched": ["planning instance 3 on #1"],
            "released": [{"task": 4, "instance": "team instance 2 on #1"}],
            "lost": ["challenger instance 1 on #2"],
        })
    );
    assert_eq!(
        serde_json::from_value::<Did>(json).expect("reads back"),
        did
    );
}

#[test]
fn a_question_is_kept_with_its_cause_and_an_old_line_still_reads() {
    let outcome = TickOutcome {
        questions: vec![id(7), id(5)],
        asked: vec![crate::app::spawn::outcome::Asked {
            task: id(7),
            question: id(61),
            cause: "merge got nowhere three times at d962227".to_owned(),
        }],
        ..TickOutcome::default()
    };
    let did = Did::of(&outcome, &[], &[], &[], &[]);
    let json = serde_json::to_value(&did).expect("serializes");
    assert_eq!(
        json["questions"],
        json!([
            {"task": 7, "question": 61, "cause": "merge got nowhere three times at d962227"},
            5,
        ]),
        "a question with no cause named keeps the issue alone"
    );
    let read = serde_json::from_value::<Did>(json).expect("reads back");
    assert_eq!(read, did);
    assert_eq!(
        read.lines(),
        [
            "question on #7",
            "question on #5",
            "asked #61 on #7: merge got nowhere three times at d962227",
        ]
    );
    // A line written before causes were kept reads as it did.
    let old: Did = serde_json::from_value(json!({"questions": [5, 9]})).expect("reads");
    assert_eq!(old.lines(), ["question on #5", "question on #9"]);
}

fn entry(seen: Result<Seen, String>) -> Entry {
    Entry {
        at: time(1_794_503_700),
        seen,
        did: Did::default(),
        skipped: vec!["#2 skipped: the challenger runs on it".to_owned()],
    }
}

#[test]
fn a_line_carries_where_every_ticket_sits_and_what_moved() {
    let seen = Seen {
        placed: vec![placed(7, running("implement")), placed(9, lane("review"))],
        unreadable: vec![Unreadable {
            ticket: id(11),
            reason: "pull request #12: not found".to_owned(),
        }],
        done: 0,
    };
    let line = entry(Ok(seen)).line(Some(&places(&[(7, "lane(implement)")])));
    assert_eq!(
        line.json,
        json!({
            "at": "2026-11-12T17:15:00Z",
            "zaf": ZAF_VERSION,
            "tickets": [
                {"ticket": 7, "place": "running", "phase": "implement", "head": null,
                 "started_at": "1970-01-01T00:00:10Z", "short": "running(implement)"},
                {"ticket": 9, "place": "lane", "next": "review", "since": "1970-01-01T00:00:20Z",
                 "weight": 0, "short": "lane(review)"},
            ],
            "unreadable": [{"ticket": 11, "reason": "pull request #12: not found"}],
            "did": serde_json::to_value(Did::default()).expect("did"),
            "skipped": ["#2 skipped: the challenger runs on it"],
            "moved": [
                {"ticket": 7, "from": "lane(implement)", "to": "running(implement)"},
                {"ticket": 9, "from": null, "to": "lane(review)"},
                {"ticket": 11, "from": null, "to": "unreadable"},
            ],
        })
    );
    assert_eq!(line.moved.len(), 3);
    // The old scheduler is gone, and so is its word on each ticket.
    assert!(line.json.get("old").is_none(), "{}", line.json);
    // Its keys are written in the module's order, so a person reads the time first.
    assert!(
        line.text()
            .starts_with(r#"{"at":"2026-11-12T17:15:00Z","zaf":"#)
    );
}

#[test]
fn a_line_that_could_not_place_has_an_error_no_tickets_and_no_move() {
    let line = entry(Err("the board is down".to_owned())).line(Some(&places(&[(7, "done")])));
    assert_eq!(line.json["error"], "the board is down");
    assert!(line.json.get("tickets").is_none(), "{}", line.json);
    assert!(line.json.get("unreadable").is_none(), "{}", line.json);
    assert_eq!(line.json["moved"], json!([]));
    assert_eq!(line.moved, []);
    let stopped = Entry::stopped(time(0), "the pre-tick hook failed").line(None);
    assert_eq!(stopped.json["error"], "the pre-tick hook failed");
    assert!(stopped.json.get("old").is_none(), "{}", stopped.json);
}

#[test]
fn with_the_last_line_unknown_nothing_is_a_move() {
    let seen = Seen {
        placed: vec![placed(7, lane("implement"))],
        unreadable: Vec::new(),
        done: 0,
    };
    assert_eq!(entry(Ok(seen.clone())).line(None).moved, []);
    assert_eq!(
        entry(Ok(seen)).line(Some(&Places::new())).moved.len(),
        1,
        "the first line of all: every ticket is new"
    );
}

#[test]
fn a_line_reads_back_with_its_places_moves_and_what_the_tick_did() {
    let seen = Seen {
        placed: vec![placed(7, running("implement"))],
        unreadable: vec![Unreadable {
            ticket: id(11),
            reason: "down".to_owned(),
        }],
        done: 0,
    };
    let mut written = entry(Ok(seen));
    written.did = busy_did();
    let line = written.line(Some(&Places::new()));
    let read = Recorded::parse(line.text()).expect("reads back");
    assert_eq!(read.at, written.at);
    assert_eq!(
        read.places,
        Some(places(&[(7, "running(implement)"), (11, "unreadable")]))
    );
    assert_eq!(read.moved, line.moved);
    assert_eq!(read.did, busy_did());

    let stopped = Entry::stopped(time(5), "down").line(None);
    let read = Recorded::parse(stopped.text()).expect("reads back");
    assert_eq!(read.places, None, "a line with an error placed nothing");

    assert_eq!(
        Recorded::parse(r#"{"at":"2026-11-12T17:15:00Z","tick"#),
        None
    );
    assert_eq!(Recorded::parse(r#"{"tickets":[]}"#), None, "no time");
}

#[test]
fn a_line_is_kept_fourteen_days() {
    let now = time(100 * DAY);
    assert_eq!(KEEP_FOR, Duration::from_secs(14 * DAY));
    assert!(kept(time(100 * DAY - 14 * DAY + 1), now));
    assert!(
        !kept(time(100 * DAY - 14 * DAY), now),
        "fourteen days old is cut"
    );
    assert!(!kept(time(DAY), now));
    assert!(
        kept(time(101 * DAY), now),
        "a clock that stepped back keeps it"
    );
}

/// A trace line at `at` that saw `seen`.
fn text_at(at: SystemTime, seen: Result<Seen, String>) -> String {
    Entry { at, ..entry(seen) }.line(None).text().to_owned()
}

#[test]
fn the_file_before_a_write_keeps_the_young_lines_and_the_last_places() {
    let now = time(100 * DAY);
    let old = text_at(time(80 * DAY), Ok(Seen::default()));
    let placed_line = text_at(
        time(99 * DAY),
        Ok(Seen {
            placed: vec![placed(7, lane("implement"))],
            unreadable: Vec::new(),
            done: 0,
        }),
    );
    let error_line = text_at(time(99 * DAY + 60), Err("down".to_owned()));
    let text = format!("{old}\n{placed_line}\n{error_line}\n");
    let before = Before::of(&text, now);
    assert_eq!(before.kept, [placed_line.as_str(), error_line.as_str()]);
    assert!(before.dropped);
    assert_eq!(
        before.previous,
        places(&[(7, "lane(implement)")]),
        "the last line with tickets, past the one with an error"
    );

    let young = format!("{placed_line}\n");
    let before = Before::of(&young, now);
    assert!(!before.dropped, "nothing old, nothing dropped");
    assert_eq!(before.kept.len(), 1);

    let half = format!("{placed_line}\n{{\"at\":\"2026-");
    assert!(Before::of(&half, now).dropped, "a half-written line goes");

    let empty = Before::of("", now);
    assert!(!empty.dropped);
    assert!(empty.kept.is_empty());
    assert_eq!(empty.previous, Places::new());
}

#[test]
fn the_count_line_counts_the_done_tasks_of_the_open_epics() {
    use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
    use crate::app::schedule::tick::support::{facts, gathered};
    use crate::domain::TaskSize;

    let list = [ListedPhase {
        action: ActionName::from("merge"),
        kind: ActionKind::Zaf,
        ends: PhaseEnd::Merged,
        on_fail: None,
    }];
    let mut seen_by = gathered(vec![(
        facts(7, &list, TaskSize::S, time(10), None, Vec::new()),
        list.to_vec(),
    )]);
    // The board lists no done ticket as open: the epics' task lists name #8 and #9 done. #7 is
    // among them too, and is counted where the core placed it.
    seen_by.done = [id(7), id(8), id(9)].into();

    let seen = Seen::of(&seen_by, time(100));

    assert_eq!(seen.done, 2);
    assert_eq!(
        seen.count_line().as_deref(),
        Some("3 tickets: 0 running, 1 in the lane, 0 waiting, 2 done")
    );
}
