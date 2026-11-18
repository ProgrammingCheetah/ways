//! The core against today's rule table and the day's eight real failures (2026-11-12), and the
//! builders every module's tests share.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use super::*;
use crate::domain::{CommitId, Standing, TaskSize, TicketId, TicketState};

/// The ticket the scenes are about.
pub(super) const TICKET: u64 = 437;

/// The time the scenes place at, well after everything in them.
pub(super) const NOW: u64 = 100_000;

pub(super) fn ticket() -> TicketId {
    TicketId::from(TICKET)
}

pub(super) fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

pub(super) fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

pub(super) fn now() -> SystemTime {
    time(NOW)
}

pub(super) fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

pub(super) fn commit(text: &str) -> CommitId {
    CommitId::from(text)
}

pub(super) fn phase(action: &str, ends: &[PhaseEnd], on_fail: Option<&str>) -> Phase {
    Phase {
        name: name(action),
        ends: ends.to_vec(),
        on_fail: on_fail.map(name),
    }
}

/// The pack's list for a Task: implement opens the pull request, CI and the review fail to `fix`,
/// and merge.
pub(super) fn pack() -> Vec<Phase> {
    vec![
        phase("implement", &[PhaseEnd::PrOpen], None),
        phase("ci", &[PhaseEnd::CiDone], Some("fix")),
        phase("review", &[PhaseEnd::Exit], Some("fix")),
        phase("merge", &[PhaseEnd::Merged], None),
    ]
}

/// A ticket with `phases` and nothing else: no epic, question, dependency, pull request or run,
/// ready since 1.
pub(super) fn facts(phases: Vec<Phase>) -> TicketFacts {
    TicketFacts {
        ticket: ticket(),
        epic: None,
        open_questions: BTreeSet::new(),
        dependencies: Vec::new(),
        pull: None,
        runs: Vec::new(),
        phases,
        size: TaskSize::M,
        priority: None,
        ruled: Vec::new(),
        ready_since: Some(time(1)),
        merging: Default::default(),
        no_check_ruled: None,
    }
}

/// A pull request opened at `opened`, at `head`, changing a file, with no check or review.
pub(super) fn pull(head: &str, opened: u64) -> PullFacts {
    PullFacts {
        opened_at: time(opened),
        merged_at: None,
        head: Some(commit(head)),
        changed_file: Some(crate::domain::RepoPath::from("src/lib.rs")),
        checks: Vec::new(),
        reviews: Vec::new(),
    }
}

pub(super) fn check(name: &str, result: CheckResult) -> CheckFact {
    CheckFact {
        name: CheckName::from(name),
        result,
    }
}

pub(super) fn passed(at: u64) -> CheckFact {
    check("ci", CheckResult::Passed(time(at)))
}

pub(super) fn failed(at: u64) -> CheckFact {
    check("ci", CheckResult::Failed(time(at)))
}

pub(super) fn pending() -> CheckFact {
    check("ci", CheckResult::Pending)
}

pub(super) fn review(head: Option<&str>, at: u64) -> ReviewFact {
    ReviewFact {
        head: head.map(commit),
        posted_at: time(at),
    }
}

/// A run of `action` started at `started` at `head`: dead, no exit recorded, no end head (an old
/// folder), no question.
pub(super) fn run(action: &str, head: Option<&str>, started: u64) -> RunRecord {
    RunRecord {
        action: name(action),
        head: head.map(commit),
        head_end: HeadEnd::Unknown,
        started_at: time(started),
        alive: false,
        exit: None,
        questions: BTreeSet::new(),
        last_line: None,
    }
}

/// What a scene adds to a run.
pub(super) trait Recorded {
    /// It exited `code` at `at`.
    fn exits(self, code: i32, at: u64) -> Self;
    /// It left `head`.
    fn leaves(self, head: Option<&str>) -> Self;
    /// It raised the question `number`.
    fn asks(self, number: u64) -> Self;
    /// Its process is alive.
    fn lives(self) -> Self;
}

impl Recorded for RunRecord {
    fn exits(self, code: i32, at: u64) -> Self {
        Self {
            exit: Some(RunExit {
                code: ExitCode::from(code),
                at: time(at),
            }),
            ..self
        }
    }

    fn leaves(self, head: Option<&str>) -> Self {
        Self {
            head_end: HeadEnd::Left(head.map(commit)),
            ..self
        }
    }

    fn asks(mut self, number: u64) -> Self {
        self.questions.insert(id(number));
        self
    }

    fn lives(self) -> Self {
        Self {
            alive: true,
            ..self
        }
    }
}

pub(super) fn in_lane(next: &str, since: u64) -> Place {
    Place::InLane {
        next: name(next),
        since: time(since),
    }
}

pub(super) fn waiting(on: &[WaitingOn]) -> Place {
    Place::Waiting(on.iter().cloned().collect())
}

/// A wait on the checks at `head` of the scene's ticket.
pub(super) fn on_check(head: &str) -> WaitingOn {
    WaitingOn::Check(ticket(), commit(head))
}

// ---------------------------------------------------------------------------------------------
// The parity table: every case of app/actions/phase/standing/tests.rs, ported.
//
// Today's answer is written as the place it means: `Ready { action, since }` is in the lane at
// `action`; `Failed { on_fail: Some(f), since }` is in the lane at `f`, and with no on-fail at the
// wait itself; `Waiting { action }` waits on the check at the head; `Running` and `Done` are the
// same. Not ported, being the gather's and not the core's: an_unreadable_forge_gives_no_standing,
// a_recorded_pr_the_forge_does_not_know_is_unreadable, a_finished_check_without_a_time_is_unreadable
// (the gather fails instead of handing facts) and nothing_is_written_while_deriving (the core can
// write nothing: the domain imports no outer layer, which tests/architecture.rs enforces).
//
// The intended differences, each named in its row:
//
// - D1: `exit` is keyed on the head after the `pr-open` phase. There a run must have left the pull
//   request's current head, so an exit phase whose run left no head or another head does not hold
//   at it. A phase listed at or before the `pr-open` phase (a brief, the old list's implement)
//   holds on any run that exited 0 and raised no question, whatever the head, so the old list
//   with a pull request ends implement as it did; a list with no `pr-open` phase keys every
//   phase. The rows with a pull request use the old list with implement ending at `pr-open`, as
//   the pack's implement does, and the old list has a row of its own.
// - D2: with a `ci-done` phase and a head, nothing is next until every check at the head ended;
//   none registered counts as not ended. The rows with no check get a twin with a green check.
// - D3: the `ci-done` phase is never next: while checks run the ticket waits on them, a red one
//   runs its on-fail, and green ends it.
// - D4: a failed run makes the ticket ready since it failed, as a failed wait did; a model phase
//   used to keep the earlier `since`.
// - D5: `pr-open` holds only for a pull request that changes a file.
// - D6: a run that raised a question is never done, whatever its exit code, and it stays so once
//   the question is ruled: the phase runs again (the unified exit rule of slice 3).
// - D7: a wait whose run died with no exit code runs again; it used to wait forever.
// - D8 (getting nowhere, its own test below): exit-0 runs at an unmoved head count.
// ---------------------------------------------------------------------------------------------

/// One case: the facts, today's answer as a place, the core's answer and the difference.
struct Row {
    case: &'static str,
    facts: TicketFacts,
    today: Place,
    core: Place,
    difference: Option<&'static str>,
}

/// The list of the old tests: implement, open the pull request, a review wait, CI, merge.
fn old_list() -> Vec<Phase> {
    vec![
        phase("implement", &[PhaseEnd::Exit], None),
        phase("open-pr", &[PhaseEnd::PrOpen], None),
        phase("review", &[PhaseEnd::ReviewPosted], Some("fix")),
        phase("ci", &[PhaseEnd::CiDone], Some("fix")),
        phase("merge", &[PhaseEnd::Merged], None),
    ]
}

/// The old list with implement ending at `pr-open` (D1), for the rows with a pull request.
fn pr_list() -> Vec<Phase> {
    let mut list = old_list();
    list[0].ends = vec![PhaseEnd::PrOpen];
    list
}

/// The old scene `opened`: implement exited 0 at 10 and the pull request opened at 20 at `h1`.
fn opened() -> TicketFacts {
    let mut facts = facts(pr_list());
    facts.runs.push(run("implement", None, 5).exits(0, 10));
    facts.pull = Some(pull("h1", 20));
    facts
}

fn with_checks(mut facts: TicketFacts, checks: Vec<CheckFact>) -> TicketFacts {
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = checks;
    }
    facts
}

fn with_reviews(mut facts: TicketFacts, reviews: Vec<ReviewFact>) -> TicketFacts {
    if let Some(pull) = facts.pull.as_mut() {
        pull.reviews = reviews;
    }
    facts
}

fn with_run(mut facts: TicketFacts, run: RunRecord) -> TicketFacts {
    facts.runs.push(run);
    facts
}

fn at_head(mut facts: TicketFacts, head: &str) -> TicketFacts {
    if let Some(pull) = facts.pull.as_mut() {
        pull.head = Some(commit(head));
    }
    facts
}

fn same(case: &'static str, facts: TicketFacts, today: Place) -> Row {
    Row {
        case,
        facts,
        core: today.clone(),
        today,
        difference: None,
    }
}

fn differs(
    case: &'static str,
    facts: TicketFacts,
    today: Place,
    core: Place,
    difference: &'static str,
) -> Row {
    Row {
        case,
        facts,
        today,
        core,
        difference: Some(difference),
    }
}

fn parity_rows() -> Vec<Row> {
    let reviewed = || with_reviews(opened(), vec![review(Some("h1"), 30)]);
    let no_on_fail = {
        let mut facts = with_checks(reviewed(), vec![failed(40)]);
        facts.phases[3].on_fail = None;
        facts
    };
    let failed_review = with_run(opened(), run("review", Some("h1"), 25).exits(1, 26));
    let ran_at_old_head = with_run(opened(), run("review", Some("h0"), 25).exits(0, 26));
    let waited_review = with_run(opened(), run("review", Some("h1"), 25).exits(0, 26));
    let polish = {
        let mut facts = facts(vec![
            phase("open-pr", &[PhaseEnd::PrOpen], None),
            phase("polish", &[PhaseEnd::Exit], None),
        ]);
        facts.pull = Some(pull("h1", 20));
        with_run(facts, run("polish", None, 5).exits(0, 10))
    };
    let folded = || {
        facts(vec![
            phase("implement", &[PhaseEnd::Exit, PhaseEnd::Exit], None),
            phase("open-pr", &[PhaseEnd::PrOpen], None),
        ])
    };
    let split = |implement: RunRecord| {
        let mut facts = facts(vec![
            phase("implement", &[PhaseEnd::Exit], None),
            phase("review", &[PhaseEnd::ReviewPosted], None),
            phase("document", &[PhaseEnd::Exit], None),
        ]);
        facts.pull = Some(pull("h1", 20));
        with_run(facts, implement)
    };
    let merged = {
        let mut facts = opened();
        if let Some(pull) = facts.pull.as_mut() {
            pull.merged_at = Some(time(50));
        }
        facts
    };
    let old_opened = {
        let mut facts = with_checks(reviewed(), vec![passed(40)]);
        facts.phases = old_list();
        facts
    };
    vec![
        same(
            "green_checks_make_merge_ready",
            with_checks(reviewed(), vec![passed(40)]),
            in_lane("merge", 40),
        ),
        same(
            "a_failed_check_is_failed",
            with_checks(reviewed(), vec![failed(40)]),
            in_lane("fix", 40),
        ),
        same(
            "a_failed_wait_without_on_fail_has_none",
            no_on_fail,
            in_lane("ci", 40),
        ),
        differs(
            "a_failed_run_of_a_wait_at_the_head_is_failed",
            failed_review.clone(),
            in_lane("fix", 26),
            waiting(&[on_check("h1")]),
            "D2",
        ),
        same(
            "a_failed_run_of_a_wait_at_the_head_is_failed, a check ended",
            with_checks(failed_review, vec![passed(24)]),
            in_lane("fix", 26),
        ),
        differs(
            "a_wait_not_run_at_head_is_ready",
            opened(),
            in_lane("review", 20),
            waiting(&[on_check("h1")]),
            "D2",
        ),
        same(
            "a_wait_not_run_at_head_is_ready, a check ended",
            with_checks(opened(), vec![passed(24)]),
            in_lane("review", 20),
        ),
        same(
            "a_wait_not_run_at_head_is_ready, a run at an older head, a check ended",
            with_checks(ran_at_old_head, vec![passed(24)]),
            in_lane("review", 20),
        ),
        same(
            "a_wait_run_at_head_is_waiting",
            waited_review.clone(),
            waiting(&[on_check("h1")]),
        ),
        same(
            "a_wait_run_at_head_is_waiting, a check ended",
            with_checks(waited_review, vec![passed(24)]),
            waiting(&[on_check("h1")]),
        ),
        same(
            "a_live_process_is_running",
            with_run(facts(old_list()), run("implement", None, 5).lives()),
            Place::Running(Run {
                phase: name("implement"),
                head: None,
                started_at: time(5),
            }),
        ),
        differs(
            "an_exit_zero_ends_the_phase, after the failed run",
            with_run(facts(old_list()), run("implement", None, 5).exits(1, 10)),
            in_lane("implement", 1),
            in_lane("implement", 10),
            "D4",
        ),
        same(
            "an_exit_zero_ends_the_phase, after the run that exited 0",
            with_run(
                with_run(facts(old_list()), run("implement", None, 5).exits(1, 10)),
                run("implement", None, 11).exits(0, 15),
            ),
            in_lane("open-pr", 15),
        ),
        same(
            "a_dead_run_without_exit_does_not_end_it",
            with_run(facts(old_list()), run("implement", None, 5)),
            in_lane("implement", 1),
        ),
        // The gather reads an exit that is no number as no exit, so the core sees the row above.
        same(
            "an_exit_that_is_no_number_is_no_exit",
            with_run(facts(old_list()), run("implement", None, 5)),
            in_lane("implement", 1),
        ),
        // The same answer for a new reason: the run left no head, and the head is h1 (D1).
        same(
            "a_run_before_the_previous_fact_does_not_end_the_phase",
            polish,
            in_lane("polish", 20),
        ),
        differs(
            "a_new_head_reopens_the_review, at h1",
            reviewed(),
            in_lane("ci", 30),
            waiting(&[on_check("h1")]),
            "D2, D3",
        ),
        differs(
            "a_new_head_reopens_the_review, at h2",
            at_head(reviewed(), "h2"),
            in_lane("review", 20),
            waiting(&[on_check("h2")]),
            "D2",
        ),
        same(
            "a_new_head_reopens_the_review, at h2, a check ended",
            with_checks(at_head(reviewed(), "h2"), vec![passed(50)]),
            in_lane("review", 20),
        ),
        differs(
            "a_review_with_no_head_is_not_at_the_head",
            with_reviews(opened(), vec![review(None, 30)]),
            in_lane("review", 20),
            waiting(&[on_check("h1")]),
            "D2",
        ),
        same(
            "a_review_with_no_head_is_not_at_the_head, a check ended",
            with_checks(
                with_reviews(opened(), vec![review(None, 30)]),
                vec![passed(24)],
            ),
            in_lane("review", 20),
        ),
        same("a_merged_pr_is_done", merged, Place::Done),
        // The fold of adjacent model actions is the gather's; the core gets one phase of two ends.
        same(
            "adjacent_model_phases_are_one_run, before",
            folded(),
            in_lane("implement", 1),
        ),
        same(
            "adjacent_model_phases_are_one_run, after",
            with_run(folded(), run("implement", None, 5).exits(0, 10)),
            in_lane("open-pr", 10),
        ),
        differs(
            "a_wait_between_model_phases_splits_the_run",
            split(run("implement", None, 5).exits(0, 10)),
            in_lane("review", 10),
            in_lane("implement", 1),
            "D1",
        ),
        same(
            "a_wait_between_model_phases_splits_the_run, the run left h1",
            split(run("implement", None, 5).exits(0, 10).leaves(Some("h1"))),
            in_lane("review", 10),
        ),
        // The gather hands the core this ticket's runs alone.
        same(
            "another_tasks_run_is_not_this_ones",
            facts(old_list()),
            in_lane("implement", 1),
        ),
        same(
            "a_task_with_nothing_is_ready_for_its_first_phase",
            facts(old_list()),
            in_lane("implement", 1),
        ),
        // Implement is listed before the `pr-open` phase, so its `exit` is not keyed on the head.
        same(
            "the old list with a pull request keeps implement ended",
            old_opened,
            in_lane("merge", 40),
        ),
        differs(
            "D5: an empty pull request does not end implement",
            {
                let mut facts = facts(pack());
                facts.pull = Some(PullFacts {
                    changed_file: None,
                    checks: vec![passed(12)],
                    ..pull("h1", 8)
                });
                with_run(
                    facts,
                    run("implement", None, 5).exits(0, 10).leaves(Some("h1")),
                )
            },
            in_lane("review", 12),
            in_lane("implement", 10),
            "D5",
        ),
        differs(
            "D6: a review that raised an open question is not done",
            {
                let mut facts = facts(pack());
                facts.pull = Some(PullFacts {
                    checks: vec![passed(12)],
                    ..pull("h1", 8)
                });
                facts.open_questions.insert(id(900));
                with_run(
                    facts,
                    run("review", Some("h1"), 13)
                        .exits(0, 14)
                        .leaves(Some("h1"))
                        .asks(900),
                )
            },
            in_lane("merge", 14),
            waiting(&[WaitingOn::Question(id(900))]),
            "D6",
        ),
        differs(
            "D6: a review whose question was ruled is not done either: it failed, and fix is next",
            {
                let mut facts = facts(pack());
                facts.pull = Some(PullFacts {
                    checks: vec![passed(12)],
                    ..pull("h1", 8)
                });
                with_run(
                    facts,
                    run("review", Some("h1"), 13)
                        .exits(0, 14)
                        .leaves(Some("h1"))
                        .asks(900),
                )
            },
            in_lane("merge", 14),
            in_lane("fix", 14),
            "D6",
        ),
        differs(
            "D7: a wait that died with no exit code runs again",
            with_checks(
                with_run(opened(), run("review", Some("h1"), 25)),
                vec![passed(24)],
            ),
            waiting(&[on_check("h1")]),
            in_lane("review", 20),
            "D7",
        ),
    ]
}

#[test]
fn the_core_answers_as_todays_rule_table_but_for_the_listed_differences() {
    let mut wrong = Vec::new();
    for row in parity_rows() {
        let placed = place(&row.facts, now());
        if placed != row.core {
            wrong.push(format!(
                "{}: the core placed {placed:?}, the row says {:?}",
                row.case, row.core
            ));
        }
        match row.difference {
            None if row.today != row.core => wrong.push(format!(
                "{}: no difference is listed but today's {:?} is not {:?}",
                row.case, row.today, row.core
            )),
            Some(difference) if row.today == row.core => wrong.push(format!(
                "{}: {difference} is listed but nothing differs",
                row.case
            )),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// D8: today's count of failures (#550) is the unbroken tail of non-zero exits; getting nowhere
/// counts every ended run of the action at an unmoved head, exit 0 included.
#[test]
fn exit_zero_runs_at_an_unmoved_head_count_toward_getting_nowhere() {
    let mut facts = facts(pack());
    facts.pull = Some(PullFacts {
        checks: vec![failed(30)],
        ..pull("h1", 20)
    });
    for (started, code) in [(31, 0), (41, 0), (51, 0)] {
        facts.runs.push(
            run("fix", Some("h1"), started)
                .exits(code, started + 5)
                .leaves(Some("h1")),
        );
    }
    let todays_streak = facts
        .runs
        .iter()
        .rev()
        .take_while(|run| run.exit.is_some_and(|exit| !exit.code.is_zero()))
        .count();
    assert_eq!(todays_streak, 0);
    assert_eq!(nowhere(&facts, &name("fix")).len(), 3);
    assert!(gets_nowhere(&facts, &name("fix")));
}

// ---------------------------------------------------------------------------------------------
// The eight real failures of 2026-11-12, each as a pure precursor.
// ---------------------------------------------------------------------------------------------

/// Failure 1: a `blocked` label nothing cleared held a ticket. The facts carry no state label,
/// so a done dependency leaves no ticket waiting on it, and the label written for the ticket is
/// never `blocked`, claimed or not.
#[test]
fn failure_1_a_done_dependency_leaves_no_ticket_waiting_whatever_its_label() {
    let mut facts = facts(pack());
    facts.dependencies.push(Dependency {
        ticket: id(25),
        standing: Standing::Done,
    });
    let placed = place(&facts, now());
    assert_eq!(placed, in_lane("implement", 1));
    for claimed in [false, true] {
        assert_ne!(label_for(&placed, claimed), TicketState::Blocked);
    }
    let placement = Placement::of(std::slice::from_ref(&facts), now());
    assert!(!placement.waiting.contains_key(&WaitingOn::Ticket(id(25))));
    for standing in [
        Standing::Open,
        Standing::ClosedNotPlanned,
        Standing::Missing,
    ] {
        facts.dependencies[0].standing = standing;
        assert_eq!(
            place(&facts, now()),
            waiting(&[WaitingOn::Ticket(id(25))]),
            "{standing:?}"
        );
    }
}

/// The scene of failures 2 and 3: implement asked question #900, exited 0 and left an empty pull
/// request at `h1`, whose check passed.
fn asked_and_exited_zero() -> TicketFacts {
    let mut facts = facts(pack());
    facts.pull = Some(PullFacts {
        changed_file: None,
        checks: vec![passed(12)],
        ..pull("h1", 8)
    });
    facts.runs.push(
        run("implement", None, 5)
            .exits(0, 10)
            .leaves(Some("h1"))
            .asks(900),
    );
    facts
}

/// Failure 2: a script's exit 0 was read as done. A programmer that asked a question and exited
/// 0 with an empty pull request is not done: the ticket waits on the question.
#[test]
fn failure_2_a_question_and_exit_zero_with_an_empty_pull_request_is_not_done() {
    let mut facts = asked_and_exited_zero();
    facts.open_questions.insert(id(900));
    let implement = &facts.phases[0];
    assert!(!end_fact_holds(implement, &facts));
    let report = facts.runs[0].report();
    assert_eq!(
        outcome(&report, implement, &facts),
        Outcome::WaitOn(WaitingOn::Question(id(900)))
    );
    assert_eq!(
        place(&facts, now()),
        waiting(&[WaitingOn::Question(id(900))])
    );
    // Without the question an exit 0 on an empty pull request still is not done.
    facts.runs[0].questions.clear();
    facts.open_questions.clear();
    let report = facts.runs[0].report();
    assert!(matches!(
        outcome(&report, &facts.phases[0], &facts),
        Outcome::Failed(_)
    ));
}

/// Failure 3: once the question was ruled, implement never ran again. It is the next run again.
#[test]
fn failure_3_implement_is_the_next_run_again_once_its_question_is_ruled() {
    let facts = asked_and_exited_zero();
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.phase.name, name("implement"));
    assert_eq!(current.next(), &name("implement"));
    assert_eq!(place(&facts, now()), in_lane("implement", 10));
}

/// Failure 4: a fix that changed nothing ran again and again, and done, failed, done, failed
/// never reached three failures in a row. Both are getting nowhere at one head.
#[test]
fn failure_4_a_fix_that_changes_nothing_three_times_at_one_head_gets_nowhere() {
    let red = || {
        let mut facts = facts(pack());
        facts.pull = Some(PullFacts {
            checks: vec![failed(30)],
            ..pull("h1", 20)
        });
        facts
    };
    let fix_at_h1 = |started: u64, code: i32| {
        run("fix", Some("h1"), started)
            .exits(code, started + 5)
            .leaves(Some("h1"))
    };
    let mut unchanged = red();
    assert_eq!(place(&unchanged, now()), in_lane("fix", 30));
    for started in [31, 41] {
        unchanged.runs.push(fix_at_h1(started, 0));
        assert!(!gets_nowhere(&unchanged, &name("fix")));
    }
    unchanged.runs.push(fix_at_h1(51, 0));
    assert_eq!(nowhere(&unchanged, &name("fix")).len(), NOWHERE_LIMIT);
    assert!(gets_nowhere(&unchanged, &name("fix")));

    let mut alternating = red();
    for (started, code) in [(31, 0), (41, 1), (51, 0), (61, 1)] {
        alternating.runs.push(fix_at_h1(started, code));
    }
    assert!(gets_nowhere(&alternating, &name("fix")));

    // A fix that moved the head is not getting nowhere at the new one.
    let mut moved = red();
    moved.runs.push(fix_at_h1(31, 0));
    moved.runs.push(fix_at_h1(41, 0));
    moved
        .runs
        .push(run("fix", Some("h1"), 51).exits(0, 56).leaves(Some("h2")));
    let moved = at_head(moved, "h2");
    assert!(nowhere(&moved, &name("fix")).is_empty());
}

/// Failure 5: review ran before any check had registered at a new head. With no check at the
/// head the ticket waits on the check, and review is not next.
#[test]
fn failure_5_no_check_at_a_new_head_waits_on_the_check_and_review_is_not_next() {
    let mut facts = facts(pack());
    facts.pull = Some(pull("h2", 20));
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1")),
    );
    facts
        .runs
        .push(run("fix", Some("h1"), 30).exits(0, 40).leaves(Some("h2")));
    assert_eq!(place(&facts, now()), waiting(&[on_check("h2")]));
    let placement = Placement::of(std::slice::from_ref(&facts), now());
    assert!(placement.lane.is_empty());
    // A check that registered but is pending holds it too.
    let pull = facts.pull.as_mut().expect("a pull request");
    pull.checks = vec![pending()];
    assert_eq!(place(&facts, now()), waiting(&[on_check("h2")]));
    // Once it passed, review runs at h2: the pass at h1 does not hold there.
    let pull = facts.pull.as_mut().expect("a pull request");
    pull.checks = vec![passed(50)];
    assert_eq!(place(&facts, now()), in_lane("review", 50));
}

/// Failure 6: an orphaned process held a slot. A run whose process is dead is not running, and
/// the placement's running set, which slots are counted from, does not hold it.
#[test]
fn failure_6_a_dead_process_is_not_running_and_holds_no_slot() {
    let live = with_run(facts(pack()), run("implement", None, 5).lives());
    let placement = Placement::of(std::slice::from_ref(&live), now());
    assert!(placement.running.contains_key(&ticket()));

    let dead = with_run(facts(pack()), run("implement", None, 5));
    assert_eq!(place(&dead, now()), in_lane("implement", 1));
    let placement = Placement::of(std::slice::from_ref(&dead), now());
    assert!(placement.running.is_empty());

    // A recorded exit ends a run whatever the process table says.
    let ended = with_run(facts(pack()), run("implement", None, 5).lives().exits(1, 9));
    assert!(!matches!(place(&ended, now()), Place::Running(_)));
}

// Failure 7, a watch that ran an old binary after `zaf update`, is the scheduler process's (slice
// 7, tests/spawn_watch.rs a_watch_restarts_as_the_installed_binary): a pure core has no binary,
// no process and no restart, so it has no precursor here.

/// Failure 8: a started ticket kept going while its epic was not ready. A ticket waits on an
/// unsigned, unchallenged or questioned epic, started or not: placement reads no claim.
#[test]
fn failure_8_a_ticket_waits_on_an_unsigned_unchallenged_or_questioned_epic_started_or_not() {
    let epic = id(1);
    let not_started = facts(pack());
    let started = {
        let mut facts = facts(pack());
        facts.pull = Some(PullFacts {
            checks: vec![passed(12)],
            ..pull("h1", 8)
        });
        facts
            .runs
            .push(run("implement", None, 5).exits(0, 10).leaves(Some("h1")));
        facts
    };
    for base in [not_started, started] {
        for (signed, challenged) in [(false, true), (true, false), (false, false)] {
            let mut facts = base.clone();
            facts.epic = Some(EpicFacts {
                id: epic,
                signed,
                challenged,
            });
            assert_eq!(
                place(&facts, now()),
                waiting(&[WaitingOn::Epic(epic)]),
                "signed {signed}, challenged {challenged}"
            );
        }
        let mut questioned = base.clone();
        questioned.epic = Some(EpicFacts {
            id: epic,
            signed: true,
            challenged: true,
        });
        questioned.open_questions.insert(id(30));
        assert_eq!(
            place(&questioned, now()),
            waiting(&[WaitingOn::Question(id(30))])
        );
        questioned.open_questions.clear();
        assert!(matches!(place(&questioned, now()), Place::InLane { .. }));
    }
}
