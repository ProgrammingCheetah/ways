//! What a run meant, worked out from its report and the facts, never taken from the phase (R13),
//! and when a phase is getting nowhere (R18).
//!
//! [`outcome()`] reads, in this order: a question the run raised that is still open is a wait on
//! it, whatever the exit code; the phase's end fact holding at the current head is done;
//! something outside that has not ended is a wait on the check; anything else failed.
//!
//! [`nowhere`] counts what getting nowhere means: the ended runs of the action about to start
//! that began and ended at the current head, after the last ruled marker. Exit 0 counts like any
//! other code; a failure of the machinery ([`is_machinery`]) never counts.

use std::time::SystemTime;

use super::facts::{CheckName, ExitCode, Report, RunRecord, TicketFacts};
use super::phase::{ActionName, Phase, ended_at};
use super::place::WaitingOn;
use crate::domain::CommitId;

/// How many counted runs of one action at one head make it getting nowhere: three (R18).
pub const NOWHERE_LIMIT: usize = 3;

/// The first exit code a signal leaves (128 plus the signal), as `zaf spawn stop` does.
const SIGNALLED_FROM: i32 = 128;

/// What the lane works out from a run's [`Report`] and the facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The phase ended: the ticket goes on to its next phase, or is out for good with none left.
    Done,
    /// The ticket waits, out of the lane, on this.
    WaitOn(WaitingOn),
    /// The phase failed: its `on_fail` runs next, or it runs again.
    Failed(Failure),
}

/// What a failed phase failed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The pull request's head it failed at; none without one.
    pub head: Option<CommitId>,
    /// The checks that failed at the head, in the forge's order.
    pub checks: Vec<CheckName>,
    /// The run's exit code; none when it failed on checks alone, or recorded no code.
    pub exit: Option<ExitCode>,
    /// When it failed: the earlier of the run's end and the first failed check; none when
    /// neither carries a time.
    pub at: Option<SystemTime>,
}

impl Failure {
    /// Whether it is a failure of the machinery alone: no check failed and the run's exit is one
    /// [`is_machinery`] names. Such a failure never counts; the caller tries again later.
    #[must_use]
    pub fn by_machinery(&self) -> bool {
        self.checks.is_empty() && is_machinery(self.exit)
    }

    /// Whether it is one a phase's `on-fail` answers: a check failed at the head, the run exited
    /// [`ExitCode::FAILURE`] (the canonical failure: a must-fix, a conflict), or it exited 0 and
    /// still failed, which a counted run does when it raised a question that is now ruled. Any
    /// other code is the phase's own (a backend that failed, a refusal no change can answer, a
    /// base merged and pushed): the phase runs again rather than its `on-fail`, and a wait asks.
    #[must_use]
    pub fn for_on_fail(&self) -> bool {
        !self.checks.is_empty()
            || self
                .exit
                .is_some_and(|exit| exit.is_zero() || exit == ExitCode::FAILURE)
    }
}

/// Whether a run's exit is a failure of the machinery, which never counts toward getting
/// nowhere: no exit recorded, `125` (the body never ran), or `128` and above (a signal, as
/// `zaf spawn stop` leaves). Every other code counts, negative ones included.
#[must_use]
pub fn is_machinery(exit: Option<ExitCode>) -> bool {
    exit.is_none_or(|code| code == ExitCode::NOT_RUN || code.get() >= SIGNALLED_FROM)
}

/// What the run that left `report` meant for `phase`, from the report and `facts`, in order:
///
/// 1. A question the run raised that is still open: wait on it. An exit 0 changes nothing.
/// 2. The phase's end fact holds at the current head, the report read as one more run of the
///    phase: done.
/// 3. Something outside has not ended: the ticket's checks at the head are awaited
///    ([`TicketFacts::awaited_checks`]), or the run exited 0, raised no question, the phase
///    awaits the outside ([`Phase::awaits_the_outside`]), the pull request has a head and no
///    check failed there. Wait on the check at the head.
/// 4. Anything else: failed, on the checks that failed at the head and the run's exit.
///
/// The phase's own verdict is never read: there is none.
#[must_use]
pub fn outcome(report: &Report, phase: &Phase, facts: &TicketFacts) -> Outcome {
    if let Some(question) = report
        .questions
        .iter()
        .find(|question| facts.open_questions.contains(question))
    {
        return Outcome::WaitOn(WaitingOn::Question(*question));
    }
    if ended_at(phase, facts, Some(report)).is_some() {
        return Outcome::Done;
    }
    if let Some(head) = facts.awaited_checks() {
        return Outcome::WaitOn(WaitingOn::Check(facts.ticket, head.clone()));
    }
    let pull = facts.pull.as_ref();
    let checks = pull.map(|pull| pull.failed_checks()).unwrap_or_default();
    let exited_zero = report.exit.is_some_and(|exit| exit.code.is_zero());
    let awaits_the_outside = exited_zero
        && report.questions.is_empty()
        && phase.awaits_the_outside()
        && checks.is_empty();
    if awaits_the_outside && let Some(head) = facts.head() {
        return Outcome::WaitOn(WaitingOn::Check(facts.ticket, head.clone()));
    }
    let at = report
        .exit
        .map(|exit| exit.at)
        .into_iter()
        .chain(pull.and_then(|pull| pull.first_failure_at()))
        .min();
    Outcome::Failed(Failure {
        head: facts.head().cloned(),
        checks,
        exit: report.exit.map(|exit| exit.code),
        at,
    })
}

/// The runs that make `action` getting nowhere, oldest first: its runs that ended, not by the
/// machinery, began and ended at the pull request's current head (none when there is no pull
/// request head) and started after the last ruled marker for it. "At the head" is at the head's
/// tree ([`RunRecord::stayed_at_tree`]), so an empty commit cannot dodge the count; a head whose
/// tree was not read counts by its commit id. A run whose end head is unknown never counts. It is getting nowhere once there are [`NOWHERE_LIMIT`] of them; done, failed,
/// done, failed at one head counts four.
#[must_use]
pub fn nowhere<'a>(facts: &'a TicketFacts, action: &ActionName) -> Vec<&'a RunRecord> {
    let head = facts.head();
    let after = facts.ruled_through(action);
    let mut runs: Vec<&RunRecord> = facts
        .runs
        .iter()
        .filter(|run| {
            &run.action == action && !run.live() && run.stayed_at_tree(head, &facts.trees)
        })
        .filter(|run| !is_machinery(run.exit.map(|exit| exit.code)))
        .filter(|run| after.is_none_or(|after| run.started_at > after))
        .collect();
    runs.sort_by_key(|run| run.started_at);
    runs
}

/// Whether `action` is getting nowhere: [`nowhere`] holds [`NOWHERE_LIMIT`] runs or more.
#[must_use]
pub fn gets_nowhere(facts: &TicketFacts, action: &ActionName) -> bool {
    nowhere(facts, action).len() >= NOWHERE_LIMIT
}

#[cfg(test)]
mod tests;
