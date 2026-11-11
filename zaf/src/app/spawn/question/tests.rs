use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{
    ActionKind, ActionName, CheckResult, ListedPhase, PhaseEnd, PhaseRun, PullFacts, RunExit,
};
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName, Ticket, TicketState};
use crate::fakes::FakeBoard;
use crate::ports::Holder;

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn listed(action: &str, kind: ActionKind, ends: PhaseEnd, on_fail: Option<&str>) -> ListedPhase {
    ListedPhase {
        action: name(action),
        kind,
        ends,
        on_fail: on_fail.map(name),
    }
}

/// implement (a model), ci (a wait, with `on_fail`), merge (zaf).
fn list(on_fail: Option<&str>) -> Vec<ListedPhase> {
    vec![
        listed("implement", ActionKind::Model, PhaseEnd::Exit, None),
        listed("ci", ActionKind::Wait, PhaseEnd::CiDone, on_fail),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged, None),
    ]
}

fn implemented() -> PhaseRun {
    PhaseRun {
        action: name("implement"),
        head: None,
        started_at: time(2),
        alive: false,
        exit: Some(RunExit {
            code: 0,
            at: time(9),
        }),
    }
}

fn pull(checks: Vec<CheckResult>, failed_checks: &[&str]) -> PullFacts {
    PullFacts {
        opened_at: time(10),
        merged_at: None,
        head: Some("abc123".to_owned()),
        checks,
        failed_checks: failed_checks
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        reviews: Vec::new(),
    }
}

/// A task whose `build` check failed at its head, its ci wait declaring `on_fail`.
fn failed_ci(task: u64, on_fail: Option<&str>) -> TickTask {
    TickTask {
        task: TicketId::from(task),
        list: list(on_fail),
        facts: StandingFacts {
            ready_since: time(1),
            pull: Some(pull(
                vec![CheckResult::Passed(time(20)), CheckResult::Failed(time(21))],
                &["build"],
            )),
            runs: vec![implemented()],
        },
    }
}

/// A task whose ci run at its head exited 3, no check having failed.
fn ci_exited(task: u64) -> TickTask {
    let mut ran = failed_ci(task, None);
    let facts = &mut ran.facts;
    facts.pull = Some(pull(vec![CheckResult::Pending], &[]));
    facts.runs.push(PhaseRun {
        action: name("ci"),
        head: Some("abc123".to_owned()),
        started_at: time(11),
        alive: false,
        exit: Some(RunExit {
            code: 3,
            at: time(15),
        }),
    });
    ran
}

fn board_with(tasks: &[u64]) -> FakeBoard {
    let board = FakeBoard::default();
    for task in tasks {
        board.add_ticket(Ticket {
            id: TicketId::from(*task),
            epic: Some(TicketId::from(421)),
            title: format!("Task {task}").into(),
            body: "".into(),
            state: Some(TicketState::Ready),
            url: "".into(),
            kind: None,
        });
    }
    board
}

fn asked(board: &FakeBoard) -> Vec<(Option<TicketId>, String)> {
    board
        .questions()
        .expect("lists")
        .iter()
        .map(|ticket| {
            (
                NewQuestion::raised_on(ticket.body.as_str()),
                NewQuestion::text_of(ticket).to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_failed_wait_without_on_fail_asks() {
    let board = board_with(&[441]);
    let tasks = [failed_ci(441, None)];
    let outcome = ask_outcome(&board, &tasks).expect("asks");
    assert_eq!(outcome.questions, vec![TicketId::from(441)]);
    assert_eq!(
        asked(&board),
        vec![(
            Some(TicketId::from(441)),
            "ci failed at abc123: failed checks build; it declares no on-fail.".to_owned()
        )],
        "one question naming ci, the head and the failed check"
    );
    assert_eq!(
        board.task(TicketId::from(441)).expect("task").state,
        Some(TicketState::Question),
        "the question makes the task wait"
    );
    assert_eq!(
        outcome.render_text(),
        "question on #441\n",
        "the outcome lists the question with its task"
    );
}

#[test]
fn a_failed_run_names_its_exit_code() {
    let board = board_with(&[441]);
    ask_failed_waits(&board, &[ci_exited(441)]).expect("asks");
    assert_eq!(
        asked(&board)[0].1,
        "ci failed at abc123: exit code 3; it declares no on-fail."
    );
}

#[test]
fn an_open_question_is_not_asked_twice() {
    let board = board_with(&[441]);
    let tasks = [failed_ci(441, None)];
    assert_eq!(ask_failed_waits(&board, &tasks).expect("asks").len(), 1);
    let second = ask_failed_waits(&board, &tasks).expect("asks");
    assert!(second.is_empty(), "the second tick raises none");
    assert_eq!(asked(&board).len(), 1, "the board holds the one question");
}

#[test]
fn another_open_question_on_the_task_is_not_asked_over() {
    let board = board_with(&[441]);
    crate::app::board::question(
        &board,
        &NewQuestion {
            on: TicketId::from(441),
            text: "Which retry window?".to_owned(),
            door: false,
        },
    )
    .expect("asks");
    let raised = ask_failed_waits(&board, &[failed_ci(441, None)]).expect("asks");
    assert!(raised.is_empty(), "a task with any open question gets none");
    assert_eq!(asked(&board).len(), 1);
}

#[test]
fn a_wait_with_on_fail_does_not_ask() {
    let board = board_with(&[441]);
    let tasks = [failed_ci(441, Some("fix"))];
    let outcome = ask_outcome(&board, &tasks).expect("asks");
    assert!(outcome.questions.is_empty());
    assert!(outcome.is_idle());
    assert!(asked(&board).is_empty(), "#442's case: no question raised");
    assert_eq!(
        board.task(TicketId::from(441)).expect("task").state,
        Some(TicketState::Ready)
    );
}

#[test]
fn only_a_failed_wait_asks() {
    let board = board_with(&[441, 442]);
    let mut green = failed_ci(442, None);
    green.facts.pull = Some(pull(vec![CheckResult::Passed(time(20))], &[]));
    let raised = ask_failed_waits(&board, &[green, failed_ci(441, None)]).expect("asks");
    assert_eq!(raised, vec![TicketId::from(441)]);
}

fn planned(number: u32, epic: u64, _tasks: &[u64]) -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(epic),
            number: InstanceNumber::from(number),
        },
    }
}

#[test]
fn a_question_costs_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let holder = Holder {
        pid: 100,
        start: "1000".to_owned(),
    };
    runs::reserve(
        dir.path(),
        &planned(1, 421, &[440]),
        &holder,
        "2026-11-09T08:00:00Z",
    )
    .expect("reserves");
    let alive = |_: &Holder| true;
    let (before, _) = crate::app::spawn::derive(dir.path(), &alive).expect("derives");
    let folders_before = runs::read_all(dir.path()).expect("reads").len();

    let board = board_with(&[441, 442]);
    let outcome = ask_outcome(&board, &[failed_ci(441, None), ci_exited(442)]).expect("asks");

    let (after, _) = crate::app::spawn::derive(dir.path(), &alive).expect("derives");
    assert_eq!(outcome.questions.len(), 2, "both asked");
    assert_eq!(before.running(), 1);
    assert_eq!(after.running(), before.running(), "no slot taken or freed");
    assert_eq!(after.launches, before.launches);
    assert_eq!(
        runs::read_all(dir.path()).expect("reads").len(),
        folders_before,
        "no run folder opened"
    );
    assert_eq!(outcome.units(), 0, "no units charged");
    assert!(outcome.started.is_none(), "nothing started");
    assert!(outcome.waits.is_empty() && outcome.ran.is_empty());
}
