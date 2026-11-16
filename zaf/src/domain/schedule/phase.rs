//! A ticket's phases, the fact that ends each, and which one is current (R12, R14).
//!
//! The current phase is never stored: it is the first phase whose end fact does not hold at the
//! pull request's head ([`current_phase`]), so a push after the review opens the review again. An
//! `exit` phase listed at or before the one that opens the pull request is not keyed on the head
//! ([`end_fact_holds`]): a fix pushed later does not open the brief or the implement again. Before
//! the pull request or after it, a run that raised a question is never done by its exit code.
//! A [`Phase`] has no kind (R8): what it costs and how it launches is the caller's, and the core
//! reads only its name, its end facts and its `on_fail`. A failed phase runs its `on_fail` only
//! for what the `on_fail` answers ([`Failure::for_on_fail`]), and a ruled question its run raised
//! reaches the `on_fail` once before the phase applies again ([`phase_status`]).

use std::time::SystemTime;

use super::facts::{Report, RunRecord, TicketFacts};
use super::outcome::{Failure, Outcome, is_machinery, outcome};
use super::place::WaitingOn;
use crate::newtype::text_newtype;

text_newtype! {
    /// The name of an action: an entry of a task type's list under `[phases]`, or the one an
    /// action's `on-fail` names. A phase is named by its first action.
    ActionName
}

/// The fact that ends a phase: the value of `# zaf ends: pr-open | review-posted | ci-done |
/// merged | exit` (docs/zaf-actions.md §5.1). The first four are answered by the forge; `exit` by
/// the phase's own run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum PhaseEnd {
    /// The task's pull request is open and changes at least one file.
    PrOpen,
    /// A review is posted at the pull request's head.
    ReviewPosted,
    /// The pull request's checks at its head all passed.
    CiDone,
    /// The pull request is merged.
    Merged,
    /// A run of the phase exited 0 and raised no question at all; after the `pr-open` phase, a
    /// run that left the head (see [`end_fact_holds`]).
    Exit,
}

impl PhaseEnd {
    /// Whether something outside the phase's run brings it later: a review, the checks, a merge.
    /// `exit` is the run's own; `pr-open` is too, since the phase that ends at it opens the pull
    /// request itself, so a run that exited 0 without one failed rather than awaits.
    #[must_use]
    pub const fn comes_from_outside(self) -> bool {
        matches!(self, Self::ReviewPosted | Self::CiDone | Self::Merged)
    }
}

/// One step of a ticket: a single action, or adjacent model actions the caller folded into one
/// run, named by the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    /// Its name: the action that runs it, the first of a fold.
    pub name: ActionName,
    /// The facts that end it, one per folded action; it ends when every one holds. A phase with
    /// none never ends, so it stays current where it can be seen rather than being passed by
    /// (the caller never builds one: every listed action declares an end).
    pub ends: Vec<PhaseEnd>,
    /// The phase to run when this one fails (a red check runs `fix`); none runs it again.
    pub on_fail: Option<ActionName>,
}

impl Phase {
    /// Whether it waits on the outside: one of its ends comes from outside its run
    /// ([`PhaseEnd::comes_from_outside`]). Such a phase, once its run exited 0, awaits rather
    /// than fails. This reads the end facts, never a kind (R8).
    #[must_use]
    pub fn awaits_the_outside(&self) -> bool {
        self.ends.iter().any(|end| end.comes_from_outside())
    }
}

/// Where a phase stands at the pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseStatus {
    /// Its end fact holds.
    Done,
    /// It has no counted run at the head and nothing failed there: it may run.
    Open,
    /// Its latest counted run at the head waits on something outside: a check, a review, a
    /// question (the plan's `Awaiting`, carrying what it awaits).
    Awaiting(WaitingOn),
    /// Its latest counted run at the head failed, or a check failed at the head.
    Failed(Failure),
}

/// The current phase of a ticket, and what the next run is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentPhase<'a> {
    /// The phase: the first whose end fact does not hold.
    pub phase: &'a Phase,
    /// Where it stands.
    pub status: PhaseStatus,
    /// When the phase before it ended; none for the first phase.
    pub previous: Option<SystemTime>,
}

impl CurrentPhase<'_> {
    /// The next run: the current phase, or its `on_fail` when it has failed on something the
    /// `on_fail` answers ([`Failure::for_on_fail`]) and names one. A failure no `on_fail` answers
    /// (a backend, a refusal, a base merged and pushed) runs the phase again.
    #[must_use]
    pub fn next(&self) -> &ActionName {
        match (&self.status, &self.phase.on_fail) {
            (PhaseStatus::Failed(failure), Some(on_fail)) if failure.for_on_fail() => on_fail,
            _ => &self.phase.name,
        }
    }
}

/// When `end` came to hold for `phase` at the pull request's current head; none when it does
/// not hold. `report`, when given, is read as one more run of the phase (a run just ended whose
/// folder the facts may not hold yet).
fn end_time(
    end: PhaseEnd,
    phase: &Phase,
    facts: &TicketFacts,
    report: Option<&Report>,
) -> Option<SystemTime> {
    let pull = facts.pull.as_ref();
    let head = facts.head();
    match end {
        PhaseEnd::PrOpen => pull
            .filter(|pull| pull.changes_a_file && !last_run_asked(phase, facts, report))
            .map(|pull| pull.opened_at),
        PhaseEnd::Merged => pull?.merged_at,
        PhaseEnd::ReviewPosted => {
            let head = head?;
            pull?
                .reviews
                .iter()
                .filter(|review| review.head.as_ref() == Some(head))
                .map(|review| review.posted_at)
                .min()
        }
        PhaseEnd::CiDone => pull?.checks_passed_at(),
        PhaseEnd::Exit => {
            let keyed = exit_keyed_on_head(phase, facts);
            let recorded = facts
                .runs
                .iter()
                .filter(|run| run.action == phase.name)
                .map(|run| (run.left(), run.exit, &run.questions));
            let reported =
                report.map(|report| (report.head.as_ref(), report.exit, &report.questions));
            recorded
                .chain(reported)
                .filter(|(left, _, questions)| questions.is_empty() && (!keyed || *left == head))
                .filter_map(|(_, exit, _)| exit.filter(|exit| exit.code.is_zero()))
                .map(|exit| exit.at)
                .min()
        }
    }
}

/// Whether `phase`'s `exit` is keyed on the head: it is listed after the phase that carries
/// `pr-open`, so a push since its run opens it again. A phase listed at or before that one ran
/// before the pull request had a head to key on (a brief, the implement that opens it), and a
/// push later on does not undo it. A list with no `pr-open` phase keys every phase, and so does a
/// phase the list does not name.
fn exit_keyed_on_head(phase: &Phase, facts: &TicketFacts) -> bool {
    let Some(opens) = facts
        .phases
        .iter()
        .position(|listed| listed.ends.contains(&PhaseEnd::PrOpen))
    else {
        return true;
    };
    facts
        .phases
        .iter()
        .position(|listed| listed.name == phase.name)
        .is_none_or(|at| at > opens)
}

/// When `phase` ended at the head: when the last of its end facts came to hold; none while one
/// does not, and none for a phase with no end.
pub(super) fn ended_at(
    phase: &Phase,
    facts: &TicketFacts,
    report: Option<&Report>,
) -> Option<SystemTime> {
    let mut last = None;
    for end in &phase.ends {
        let at = end_time(*end, phase, facts, report)?;
        last = last.max(Some(at));
    }
    last
}

/// Whether every end fact of `phase` holds at the pull request's current head:
///
/// - `pr-open`: a pull request is recorded and changes at least one file.
/// - `review-posted`: a review was made on the head.
/// - `ci-done`: at least one check at the head, every one passed.
/// - `merged`: the pull request is merged.
/// - `exit`, for a phase listed after the one that carries `pr-open`: a run of the phase left the
///   head (its recorded end head, or its start head for a folder that recorded none) with exit 0
///   and raised no question. With no pull request the head is none, and a run that left none holds
///   it. A list with no `pr-open` phase reads every phase so.
/// - `exit`, for a phase listed at or before the one that carries `pr-open`: a run of the phase
///   exited 0 and raised no question, whatever head it left: it ran before there was a head, so a
///   push after it does not reopen it.
///
/// Either way a run that raised a question is never done by its exit code, open or ruled: once
/// the question is ruled the phase runs again, with the ruling to go on.
#[must_use]
pub fn end_fact_holds(phase: &Phase, facts: &TicketFacts) -> bool {
    ended_at(phase, facts, None).is_some()
}

/// Where `phase` stands at the pull request's current head.
///
/// [`PhaseStatus::Done`] when its end fact holds. Otherwise the latest counted run of it at the
/// head decides, by its [`outcome()`]: a counted run is one that ended, not by the machinery
/// ([`is_machinery`]), and left the head ([`RunRecord::left`](super::RunRecord::left)). A run
/// that waits on something is [`PhaseStatus::Awaiting`], one that failed is
/// [`PhaseStatus::Failed`]. With no counted run, a check that failed at the head fails the phase,
/// and otherwise it is [`PhaseStatus::Open`]. A run that died, was stopped or never ran its body
/// is not counted, so the phase runs again.
///
/// A failure its `on_fail` has answered is [`PhaseStatus::Open`] again: the latest counted run
/// raised a question, the question is ruled, and a run of the `on_fail` began and ended at the
/// head after it, exited 0 and raised none ([`answered`]). The ruling reached a model once, which
/// found nothing to change, so the phase applies again rather than the `on_fail` running until it
/// gets nowhere.
#[must_use]
pub fn phase_status(phase: &Phase, facts: &TicketFacts) -> PhaseStatus {
    if end_fact_holds(phase, facts) {
        return PhaseStatus::Done;
    }
    if let Some(run) = latest_counted_run(phase, facts) {
        return match outcome(&run.report(), phase, facts) {
            Outcome::Done => PhaseStatus::Done,
            Outcome::WaitOn(waiting_on) => PhaseStatus::Awaiting(waiting_on),
            Outcome::Failed(_) if answered(run, phase, facts) => PhaseStatus::Open,
            Outcome::Failed(failure) => PhaseStatus::Failed(failure),
        };
    }
    let head = facts.head();
    match facts.pull.as_ref().and_then(|pull| pull.first_failure_at()) {
        Some(at) => PhaseStatus::Failed(Failure {
            head: head.cloned(),
            checks: facts
                .pull
                .iter()
                .flat_map(|pull| pull.failed_checks())
                .collect(),
            exit: None,
            at: Some(at),
        }),
        None => PhaseStatus::Open,
    }
}

/// Whether the last run of `phase` raised a question: `report` when given (a run just ended), else
/// the latest recorded run that ended, not by the machinery ([`is_machinery`]), at whatever head.
/// A run that raised a question does not end its phase, whatever its end fact (the Engineer,
/// 2026-11-15): what it pushed before it asked is a stand-in, and the phase runs again once the
/// question is ruled. With no run on record nothing asked, so a pull request opened by hand counts.
fn last_run_asked(phase: &Phase, facts: &TicketFacts, report: Option<&Report>) -> bool {
    if let Some(report) = report {
        return !report.questions.is_empty();
    }
    facts
        .runs
        .iter()
        .filter(|run| run.action == phase.name && !run.live() && run.exit.is_some())
        .filter(|run| !is_machinery(run.exit.map(|exit| exit.code)))
        .max_by_key(|run| run.started_at)
        .is_some_and(|run| !run.questions.is_empty())
}

/// The latest counted run of `phase` at the pull request's current head: one that ended, not by
/// the machinery ([`is_machinery`]), and left the head ([`RunRecord::left`]). None when there is
/// none; [`phase_status`] then reads the checks alone.
#[must_use]
pub fn latest_counted_run<'a>(phase: &Phase, facts: &'a TicketFacts) -> Option<&'a RunRecord> {
    let head = facts.head();
    facts
        .runs
        .iter()
        .filter(|run| run.action == phase.name && !run.live() && run.left() == head)
        .filter(|run| !is_machinery(run.exit.map(|exit| exit.code)))
        .max_by_key(|run| run.started_at)
}

/// Whether `phase`'s `on_fail` has answered the failure `run` left: `run` raised a question (ruled
/// by now, or the run would wait on it), and a run of the `on_fail` began and ended at the current
/// head, started once `run` had ended, exited 0 and raised no question. A failure with no question
/// is never answered so: its `on_fail` runs until the head moves, or it gets nowhere.
fn answered(run: &RunRecord, phase: &Phase, facts: &TicketFacts) -> bool {
    let Some(on_fail) = phase.on_fail.as_ref() else {
        return false;
    };
    if run.questions.is_empty() {
        return false;
    }
    let head = facts.head();
    let ended = run.exit.map_or(run.started_at, |exit| exit.at);
    facts.runs.iter().any(|later| {
        &later.action == on_fail
            && !later.live()
            && later.stayed_at(head)
            && later.questions.is_empty()
            && later.exit.is_some_and(|exit| exit.code.is_zero())
            && later.started_at > run.started_at
            && later.started_at >= ended
    })
}

/// The ticket's current phase: the first of its phases whose end fact does not hold at the pull
/// request's head, with its status and when the phase before it ended. None when every phase
/// ended, an empty list included (the caller passes a ticket with no list over, as today).
#[must_use]
pub fn current_phase(facts: &TicketFacts) -> Option<CurrentPhase<'_>> {
    let mut previous = None;
    for phase in &facts.phases {
        match ended_at(phase, facts, None) {
            Some(at) => previous = Some(at),
            None => {
                return Some(CurrentPhase {
                    phase,
                    status: phase_status(phase, facts),
                    previous,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
