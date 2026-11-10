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

    let found = read(&board, &ready).expect("reads");

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
