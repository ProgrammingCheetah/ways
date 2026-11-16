use super::super::tests::*;
use super::*;
use crate::domain::schedule::Run;

#[test]
fn running_is_claimed_and_done_is_done() {
    let running = Place::Running(Run {
        phase: name("implement"),
        head: None,
        started_at: time(5),
    });
    for claimed in [false, true] {
        assert_eq!(label_for(&running, claimed), TicketState::Claimed);
        assert_eq!(label_for(&Place::Done, claimed), TicketState::Done);
    }
}

#[test]
fn a_question_wait_is_question_claimed_or_not() {
    let questioned = waiting(&[WaitingOn::Question(id(30)), WaitingOn::Ticket(id(25))]);
    for claimed in [false, true] {
        assert_eq!(label_for(&questioned, claimed), TicketState::Question);
    }
}

#[test]
fn a_ticket_wait_is_blocked_only_while_unclaimed() {
    let blocked = waiting(&[WaitingOn::Ticket(id(25))]);
    assert_eq!(label_for(&blocked, false), TicketState::Blocked);
    assert_eq!(label_for(&blocked, true), TicketState::Claimed);
}

#[test]
fn the_lane_and_other_waits_leave_the_claim_its_own() {
    for place in [
        in_lane("implement", 1),
        waiting(&[WaitingOn::Epic(id(1))]),
        waiting(&[on_check("h1")]),
        waiting(&[WaitingOn::Merge(id(12))]),
        waiting(&[WaitingOn::Base(commit("b1"))]),
    ] {
        assert_eq!(label_for(&place, false), TicketState::Ready, "{place:?}");
        assert_eq!(label_for(&place, true), TicketState::Claimed, "{place:?}");
    }
}
