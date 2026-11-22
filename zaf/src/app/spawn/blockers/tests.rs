use std::time::{Duration, SystemTime};

use super::*;
use crate::domain::TicketState;
use crate::fakes::FakeBoard;

fn number(value: u64) -> TicketId {
    TicketId::from(value)
}

fn numbers(values: &[u64]) -> Vec<TicketId> {
    values.iter().copied().map(number).collect()
}

fn graph(edges: &[(u64, &[u64])]) -> Graph {
    edges
        .iter()
        .map(|(task, named)| (number(*task), numbers(named)))
        .collect()
}

fn ticket(value: u64, body: &str, label: TicketState) -> Ticket {
    Ticket {
        id: number(value),
        epic: Some(number(2)),
        title: format!("Task {value}").into(),
        body: body.into(),
        state: Some(label),
        url: "".into(),
        kind: None,
    }
}

#[test]
fn a_body_names_the_issues_on_its_blocked_by_line() {
    assert_eq!(
        blockers("Goal: x\nBlocked by: #24 #25, 26"),
        numbers(&[24, 25, 26])
    );
    assert!(blockers("Blocked by: —").is_empty());
    assert!(blockers("Blocked by: none").is_empty());
    assert!(blockers("Goal: no blockers here").is_empty());
}

#[test]
fn two_tasks_blocking_each_other_are_one_cycle_from_the_lowest() {
    let found = cycles(&graph(&[(8, &[5]), (5, &[8])]));
    assert_eq!(found, [numbers(&[5, 8])]);
    assert_eq!(render_cycle(&found[0]), "#5 → #8 → #5");
}

#[test]
fn a_three_task_cycle_starts_at_its_lowest_number_whichever_task_is_met_first() {
    let found = cycles(&graph(&[(9, &[3]), (3, &[7]), (7, &[9])]));
    assert_eq!(found, [numbers(&[3, 7, 9])]);
    assert_eq!(render_cycle(&found[0]), "#3 → #7 → #9 → #3");
}

#[test]
fn a_task_that_names_itself_is_a_cycle_of_one() {
    let found = cycles(&graph(&[(4, &[4])]));
    assert_eq!(found, [numbers(&[4])]);
    assert_eq!(render_cycle(&found[0]), "#4 → #4");
}

#[test]
fn separate_cycles_are_each_named_in_order() {
    let found = cycles(&graph(&[
        (1, &[2]),
        (2, &[1]),
        (3, &[4]),
        (4, &[5]),
        (5, &[3]),
    ]));
    assert_eq!(found, [numbers(&[1, 2]), numbers(&[3, 4, 5])]);
}

#[test]
fn a_chain_and_a_task_leading_into_a_cycle_are_not_on_it() {
    let found = cycles(&graph(&[
        (1, &[2]),
        (2, &[3]),
        (3, &[]),
        (6, &[7]),
        (7, &[8]),
        (8, &[7]),
    ]));
    assert_eq!(found, [numbers(&[7, 8])]);
}

#[test]
fn a_blocker_that_is_not_open_cannot_close_a_cycle() {
    assert!(cycles(&graph(&[(1, &[2]), (2, &[9])])).is_empty());
}

#[test]
fn every_task_on_a_cycle_is_on_a_returned_cycle() {
    // #1 and #2 share #3: both cycles run through it.
    let found = cycles(&graph(&[(1, &[3]), (3, &[1, 2]), (2, &[3])]));
    let on: BTreeSet<TicketId> = found.iter().flatten().copied().collect();
    assert_eq!(on, numbers(&[1, 2, 3]).into_iter().collect());
}

#[test]
fn read_walks_open_blockers_and_classes_the_rest() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(3, "Blocked by: #4 #5 #6 #8 #99", TicketState::Ready));
    board.add_ticket(ticket(4, "Blocked by: #3", TicketState::Blocked));
    board.add_ticket(ticket(5, "", TicketState::Done));
    board.add_ticket(ticket(6, "", TicketState::Blocked));
    board.close_not_planned(number(6));
    board.add_ticket(ticket(8, "", TicketState::Blocked));
    board.close(number(8));
    let ready = board.ready().expect("ready");

    let found = read(&board, &ready, &[]).expect("reads");

    assert_eq!(
        found.graph.keys().copied().collect::<Vec<_>>(),
        numbers(&[3, 4])
    );
    assert_eq!(
        found.closed,
        BTreeSet::from([number(5), number(8)]),
        "closed as completed is done, label or not"
    );
    assert_eq!(
        found.dead,
        BTreeMap::from([
            (number(6), DeadReason::ClosedNotPlanned),
            (number(99), DeadReason::NotAnIssue),
        ])
    );
}

/// Epic #20 and its tasks, each `(number, label, closed as not planned, closed at)`, with ready
/// #3 blocked by the epic; the epic as the board lists it.
fn epic_blocking_three(tasks: &[(u64, TicketState, bool, Option<u64>)]) -> (FakeBoard, Vec<Epic>) {
    let board = FakeBoard::default();
    board.add_challenged_epic(number(20), "Epic");
    board.add_ticket(ticket(3, "Blocked by: #20", TicketState::Ready));
    for &(task, label, dropped, closed_at) in tasks {
        board.add_ticket(Ticket {
            epic: Some(number(20)),
            ..ticket(task, "", label)
        });
        if dropped {
            board.close_not_planned(number(task));
        } else if let Some(secs) = closed_at {
            board.close_at(
                number(task),
                SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            );
        }
    }
    let epics = board.epics().expect("epics");
    (board, epics)
}

#[test]
fn lane_takes_ticket_blocked_by_finished_epic() {
    let (board, epics) = epic_blocking_three(&[
        (21, TicketState::Done, false, Some(100)),
        (22, TicketState::Done, false, Some(300)),
    ]);
    let ready = board.ready().expect("ready");

    let found = read(&board, &ready, &epics).expect("reads");

    assert_eq!(found.closed, BTreeSet::from([number(20)]));
    assert!(found.dead.is_empty(), "{:?}", found.dead);
    assert_eq!(
        found.graph.keys().copied().collect::<Vec<_>>(),
        numbers(&[3]),
        "the epic is not an open task the ticket waits on"
    );
    // Nothing is written: the epic stays open and gets no label.
    assert_eq!(board.closed_as(number(20)), None);
    assert_eq!(board.task(number(20)).expect("reads").state, None);
    assert_eq!(board.epics().expect("epics").len(), 1);
}

#[test]
fn the_finished_epic_stands_done_at_its_last_close() {
    let (_, epics) = epic_blocking_three(&[
        (21, TicketState::Done, false, Some(100)),
        (22, TicketState::Done, false, Some(300)),
        (23, TicketState::Ready, true, None),
    ]);

    let at = finished_epic_standing(number(20), Standing::Open.into(), &epics);

    assert_eq!(at.standing, Standing::Done);
    assert_eq!(
        at.closed_at,
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(300))
    );
    // Another number, and a standing that is not open, are read as they are.
    let other = finished_epic_standing(number(21), Standing::Open.into(), &epics);
    assert_eq!(other.standing, Standing::Open);
    let dropped = finished_epic_standing(number(20), Standing::ClosedNotPlanned.into(), &epics);
    assert_eq!(dropped.standing, Standing::ClosedNotPlanned);
}

#[test]
fn all_dropped_epic_keeps_blocker_waiting() {
    let (board, epics) = epic_blocking_three(&[
        (21, TicketState::Claimed, true, None),
        (22, TicketState::Claimed, true, None),
    ]);
    let ready = board.ready().expect("ready");

    let found = read(&board, &ready, &epics).expect("reads");

    assert!(found.closed.is_empty(), "{:?}", found.closed);
    assert_eq!(
        found.graph.keys().copied().collect::<Vec<_>>(),
        numbers(&[3, 20]),
        "the epic is still an open blocker"
    );
}

#[test]
fn an_epic_with_a_task_open_keeps_blocker_waiting() {
    let (board, epics) = epic_blocking_three(&[
        (21, TicketState::Done, false, Some(100)),
        (22, TicketState::Ready, false, None),
    ]);
    let ready = board.ready().expect("ready");

    let found = read(&board, &ready, &epics).expect("reads");

    assert!(found.closed.is_empty(), "{:?}", found.closed);
}

#[test]
fn hand_labelled_done_epic_still_clears() {
    // Labelled `done` by hand with a task still open: the board's own read says done.
    let (board, epics) = epic_blocking_three(&[(21, TicketState::Ready, false, None)]);
    board
        .set_state(number(20), TicketState::Done)
        .expect("labelled");
    let ready = board.ready().expect("ready");

    let found = read(&board, &ready, &epics).expect("reads");

    assert_eq!(found.closed, BTreeSet::from([number(20)]));
}

#[test]
fn dead_blockers_are_listed_by_task_then_blocker_with_their_reason() {
    let dead_map = BTreeMap::from([
        (number(9), DeadReason::NotAnIssue),
        (number(8), DeadReason::ClosedNotPlanned),
    ]);
    let found = dead(&graph(&[(2, &[9, 8, 8]), (1, &[9, 3])]), &dead_map);
    let lines: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "dead blocker #9 on #1: not an issue",
            "dead blocker #8 on #2: closed as not planned",
            "dead blocker #9 on #2: not an issue",
        ]
    );
}
