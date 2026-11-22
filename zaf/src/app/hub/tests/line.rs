//! The colour of a task's line: the first of §2's six that holds, from the core's placement.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use super::super::*;
use crate::domain::schedule::{HeadEnd, Phase, Place, Run, RunRecord, TicketFacts, WaitingOn};
use crate::domain::{ActionName, EpicTask, PhaseEnd, Standing, TaskSize, TicketId, TicketState};

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

fn task(standing: Standing, state: Option<TicketState>) -> EpicTask {
    EpicTask {
        number: id(118),
        title: "A task".into(),
        body: "".into(),
        url: "".into(),
        state,
        standing,
        group: None,
        kind: None,
        closed_at: None,
    }
}

fn open() -> EpicTask {
    task(Standing::Open, Some(TicketState::Ready))
}

fn phase(name: &str, end: PhaseEnd) -> Phase {
    Phase {
        name: ActionName::from(name),
        ends: vec![end],
        on_fail: None,
    }
}

/// A ticket with two phases and nothing else: no run, no question, no pull request.
fn facts() -> TicketFacts {
    TicketFacts {
        ticket: id(118),
        epic: None,
        open_questions: BTreeSet::new(),
        dependencies: Vec::new(),
        pull: None,
        runs: Vec::new(),
        phases: vec![
            phase("implement", PhaseEnd::Exit),
            phase("merge", PhaseEnd::Merged),
        ],
        size: TaskSize::S,
        priority: None,
        ruled: Vec::new(),
        ready_since: None,
        merging: Default::default(),
        no_check_ruled: None,
        trees: Default::default(),
    }
}

fn run_record(action: &str) -> RunRecord {
    RunRecord {
        action: ActionName::from(action),
        head: None,
        head_end: HeadEnd::Unknown,
        started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(5),
        alive: false,
        exit: None,
        questions: BTreeSet::new(),
        last_line: None,
    }
}

fn running() -> Place {
    Place::Running(Run {
        phase: ActionName::from("implement"),
        head: None,
        started_at: SystemTime::UNIX_EPOCH,
    })
}

fn lane() -> Place {
    Place::InLane {
        next: ActionName::from("implement"),
        since: SystemTime::UNIX_EPOCH,
    }
}

fn waiting(on: WaitingOn) -> Place {
    Place::Waiting(BTreeSet::from([on]))
}

fn line(task: &EpicTask, place: Option<&Place>, facts: Option<&TicketFacts>) -> LineState {
    line_state(task, place, facts, &BTreeSet::new())
}

#[test]
fn line_state_done_wins_over_question() {
    let mut asked = facts();
    asked.open_questions.insert(id(200));
    let done = task(Standing::Done, Some(TicketState::Done));
    assert_eq!(
        line(
            &done,
            Some(&waiting(WaitingOn::Question(id(200)))),
            Some(&asked)
        ),
        LineState::Done
    );
    assert_eq!(
        line(&open(), Some(&Place::Done), Some(&asked)),
        LineState::Done
    );
}

#[test]
fn line_state_question_wins_over_running() {
    let mut asked = facts();
    asked.open_questions.insert(id(200));
    assert_eq!(
        line(&open(), Some(&running()), Some(&asked)),
        LineState::Question
    );
    assert_eq!(
        line(
            &open(),
            Some(&waiting(WaitingOn::Question(id(200)))),
            Some(&facts())
        ),
        LineState::Question
    );
}

#[test]
fn line_state_an_epic_question_colours_no_task() {
    let mut asked = facts();
    asked.open_questions.insert(id(300));
    let on_epic = BTreeSet::from([id(300)]);
    assert_eq!(
        line_state(&open(), Some(&running()), Some(&asked), &on_epic),
        LineState::Working
    );
    assert_eq!(
        line_state(
            &open(),
            Some(&waiting(WaitingOn::Question(id(300)))),
            Some(&asked),
            &on_epic
        ),
        LineState::NotStarted
    );
}

#[test]
fn line_state_running_is_working() {
    assert_eq!(
        line(&open(), Some(&running()), Some(&facts())),
        LineState::Working
    );
}

#[test]
fn line_state_waiting_on_ticket_is_blocked() {
    assert_eq!(
        line(
            &open(),
            Some(&waiting(WaitingOn::Ticket(id(5)))),
            Some(&facts())
        ),
        LineState::Blocked
    );
}

#[test]
fn line_state_started_in_lane_waits_for_tick() {
    let mut began = facts();
    began.runs.push(run_record("implement"));
    assert_eq!(
        line(&open(), Some(&lane()), Some(&began)),
        LineState::WaitingTick
    );
}

#[test]
fn line_state_never_started_is_not_started() {
    assert_eq!(
        line(&open(), Some(&lane()), Some(&facts())),
        LineState::NotStarted
    );
}

#[test]
fn line_state_unplaced_reads_its_label() {
    let by = |state| line(&task(Standing::Open, state), None, None);
    assert_eq!(by(Some(TicketState::Question)), LineState::Question);
    assert_eq!(by(Some(TicketState::Claimed)), LineState::Working);
    assert_eq!(by(Some(TicketState::Blocked)), LineState::Blocked);
    assert_eq!(by(Some(TicketState::Ready)), LineState::NotStarted);
    assert_eq!(by(None), LineState::NotStarted);
    assert_eq!(
        line(&task(Standing::Done, Some(TicketState::Done)), None, None),
        LineState::Done
    );
}

#[test]
fn line_state_serialises_snake_case() {
    assert_eq!(
        serde_json::to_string(&LineState::WaitingTick).expect("json"),
        "\"waiting_tick\""
    );
    assert_eq!(
        serde_json::to_string(&LineState::NotStarted).expect("json"),
        "\"not_started\""
    );
}
