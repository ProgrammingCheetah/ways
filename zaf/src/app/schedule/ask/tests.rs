use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
use crate::app::schedule::gather::Base;
use crate::app::schedule::tick::support;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::schedule::{
    CheckFact, CheckName, CheckResult, Dependency, ExitCode, HeadEnd, Place, PullFacts, RunExit,
    WaitingOn, place,
};
use crate::domain::{
    BodyHash, Epic, EpicTask, Instance, InstanceNumber, RoleName, Signature, Standing, TaskSize,
    Ticket, TicketState,
};
use crate::fakes::FakeBoard;
use crate::ports::Holder;

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

/// Step 5 of a tick no other tick meets: [`ask`] under a lock of its own.
fn ask_alone(board: &dyn Board, causes: &[Cause]) -> Result<Vec<Asked>, AskError> {
    let state_dir = tempfile::tempdir().expect("tempdir");
    ask(board, state_dir.path(), causes)
}

/// [`ask_outcome`] of a tick no other tick meets, under a lock of its own.
fn ask_outcome_alone(board: &dyn Board, causes: &[Cause]) -> Result<TickOutcome, AskError> {
    let state_dir = tempfile::tempdir().expect("tempdir");
    ask_outcome(board, state_dir.path(), causes)
}

// What moved here from the getting-nowhere step (`failures.rs`).

fn hold(codes: &[i32], last_line: Option<&str>) -> Hold {
    Hold {
        task: TicketId::from(3),
        action: ActionName::from("fix"),
        head: Some("h1".to_owned()),
        has_pull: true,
        codes: codes.to_vec(),
        last_line: last_line.map(str::to_owned),
        last_run: "1700".to_owned(),
    }
}

#[test]
fn the_question_names_the_phase_the_codes_and_the_last_line() {
    assert_eq!(
        nowhere_text(&hold(&[1, 2, 3], Some("no such file"))),
        "fix got nowhere three times at h1 (exit codes 1, 2, 3); the last line of its log: \
         \"no such file\". It is not started again until this is ruled. Last run: 1700."
    );
}

#[test]
fn the_question_names_a_missing_head() {
    let mut no_pull = hold(&[0, 1, 0], None);
    no_pull.head = None;
    no_pull.has_pull = false;
    assert!(
        nowhere_text(&no_pull).starts_with("fix got nowhere three times with no pull request "),
        "{}",
        nowhere_text(&no_pull)
    );
    let mut no_head = no_pull;
    no_head.has_pull = true;
    assert!(
        nowhere_text(&no_head).starts_with("fix got nowhere three times at no head "),
        "{}",
        nowhere_text(&no_head)
    );
}

#[test]
fn a_run_with_no_output_says_so() {
    assert!(nowhere_text(&hold(&[1, 1, 1], None)).contains("none was recorded"));
}

#[test]
fn a_ruled_question_names_the_run_it_counted_to() {
    let text = nowhere_text(&hold(&[1, 1, 1], Some("Last run: 9. nope")));
    assert_eq!(marked_run(&text, &ActionName::from("fix")), Some("1700"));
    assert_eq!(
        marked_run(&text, &ActionName::from("other")),
        None,
        "another phase's question is not this one's"
    );
    assert_eq!(
        marked_run("Which retry window?", &ActionName::from("fix")),
        None
    );
}

#[test]
fn a_question_raised_before_getting_nowhere_counted_still_marks_its_run() {
    let old = "fix failed 3 times in a row with exit codes 1, 1, 1; the last line of its log: \
               none was recorded. It is not started again until this is ruled. Last run: 1600.";
    assert_eq!(marked_run(old, &ActionName::from("fix")), Some("1600"));
}

#[test]
fn the_limit_is_three() {
    assert_eq!(NOWHERE_LIMIT, 3);
}

#[test]
fn getting_nowhere_is_named_by_its_short_cause() {
    let mut long_head = hold(&[1, 1, 1], None);
    long_head.action = name("merge");
    long_head.head = Some("d962227c0ffee".to_owned());
    assert_eq!(
        Cause::Nowhere(long_head).line(),
        "merge got nowhere three times at d962227"
    );
}

// What moved here from the failed-wait step (`question.rs`).

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

/// D1: the list has no `pr-open` phase, so implement's `exit` is keyed on the head: its run left
/// the head the checks ran at.
fn implemented() -> RunRecord {
    RunRecord {
        action: name("implement"),
        head: None,
        head_end: HeadEnd::Left(Some(CommitId::from("abc123"))),
        started_at: time(2),
        alive: false,
        exit: Some(RunExit {
            code: ExitCode::from(0),
            at: time(9),
        }),
        questions: BTreeSet::new(),
        last_line: None,
    }
}

/// A check named `name` with `result`.
fn check(name: &str, result: CheckResult) -> CheckFact {
    CheckFact {
        name: CheckName::from(name),
        result,
    }
}

fn pull(checks: Vec<CheckFact>) -> PullFacts {
    PullFacts {
        opened_at: time(10),
        merged_at: None,
        head: Some(CommitId::from("abc123")),
        changes_a_file: true,
        checks,
        reviews: Vec::new(),
    }
}

/// A task whose `build` check failed at its head, its ci wait declaring `on_fail`.
fn failed_ci(task: u64, on_fail: Option<&str>) -> (TicketFacts, Vec<ListedPhase>) {
    let pull = pull(vec![
        check("lint", CheckResult::Passed(time(20))),
        check("build", CheckResult::Failed(time(21))),
    ]);
    let facts = support::facts(
        task,
        &list(on_fail),
        TaskSize::S,
        time(1),
        Some(pull),
        vec![implemented()],
    );
    (facts, list(on_fail))
}

/// The failed waits among `tickets`, as the tick's gate hands them to step 5.
fn failed_waits(tickets: Vec<(TicketFacts, Vec<ListedPhase>)>) -> Vec<FailedWait> {
    support::sorted(&support::gathered(tickets), time(100)).failed_waits
}

/// The causes of `failed`.
fn causes(failed: &[FailedWait]) -> Vec<Cause> {
    failed.iter().cloned().map(Cause::FailedWait).collect()
}

/// Step 5 on the failed waits `failed`: the issues it raised a question on.
fn ask_failed_waits(board: &FakeBoard, failed: &[FailedWait]) -> Vec<TicketId> {
    ask_alone(board, &causes(failed))
        .expect("asks")
        .iter()
        .map(|asked| asked.task)
        .collect()
}

/// A task whose ci run at its head exited 3, no check having failed.
///
/// D2: with a `ci-done` phase in the list, a check not ended at the head makes the task wait,
/// whatever a run exited, so this ci wait ends at its own `exit` and fails on its code.
fn ci_exited(task: u64) -> (TicketFacts, Vec<ListedPhase>) {
    let mut ran = failed_ci(task, None);
    ran.1[1].ends = PhaseEnd::Exit;
    ran.0.phases = crate::app::schedule::gather::phases_of(&ran.1);
    let facts = &mut ran.0;
    facts.pull = Some(pull(vec![check("build", CheckResult::Pending)]));
    facts.runs.push(RunRecord {
        action: name("ci"),
        head: Some(CommitId::from("abc123")),
        head_end: HeadEnd::Left(Some(CommitId::from("abc123"))),
        started_at: time(11),
        alive: false,
        exit: Some(RunExit {
            code: ExitCode::from(3),
            at: time(15),
        }),
        questions: BTreeSet::new(),
        last_line: None,
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
    let tasks = failed_waits(vec![failed_ci(441, None)]);
    let outcome = ask_outcome_alone(&board, &causes(&tasks)).expect("asks");
    assert_eq!(outcome.questions, vec![TicketId::from(441)]);
    assert_eq!(
        asked(&board),
        vec![(
            Some(TicketId::from(441)),
            "ci failed at abc123: failed checks build; it declares no on-fail. \
             Failed wait: ci at abc123, 21000000000."
                .to_owned()
        )],
        "one question naming ci, the head and the failed check"
    );
    assert_eq!(
        board.task(TicketId::from(441)).expect("task").state,
        Some(TicketState::Question),
        "the question makes the task wait"
    );
    let question = outcome.asked[0].question;
    assert_eq!(
        outcome.render_text(),
        format!(
            "question on #441\nasked {question} on #441: ci failed at abc123 and names no \
             on-fail\n"
        ),
        "the outcome lists the question with its task, then names it with its cause"
    );
}

#[test]
fn a_failed_run_names_its_exit_code() {
    let board = board_with(&[441]);
    ask_failed_waits(&board, &failed_waits(vec![ci_exited(441)]));
    assert_eq!(
        asked(&board)[0].1,
        "ci failed at abc123: exit code 3; it declares no on-fail. \
         Failed wait: ci at abc123, 15000000000."
    );
}

#[test]
fn an_open_question_is_not_asked_twice() {
    let board = board_with(&[441]);
    let tasks = failed_waits(vec![failed_ci(441, None)]);
    assert_eq!(ask_failed_waits(&board, &tasks).len(), 1);
    let second = ask_failed_waits(&board, &tasks);
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
    let raised = ask_failed_waits(&board, &failed_waits(vec![failed_ci(441, None)]));
    assert!(raised.is_empty(), "a task with any open question gets none");
    assert_eq!(asked(&board).len(), 1);
}

#[test]
fn a_wait_with_on_fail_does_not_ask() {
    let board = board_with(&[441]);
    let tasks = failed_waits(vec![failed_ci(441, Some("fix"))]);
    assert!(
        tasks.is_empty(),
        "the gate hands step 5 no wait that names an on-fail"
    );
    let outcome = ask_outcome_alone(&board, &causes(&tasks)).expect("asks");
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
    green.0.pull = Some(pull(vec![check("build", CheckResult::Passed(time(20)))]));
    let raised = ask_failed_waits(&board, &failed_waits(vec![green, failed_ci(441, None)]));
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
    let tasks = failed_waits(vec![failed_ci(441, None), ci_exited(442)]);
    let outcome = ask_outcome_alone(&board, &causes(&tasks)).expect("asks");

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

// A failed wait's ruling, keyed on the head.

/// A failed ci wait on #441 at `head`, its `build` check failed at `at`.
fn failed_at(head: &str, at: u64) -> Cause {
    failed_on(441, head, at)
}

/// [`failed_at`] on `ticket`.
fn failed_on(ticket: u64, head: &str, at: u64) -> Cause {
    Cause::FailedWait(FailedWait {
        ticket: id(ticket),
        action: name("ci"),
        failure: Failure {
            head: Some(CommitId::from(head)),
            checks: vec![CheckName::from("build")],
            exit: None,
            at: Some(time(at)),
        },
    })
}

/// Rules every open question on the board.
fn rule_all(board: &FakeBoard) {
    for question in board.questions().expect("lists") {
        board
            .record_ruling(question.id, "Seen; go on.")
            .expect("rules");
    }
}

#[test]
fn a_failed_wait_with_no_on_fail_is_not_asked_again_after_its_ruling_at_the_same_head() {
    let board = board_with(&[441]);
    assert_eq!(
        ask_alone(&board, &[failed_at("abc123", 21)])
            .expect("asks")
            .len(),
        1
    );
    rule_all(&board);
    // The ruling returned the task to ready; the gate still holds it at the same failure.
    let again = ask_alone(&board, &[failed_at("abc123", 21)]).expect("asks");
    assert!(again.is_empty(), "ruled at abc123: {again:?}");
    assert!(asked(&board).is_empty(), "no question is open");
}

#[test]
fn and_is_asked_again_at_a_new_head() {
    let board = board_with(&[441]);
    ask_alone(&board, &[failed_at("abc123", 21)]).expect("asks");
    rule_all(&board);
    let again = ask_alone(&board, &[failed_at("def456", 40)]).expect("asks");
    assert_eq!(again.len(), 1, "a new head asks again");
    assert_eq!(again[0].cause, "ci failed at def456 and names no on-fail");
    assert!(
        asked(&board)[0]
            .1
            .ends_with("Failed wait: ci at def456, 40000000000."),
        "{:?}",
        asked(&board)
    );
}

#[test]
fn a_new_failure_at_the_same_head_is_asked_again() {
    let board = board_with(&[441]);
    ask_alone(&board, &[failed_at("abc123", 21)]).expect("asks");
    rule_all(&board);
    // The checks ran again at the same head and failed again.
    let again = ask_alone(&board, &[failed_at("abc123", 50)]).expect("asks");
    assert_eq!(again.len(), 1, "a new failure asks again: {again:?}");
}

// The check deadline.

const T0: u64 = 1_800_000_000;
const HOUR: u64 = 60 * 60;

/// implement (a model ending at an open pull request), ci (a wait ending when CI is done, no
/// `on-fail`), merge (zaf).
fn pr_list() -> Vec<ListedPhase> {
    vec![
        listed("implement", ActionKind::Model, PhaseEnd::PrOpen, None),
        listed("ci", ActionKind::Wait, PhaseEnd::CiDone, None),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged, None),
    ]
}

/// A run of implement that left `head`, exiting 0 at `at`.
fn left(head: &str, at: u64) -> RunRecord {
    RunRecord {
        action: name("implement"),
        head: None,
        head_end: HeadEnd::Left(Some(CommitId::from(head))),
        started_at: time(at - 100),
        alive: false,
        exit: Some(RunExit {
            code: ExitCode::from(0),
            at: time(at),
        }),
        questions: BTreeSet::new(),
        last_line: None,
    }
}

/// Task #7, its pull request opened at [`T0`] at `head` with `checks`, after `runs`.
fn awaiting_checks(head: &str, checks: Vec<CheckFact>, runs: Vec<RunRecord>) -> TicketFacts {
    let pull = PullFacts {
        opened_at: time(T0),
        merged_at: None,
        head: Some(CommitId::from(head)),
        changes_a_file: true,
        checks,
        reviews: Vec::new(),
    };
    support::facts(7, &pr_list(), TaskSize::S, time(T0), Some(pull), runs)
}

fn gathered_of(tickets: Vec<TicketFacts>) -> Gathered {
    support::gathered(
        tickets
            .into_iter()
            .map(|facts| (facts, pr_list()))
            .collect(),
    )
}

/// The causes `gathered` raises at `now`.
fn waiting_at(gathered: &Gathered, now: u64) -> Vec<Cause> {
    let now = time(now);
    waiting_causes(gathered, &Placement::of(&gathered.tickets, now), now)
}

/// Step 5 on what `gathered` raises at `now`.
fn asks_at(board: &FakeBoard, gathered: &Gathered, now: u64) -> Vec<Asked> {
    ask_alone(board, &waiting_at(gathered, now)).expect("asks")
}

const HEAD_A: &str = "abc1234def5678";
const HEAD_B: &str = "fed9876cba5432";

#[test]
fn a_check_that_never_registers_raises_a_question_after_the_deadline() {
    let board = board_with(&[7]);
    let facts = awaiting_checks(HEAD_A, Vec::new(), vec![left(HEAD_A, T0 - 50)]);
    assert_eq!(
        place(&facts, time(T0 + 2 * HOUR)),
        Place::Waiting(BTreeSet::from([WaitingOn::Check(
            id(7),
            CommitId::from(HEAD_A)
        )])),
        "it waits on the checks at its head"
    );
    let gathered = gathered_of(vec![facts]);
    assert!(
        asks_at(&board, &gathered, T0 + 2 * HOUR - 1).is_empty(),
        "not before the deadline"
    );
    let raised = asks_at(&board, &gathered, T0 + 2 * HOUR);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(raised[0].task, id(7));
    assert_eq!(
        raised[0].cause,
        "no check has registered at abc1234 for 2 h"
    );
    assert_eq!(
        asked(&board),
        [(
            Some(id(7)),
            format!(
                "No check has registered at abc1234 for 2 h (since 2027-01-15T08:00:00Z): the \
                 runner may be down, or the workflow's path filter may have skipped this \
                 change. It waits on its checks until one ends there. Check deadline: {HEAD_A}."
            )
        )]
    );
    assert_eq!(
        board.task(id(7)).expect("task").state,
        Some(TicketState::Question)
    );
}

#[test]
fn a_pending_check_past_the_deadline_is_named() {
    let board = board_with(&[7]);
    let pending = vec![
        check("lint", CheckResult::Passed(time(T0 + 60))),
        check("build", CheckResult::Pending),
    ];
    let gathered = gathered_of(vec![awaiting_checks(
        HEAD_A,
        pending,
        vec![left(HEAD_A, T0 - 50)],
    )]);
    // The clock starts at the last check that finished there.
    assert!(asks_at(&board, &gathered, T0 + 60 + 2 * HOUR - 1).is_empty());
    let raised = asks_at(&board, &gathered, T0 + 60 + 3 * HOUR);
    assert_eq!(
        raised[0].cause,
        "the checks at abc1234 are still pending after 3 h"
    );
    assert!(
        asked(&board)[0].1.starts_with(
            "The checks build at abc1234 are still pending after 3 h (since \
             2027-01-15T08:01:00Z): the runner may be down."
        ),
        "{:?}",
        asked(&board)
    );
}

#[test]
fn a_deadline_question_is_raised_once_per_head() {
    let board = board_with(&[7]);
    let gathered = gathered_of(vec![awaiting_checks(
        HEAD_A,
        Vec::new(),
        vec![left(HEAD_A, T0 - 50)],
    )]);
    assert_eq!(asks_at(&board, &gathered, T0 + 2 * HOUR).len(), 1);
    assert!(
        asks_at(&board, &gathered, T0 + 3 * HOUR).is_empty(),
        "open at this head"
    );
    rule_all(&board);
    assert!(
        asks_at(&board, &gathered, T0 + 5 * HOUR).is_empty(),
        "ruled at this head"
    );
    assert!(asked(&board).is_empty());
}

#[test]
fn a_new_head_starts_the_check_clock_again() {
    let board = board_with(&[7]);
    let at_a = gathered_of(vec![awaiting_checks(
        HEAD_A,
        Vec::new(),
        vec![left(HEAD_A, T0 - 50)],
    )]);
    assert_eq!(asks_at(&board, &at_a, T0 + 2 * HOUR).len(), 1);
    rule_all(&board);
    // A run pushed a new head at T0 + 4 h; its checks have not registered either.
    let t1 = T0 + 4 * HOUR;
    let facts = awaiting_checks(
        HEAD_B,
        Vec::new(),
        vec![left(HEAD_A, T0 - 50), left(HEAD_B, t1)],
    );
    assert_eq!(
        place(&facts, time(t1 + HOUR)),
        Place::Waiting(BTreeSet::from([WaitingOn::Check(
            id(7),
            CommitId::from(HEAD_B)
        )]))
    );
    let at_b = gathered_of(vec![facts]);
    assert!(
        asks_at(&board, &at_b, t1 + 2 * HOUR - 1).is_empty(),
        "the clock started again at the new head"
    );
    let raised = asks_at(&board, &at_b, t1 + 2 * HOUR);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(
        raised[0].cause,
        "no check has registered at fed9876 for 2 h"
    );
    assert!(
        asked(&board)[0]
            .1
            .ends_with(&format!("Check deadline: {HEAD_B}."))
    );
}

#[test]
fn a_question_and_an_unsigned_epic_have_no_clock() {
    let board = board_with(&[7]);
    let mut facts = awaiting_checks(HEAD_A, Vec::new(), vec![left(HEAD_A, T0 - 50)]);
    facts.open_questions = BTreeSet::from([id(90)]);
    assert!(
        waiting_at(&gathered_of(vec![facts.clone()]), T0 + 30 * HOUR).is_empty(),
        "a question is the Engineer's already"
    );
    facts.open_questions.clear();
    facts.epic = Some(crate::domain::schedule::EpicFacts {
        id: id(1),
        signed: false,
        challenged: false,
    });
    let mut gathered = gathered_of(vec![facts]);
    gathered.epics = vec![epic(None, None, false)];
    assert!(
        waiting_at(&gathered, T0 + 30 * HOUR).is_empty(),
        "an unsigned epic is the Engineer's to sign"
    );
    assert!(asks_at(&board, &gathered, T0 + 30 * HOUR).is_empty());
}

// Dead dependencies.

/// Task #7 waiting on `dependencies`, each with its standing.
fn depending(ticket: u64, dependencies: &[(u64, Standing)]) -> TicketFacts {
    let mut facts = support::facts(ticket, &pr_list(), TaskSize::S, time(T0), None, Vec::new());
    facts.dependencies = dependencies
        .iter()
        .map(|(dependency, standing)| Dependency {
            ticket: id(*dependency),
            standing: *standing,
        })
        .collect();
    facts
}

#[test]
fn a_dependency_closed_as_not_planned_raises_a_question_at_once() {
    let board = board_with(&[7]);
    let gathered = gathered_of(vec![depending(7, &[(9, Standing::ClosedNotPlanned)])]);
    let raised = asks_at(&board, &gathered, T0);
    assert_eq!(raised.len(), 1, "at once, with no clock: {raised:?}");
    assert_eq!(
        raised[0].cause,
        "its dependency #9 is closed as not planned"
    );
    assert_eq!(
        asked(&board),
        [(
            Some(id(7)),
            "#7 waits on #9, which can never complete: #9 is closed as not planned. #7 waits \
             until #9 is done or leaves its Blocked by: line. Dead dependency: #9."
                .to_owned()
        )]
    );
    assert!(asks_at(&board, &gathered, T0 + 60).is_empty(), "once");
    rule_all(&board);
    assert!(
        asks_at(&board, &gathered, T0 + 120).is_empty(),
        "once per ticket and dependency, ruled or not"
    );
}

#[test]
fn a_dependency_that_is_no_ticket_raises_a_question_at_once() {
    let board = board_with(&[7]);
    let gathered = gathered_of(vec![depending(
        7,
        &[(8, Standing::Done), (9, Standing::Missing)],
    )]);
    let raised = asks_at(&board, &gathered, T0);
    assert_eq!(raised.len(), 1, "the done one is no dead one: {raised:?}");
    assert_eq!(raised[0].cause, "its dependency #9 is not a ticket");
    assert_eq!(
        asked(&board)[0].1,
        "#7 waits on #9, which can never complete: #9 is not a ticket (no such issue, or a pull \
         request). #7 waits until #9 is done or leaves its Blocked by: line. Dead dependency: #9."
    );
}

#[test]
fn a_dependency_cycle_raises_one_question_naming_the_cycle() {
    let board = board_with(&[3, 5, 7]);
    let gathered = gathered_of(vec![
        depending(3, &[(5, Standing::Open)]),
        depending(5, &[(7, Standing::Open)]),
        depending(7, &[(3, Standing::Open)]),
    ]);
    let raised = asks_at(&board, &gathered, T0);
    assert_eq!(raised.len(), 1, "one question for the cycle: {raised:?}");
    assert_eq!(raised[0].task, id(3), "on its lowest ticket");
    assert_eq!(
        raised[0].cause,
        "its dependencies form a cycle: #3 → #5 → #7 → #3"
    );
    assert_eq!(
        asked(&board),
        [(
            Some(id(3)),
            "These tickets wait on each other: #3 → #5 → #7 → #3, so none of them can ever \
             complete. They wait until a Blocked by: line breaks the cycle. Dependency cycle: \
             #3, #5, #7."
                .to_owned()
        )]
    );
    assert!(asks_at(&board, &gathered, T0 + 60).is_empty(), "once");
}

// The challenge deadline.

const DAY: u64 = 24 * HOUR;

/// Epic #1 with one open task, signed at `signed` (none: unsigned), its planning ended at
/// `planned`, challenged or not.
fn epic(signed: Option<u64>, planned: Option<u64>, challenged: bool) -> Epic {
    let body = "Goal: ship it".into();
    Epic {
        id: id(1),
        title: "Epic".into(),
        signature: signed.map(|at| Signature {
            hash: BodyHash::of(&body),
            at: time(at),
        }),
        body,
        tasks: vec![EpicTask {
            number: id(2),
            title: "Task".into(),
            state: None,
            standing: Standing::Open,
            group: None,
            kind: None,
        }],
        planned_at: planned.map(time),
        challenged,
    }
}

#[test]
fn an_epic_not_challenged_for_a_day_raises_a_question_on_the_epic() {
    let board = FakeBoard::default();
    board.add_signed_epic(id(1), "Epic");
    let mut gathered = Gathered {
        epics: vec![epic(Some(T0 - HOUR), Some(T0), false)],
        ..Gathered::default()
    };
    assert!(asks_at(&board, &gathered, T0 + DAY - 1).is_empty());
    let raised = asks_at(&board, &gathered, T0 + DAY);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(raised[0].task, id(1), "on the epic");
    assert_eq!(raised[0].cause, "it has had no challenge for 24 h");
    assert_eq!(
        asked(&board),
        [(
            Some(id(1)),
            "Epic #1 has tasks but no challenge after 24 h (since 2027-01-15T08:00:00Z): the \
             challenger may have ended without a report, or never got a slot. Its tasks wait \
             until it is challenged. Challenge deadline: 1800000000000000000."
                .to_owned()
        )]
    );
    // Gathered again, the epic has its question open: no clock.
    gathered.questions = questions_by_epic(&board);
    assert!(waiting_at(&gathered, T0 + 2 * DAY).is_empty());
    // Ruled, it is not asked again for this planning.
    rule_all(&board);
    gathered.questions.clear();
    assert!(asks_at(&board, &gathered, T0 + 3 * DAY).is_empty());
    // A challenged epic has no clock.
    gathered.epics = vec![epic(Some(T0 - HOUR), Some(T0), true)];
    assert!(waiting_at(&gathered, T0 + 3 * DAY).is_empty());
}

/// The open questions on the board by the issue each was raised on.
fn questions_by_epic(
    board: &FakeBoard,
) -> std::collections::BTreeMap<TicketId, BTreeSet<TicketId>> {
    crate::app::schedule::gather::questions_by_issue(&board.questions().expect("lists"))
}

// One open question per cause.

#[test]
fn no_cause_has_two_open_questions() {
    let board = board_with(&[3, 5, 7]);
    let gathered = gathered_of(vec![
        depending(3, &[(5, Standing::Open)]),
        depending(5, &[(3, Standing::Open)]),
        depending(
            7,
            &[(9, Standing::ClosedNotPlanned), (11, Standing::Missing)],
        ),
    ]);
    let first = asks_at(&board, &gathered, T0);
    let causes: Vec<&str> = first.iter().map(|asked| asked.cause.as_str()).collect();
    assert_eq!(
        causes,
        [
            "its dependency #9 is closed as not planned",
            "its dependency #11 is not a ticket",
            "its dependencies form a cycle: #3 → #5 → #3",
        ],
        "two causes on #7 are two questions"
    );
    // The same causes again, the same tick or the next, raise nothing.
    let again = ask_alone(&board, &waiting_at(&gathered, T0)).expect("asks");
    assert!(again.is_empty(), "{again:?}");
    assert!(asks_at(&board, &gathered, T0 + DAY).is_empty());
    assert_eq!(asked(&board).len(), 3);
    // Within one call, a cause named twice is asked once.
    let board = board_with(&[441]);
    let twice =
        ask_alone(&board, &[failed_at("abc123", 21), failed_at("abc123", 21)]).expect("asks");
    assert_eq!(twice.len(), 1);
}

#[test]
fn each_question_is_named_with_its_cause() {
    let asked = Asked {
        task: id(7),
        question: id(61),
        cause: "merge got nowhere three times at d962227".to_owned(),
    };
    assert_eq!(
        asked.line(),
        "asked #61 on #7: merge got nowhere three times at d962227"
    );
}

// Two ticks that overlap.

/// The board as a tick reads it while another tick, which overlaps it, raises its questions:
/// `other` runs once, right after this tick's first read of the open questions, so what the other
/// tick raises is written between that read and the read under the lock. Each read of the open
/// questions and of the ruled ones, and each question opened, is logged with whether `spawn.lock`
/// in `state_dir` was held then.
struct Overlapping<'a> {
    board: &'a FakeBoard,
    state_dir: &'a Path,
    other: std::cell::RefCell<Option<Box<dyn FnOnce() + 'a>>>,
    log: std::cell::RefCell<Vec<(&'static str, bool)>>,
}

impl<'a> Overlapping<'a> {
    fn new(board: &'a FakeBoard, state_dir: &'a Path, other: impl FnOnce() + 'a) -> Self {
        Self {
            board,
            state_dir,
            other: std::cell::RefCell::new(Some(Box::new(other))),
            log: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Logs `call` with whether the spawner's lock is held now.
    fn note(&self, call: &'static str) {
        // A lock just let go can still read as held for an instant: another test's child process,
        // between its fork and its exec, holds a copy of the file. A lock this tick really holds
        // stays held, so a held reading is looked at again before it is believed.
        let mut held = true;
        for _ in 0..20 {
            held = crate::app::spawn::lock_is_held(self.state_dir).expect("the lock is read");
            if !held {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.log.borrow_mut().push((call, held));
    }

    fn log(&self) -> Vec<(&'static str, bool)> {
        self.log.borrow().clone()
    }
}

impl Board for Overlapping<'_> {
    fn ready(&self) -> Result<Vec<Ticket>, crate::ports::PortError> {
        self.board.ready()
    }
    fn task(&self, number: TicketId) -> Result<Ticket, crate::ports::PortError> {
        self.board.task(number)
    }
    fn claim(
        &self,
        task: TicketId,
        instance: &Instance,
    ) -> Result<crate::ports::ClaimOutcome, crate::ports::PortError> {
        self.board.claim(task, instance)
    }
    fn heartbeat(
        &self,
        task: TicketId,
        instance: &Instance,
    ) -> Result<(), crate::ports::PortError> {
        self.board.heartbeat(task, instance)
    }
    fn claims(&self) -> Result<Vec<crate::ports::ClaimRecord>, crate::ports::PortError> {
        self.board.claims()
    }
    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), crate::ports::PortError> {
        self.board.release(task, instance)
    }
    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), crate::ports::PortError> {
        self.board.set_state(task, label)
    }
    fn comment(&self, issue: TicketId, body: &str) -> Result<(), crate::ports::PortError> {
        self.board.comment(issue, body)
    }
    fn report(
        &self,
        task: TicketId,
        instance: &Instance,
        body: &str,
    ) -> Result<(), crate::ports::PortError> {
        self.board.report(task, instance, body)
    }
    fn questions(&self) -> Result<Vec<Ticket>, crate::ports::PortError> {
        self.note("questions");
        let read = self.board.questions();
        let other = self.other.borrow_mut().take();
        if let Some(other) = other {
            other();
        }
        read
    }
    fn questions_on(
        &self,
        task: TicketId,
    ) -> Result<Vec<(Ticket, Option<String>)>, crate::ports::PortError> {
        self.note("questions_on");
        self.board.questions_on(task)
    }
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, crate::ports::PortError> {
        self.note("open_question");
        self.board.open_question(question)
    }
    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, crate::ports::PortError> {
        self.board.record_ruling(question, ruling)
    }
}

#[test]
fn two_ticks_that_both_see_a_cause_raise_one_question() {
    let state_dir = tempfile::tempdir().expect("tempdir");
    let board = board_with(&[441]);
    let first = std::cell::RefCell::new(Vec::new());
    // Both ticks find ci failed on #441 with no question open on it. The first raises its
    // question once the second has read the open ones, and before the second reads them again.
    let second = Overlapping::new(&board, state_dir.path(), || {
        *first.borrow_mut() =
            ask(&board, state_dir.path(), &[failed_at("abc123", 21)]).expect("the first asks");
    });

    let raised =
        ask(&second, state_dir.path(), &[failed_at("abc123", 21)]).expect("the second asks");

    assert_eq!(first.borrow().len(), 1, "the first tick raised it");
    assert!(raised.is_empty(), "the second raised none: {raised:?}");
    assert_eq!(
        asked(&board).len(),
        1,
        "the board holds one question: {:?}",
        asked(&board)
    );
    assert_eq!(
        second.log(),
        [
            ("questions", false),
            ("questions_on", false),
            ("questions", true)
        ],
        "the first read is outside spawn.lock, the read before the write under it"
    );
}

#[test]
fn a_question_raised_by_another_tick_is_not_counted_as_asked_by_this_one() {
    let state_dir = tempfile::tempdir().expect("tempdir");
    let board = board_with(&[441, 442]);
    // The other tick raises #441's question between this tick's first read and its read under
    // the lock; #442's is this tick's alone.
    let this = Overlapping::new(&board, state_dir.path(), || {
        ask(&board, state_dir.path(), &[failed_at("abc123", 21)]).expect("the other asks");
    });

    let outcome = ask_outcome(
        &this,
        state_dir.path(),
        &[failed_at("abc123", 21), failed_on(442, "abc123", 21)],
    )
    .expect("this tick asks");

    assert_eq!(outcome.questions, [id(442)], "{outcome:?}");
    let asked_here: Vec<(TicketId, &str)> = outcome
        .asked
        .iter()
        .map(|asked| (asked.task, asked.cause.as_str()))
        .collect();
    assert_eq!(
        asked_here,
        [(id(442), "ci failed at abc123 and names no on-fail")]
    );
    let on: Vec<Option<TicketId>> = asked(&board).into_iter().map(|(on, _)| on).collect();
    assert_eq!(on, [Some(id(441)), Some(id(442))], "one question on each");
    // The lock is held for one question at a time: let go between #441's and #442's.
    assert_eq!(
        this.log(),
        [
            ("questions", false),
            ("questions_on", false),
            ("questions", true),
            ("questions_on", false),
            ("questions", true),
            ("open_question", true)
        ]
    );
}

#[test]
fn a_lock_that_cannot_be_taken_raises_nothing_and_says_why() {
    let dir = tempfile::tempdir().expect("tempdir");
    let not_a_folder = dir.path().join("state");
    std::fs::write(&not_a_folder, "a file").expect("a file in the way");
    let board = board_with(&[441]);

    let error = ask(&board, &not_a_folder, &[failed_at("abc123", 21)]).expect_err("no lock");

    assert!(matches!(error, AskError::Lock(_)), "{error:?}");
    assert!(error.to_string().starts_with("cannot write "), "{error}");
    assert!(asked(&board).is_empty(), "nothing raised");
}

// A red integration branch (R29).

/// The integration branch `feature/v1` at `head` with `checks`, as a gather read it.
fn base_at(head: &str, checks: Vec<CheckFact>) -> Base {
    Base {
        branch: crate::ports::BranchName::from("feature/v1"),
        facts: crate::domain::schedule::BaseFacts {
            head: Some(CommitId::from(head)),
            checks,
        },
    }
}

/// Task #7 waiting on its checks, gathered over `base`, under the signed epic #1 of a board.
fn over_base(base: Option<Base>) -> (FakeBoard, Gathered) {
    let board = FakeBoard::default();
    board.add_signed_epic(id(1), "Epic");
    let mut gathered = gathered_of(vec![awaiting_checks(HEAD_A, Vec::new(), Vec::new())]);
    gathered.base = base;
    (board, gathered)
}

#[test]
fn a_red_integration_branch_raises_one_question_on_the_epic() {
    let red = vec![
        check("ci / test", CheckResult::Failed(time(T0))),
        check("ci / lint", CheckResult::Passed(time(T0))),
        check("ci / build", CheckResult::Failed(time(T0))),
    ];
    let (board, gathered) = over_base(Some(base_at(HEAD_B, red)));
    let raised = asks_at(&board, &gathered, T0);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(raised[0].task, id(1), "on the epic");
    assert_eq!(raised[0].cause, "feature/v1 is red at fed9876");
    assert_eq!(
        asked(&board),
        [(
            Some(id(1)),
            format!(
                "The integration branch feature/v1 is red: the checks ci / test, ci / build \
                 have failed at fed9876. The tickets of epic #1 merge into it, so they wait on \
                 this question. File a fix, or rule to carry on: once this is ruled they go on, \
                 merges included, though the branch is still red at that commit. Red base: \
                 feature/v1 at {HEAD_B}."
            )
        )]
    );
    // The same red head, at the next tick: the question is open, and no second one is raised.
    assert!(asks_at(&board, &gathered, T0 + HOUR).is_empty());
    assert_eq!(asked(&board).len(), 1);
}

#[test]
fn a_ruled_red_head_is_not_asked_again_and_a_new_red_head_is() {
    let red = |head: &str| {
        Some(base_at(
            head,
            vec![check("ci", CheckResult::Failed(time(T0)))],
        ))
    };
    let (board, mut gathered) = over_base(red(HEAD_B));
    assert_eq!(asks_at(&board, &gathered, T0).len(), 1);
    rule_all(&board);
    // Ruled: work goes on though the branch is still red there.
    assert!(asks_at(&board, &gathered, T0 + HOUR).is_empty());
    assert!(board.questions().expect("lists").is_empty());
    // A merge moved the branch and it is red again: a new head, a new question.
    gathered.base = red("0123456789abcdef");
    let raised = asks_at(&board, &gathered, T0 + HOUR + 1);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(raised[0].cause, "feature/v1 is red at 0123456");
}

#[test]
fn a_branch_that_is_green_pending_or_unknown_raises_nothing() {
    for checks in [
        Vec::new(),
        vec![check("ci", CheckResult::Passed(time(T0)))],
        vec![check("ci", CheckResult::Pending)],
    ] {
        let (board, gathered) = over_base(Some(base_at(HEAD_B, checks.clone())));
        assert!(asks_at(&board, &gathered, T0).is_empty(), "{checks:?}");
    }
    let (board, gathered) = over_base(None);
    assert!(asks_at(&board, &gathered, T0).is_empty(), "not read");
    // Failed checks with no head are an unknown branch: nothing to name.
    let mut headless = base_at(HEAD_B, vec![check("ci", CheckResult::Failed(time(T0)))]);
    headless.facts.head = None;
    let (board, gathered) = over_base(Some(headless));
    assert!(asks_at(&board, &gathered, T0).is_empty());
}

#[test]
fn a_red_branch_is_asked_once_for_each_epic_with_a_ticket_not_done() {
    let board = FakeBoard::default();
    for epic in [1, 2, 3] {
        board.add_signed_epic(id(epic), "Epic");
    }
    let mut second = awaiting_checks(HEAD_A, Vec::new(), Vec::new());
    second.ticket = id(8);
    let mut merged = awaiting_checks(HEAD_A, Vec::new(), Vec::new());
    merged.ticket = id(9);
    if let Some(pull) = merged.pull.as_mut() {
        pull.merged_at = Some(time(T0));
    }
    let mut gathered = gathered_of(vec![
        awaiting_checks(HEAD_A, Vec::new(), Vec::new()),
        second,
        merged,
    ]);
    for (ticket, epic) in [(8, 2), (9, 3)] {
        if let Some(beside) = gathered.beside.get_mut(&id(ticket)) {
            beside.epic = id(epic);
        }
    }
    gathered.base = Some(base_at(
        HEAD_B,
        vec![check("ci", CheckResult::Failed(time(T0)))],
    ));
    let raised = asks_at(&board, &gathered, T0);
    let on: Vec<TicketId> = raised.iter().map(|asked| asked.task).collect();
    assert_eq!(on, [id(1), id(2)], "epic #3 has only a merged ticket");
    assert!(
        asked(&board)[0].1.contains("the check ci has failed at"),
        "{:?}",
        asked(&board)
    );
}

// An overdue wait on the integration branch's checks (R29, R19).

/// Task #7 on a board, green and with its pull request open since [`T0`], so its merge is next,
/// over the branch `feature/v1` at [`HEAD_B`] with `ci / test` still pending there.
fn held_at_its_merge() -> (FakeBoard, Gathered) {
    let board = board_with(&[7]);
    let list = vec![
        listed("implement", ActionKind::Model, PhaseEnd::PrOpen, None),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged, None),
    ];
    let pull = PullFacts {
        opened_at: time(T0),
        merged_at: None,
        head: Some(CommitId::from(HEAD_A)),
        changes_a_file: true,
        checks: Vec::new(),
        reviews: Vec::new(),
    };
    let base = base_at(HEAD_B, vec![check("ci / test", CheckResult::Pending)]);
    let mut facts = support::facts(7, &list, TaskSize::S, time(T0), Some(pull), Vec::new());
    facts.merging.base = base.facts.clone();
    let mut gathered = support::gathered(vec![(facts, list)]);
    gathered.base = Some(base);
    (board, gathered)
}

#[test]
fn a_merge_held_two_hours_by_the_integration_branch_raises_a_question_on_the_ticket() {
    let (board, gathered) = held_at_its_merge();
    assert_eq!(
        place(&gathered.tickets[0], time(T0 + 2 * HOUR)),
        Place::Waiting(BTreeSet::from([WaitingOn::Base(CommitId::from(HEAD_B))]))
    );
    assert!(
        asks_at(&board, &gathered, T0 + 2 * HOUR - 1).is_empty(),
        "not before the deadline"
    );
    let raised = asks_at(&board, &gathered, T0 + 2 * HOUR);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(raised[0].task, id(7), "on the ticket that waits");
    assert_eq!(
        raised[0].cause,
        "the integration branch's checks at fed9876 are still pending after 2 h at its merge"
    );
    assert_eq!(
        asked(&board),
        [(
            Some(id(7)),
            format!(
                "The checks ci / test of the integration branch feature/v1 are still pending \
                 at fed9876, and #7 has waited 2 h to merge (since 2027-01-15T08:00:00Z): the \
                 runner may be down. Its merge waits until they end there; once this is ruled \
                 it merges at that commit without them. Base deadline: {HEAD_B}."
            )
        )]
    );
}

#[test]
fn an_overdue_base_wait_is_not_asked_twice_nor_again_once_ruled() {
    let (board, gathered) = held_at_its_merge();
    assert_eq!(asks_at(&board, &gathered, T0 + 2 * HOUR).len(), 1);
    // Still open at the next tick: the same ticket and head raise nothing.
    assert!(asks_at(&board, &gathered, T0 + 3 * HOUR).is_empty());
    assert_eq!(asked(&board).len(), 1);
    // Ruled: not asked again at that head, even gathered before the ruling was read.
    rule_all(&board);
    assert!(asks_at(&board, &gathered, T0 + 4 * HOUR).is_empty());
    assert!(board.questions().expect("lists").is_empty());
}

#[test]
fn a_new_head_of_the_branch_is_a_new_base_deadline() {
    let (board, mut gathered) = held_at_its_merge();
    assert_eq!(asks_at(&board, &gathered, T0 + 2 * HOUR).len(), 1);
    rule_all(&board);
    let moved = base_at(HEAD_A, vec![check("ci / test", CheckResult::Pending)]);
    gathered.tickets[0].merging.base = moved.facts.clone();
    gathered.base = Some(moved);
    let raised = asks_at(&board, &gathered, T0 + 5 * HOUR);
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(
        asked(&board)[0]
            .1
            .ends_with(&format!("Base deadline: {HEAD_A}.")),
        "{:?}",
        asked(&board)
    );
}
