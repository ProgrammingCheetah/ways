//! What the core reads about one ticket: [`TicketFacts`], gathered by the caller from the board,
//! the forge and the phase run folders.
//!
//! Every field is a fact, never a state label: nothing here can say `blocked` or `question`, so a
//! label nothing clears can never hold a ticket (R3, R5). A [`RunRecord`] is one phase run folder
//! read; its [`Report`] is what the run left behind, the facts a phase hands up (R13).

use std::collections::BTreeSet;
use std::time::SystemTime;

use super::lane::Priority;
use super::phase::{ActionName, Phase, PhaseEnd};
use crate::domain::{CommitId, Standing, TaskSize, TicketId};
use crate::newtype::text_newtype;

text_newtype! {
    /// The name of a check run (`ci`, a scanner, a bot).
    CheckName
}

text_newtype! {
    /// The last non-empty line a phase run printed, kept for the question that names the run.
    OutputLine
}

/// The exit code a phase run's launcher recorded.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::From,
    derive_more::Display,
)]
pub struct ExitCode(i32);

impl ExitCode {
    /// `125`, which zaf keeps for itself: the action's body never ran (`NOT_RUN`).
    pub const NOT_RUN: Self = Self(125);

    /// `1`, the canonical failure every action declares: a must-fix, a red check, a conflict.
    /// It is the code a phase's `on-fail` answers ([`super::Failure::for_on_fail`]).
    pub const FAILURE: Self = Self(1);

    /// The bare code.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }

    /// Whether it is 0. A fact, not a verdict: exit 0 after a question is not done.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// How a run's process ended: the code, and when it was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunExit {
    /// The exit code.
    pub code: ExitCode,
    /// When the code was recorded, which is when the run ended.
    pub at: SystemTime,
}

/// The head a run left, read after its process exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadEnd {
    /// No head was recorded: a folder from before the record kept one, or a head read that
    /// failed. It never counts toward getting nowhere ([`super::nowhere`]).
    Unknown,
    /// The pull request's head when the run ended; none when the ticket had no pull request, or
    /// one with no head.
    Left(Option<CommitId>),
}

/// One phase run folder of the ticket, read. The caller hands in only the ticket's own runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    /// The action it ran: a phase's name, or an `on_fail` phase's.
    pub action: ActionName,
    /// The pull request's head when it started; none without one.
    pub head: Option<CommitId>,
    /// The head it left.
    pub head_end: HeadEnd,
    /// When it started.
    pub started_at: SystemTime,
    /// Its process is alive (its recorded pid and start time), as the caller read the process
    /// table; liveness is computed where the process table is read, never here.
    pub alive: bool,
    /// How it ended; none while it runs, and none for a dead process that recorded no code.
    pub exit: Option<RunExit>,
    /// The questions it raised, by their issue numbers.
    pub questions: BTreeSet<TicketId>,
    /// The last non-empty line of its output; none when it printed none.
    pub last_line: Option<OutputLine>,
}

impl RunRecord {
    /// Whether it runs: its process is alive and no exit is recorded. A recorded exit ends a run
    /// whatever the process table says.
    #[must_use]
    pub fn live(&self) -> bool {
        self.alive && self.exit.is_none()
    }

    /// The head it left as the `exit` end fact reads it: the recorded one, or the head it started
    /// at for a folder that recorded none (the plan's fallback for old folders).
    #[must_use]
    pub fn left(&self) -> Option<&CommitId> {
        match &self.head_end {
            HeadEnd::Left(head) => head.as_ref(),
            HeadEnd::Unknown => self.head.as_ref(),
        }
    }

    /// Whether, by its record, it began and ended at `head`. An unknown end is never at a head.
    #[must_use]
    pub fn stayed_at(&self, head: Option<&CommitId>) -> bool {
        let ended_there = matches!(&self.head_end, HeadEnd::Left(left) if left.as_ref() == head);
        self.head.as_ref() == head && ended_there
    }

    /// What it left behind, as its [`Report`].
    #[must_use]
    pub fn report(&self) -> Report {
        Report {
            exit: self.exit,
            questions: self.questions.clone(),
            head: self.left().cloned(),
        }
    }
}

/// What a run left behind, facts only (R13): how it exited, the questions it raised and the head
/// it left. Never a verdict on itself; [`super::outcome()`] works out what it meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// How it exited; none when no exit was recorded.
    pub exit: Option<RunExit>,
    /// The questions it raised.
    pub questions: BTreeSet<TicketId>,
    /// The head it left (see [`RunRecord::left`]).
    pub head: Option<CommitId>,
}

/// How one check run at the pull request's head stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// Queued or running.
    Pending,
    /// Finished green, at this time.
    Passed(SystemTime),
    /// Finished red, at this time.
    Failed(SystemTime),
}

/// One check run at the pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckFact {
    /// Its name.
    pub name: CheckName,
    /// How it stands.
    pub result: CheckResult,
}

/// One review posted on the pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFact {
    /// The commit it was made on; none when the forge did not say, which is no head at all.
    pub head: Option<CommitId>,
    /// When it was posted.
    pub posted_at: SystemTime,
}

/// What the forge says of the ticket's pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullFacts {
    /// When it opened.
    pub opened_at: SystemTime,
    /// When it merged; none while it is not merged.
    pub merged_at: Option<SystemTime>,
    /// Its current head; none when the forge reports none.
    pub head: Option<CommitId>,
    /// Whether it changes at least one file (`Forge::last_changed_file` gives one).
    pub changes_a_file: bool,
    /// Its check runs at the current head, in the forge's order.
    pub checks: Vec<CheckFact>,
    /// Its reviews, at whatever head each was made.
    pub reviews: Vec<ReviewFact>,
}

impl PullFacts {
    /// Whether every check at the head has ended: there is at least one and none is pending. No
    /// check at all has not ended, since nothing says it will never come.
    #[must_use]
    pub fn checks_ended(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .iter()
                .all(|check| check.result != CheckResult::Pending)
    }

    /// When the checks at the head finished, if they all passed: at least one, none pending,
    /// none failed; the latest finish.
    #[must_use]
    pub fn checks_passed_at(&self) -> Option<SystemTime> {
        let mut latest = None;
        for check in &self.checks {
            match check.result {
                CheckResult::Passed(at) => latest = latest.max(Some(at)),
                CheckResult::Pending | CheckResult::Failed(_) => return None,
            }
        }
        latest
    }

    /// The checks at the head that failed, in the forge's order.
    #[must_use]
    pub fn failed_checks(&self) -> Vec<CheckName> {
        self.checks
            .iter()
            .filter(|check| matches!(check.result, CheckResult::Failed(_)))
            .map(|check| check.name.clone())
            .collect()
    }

    /// When the first check at the head failed; none when none did.
    #[must_use]
    pub fn first_failure_at(&self) -> Option<SystemTime> {
        self.checks
            .iter()
            .filter_map(|check| match check.result {
                CheckResult::Failed(at) => Some(at),
                CheckResult::Pending | CheckResult::Passed(_) => None,
            })
            .min()
    }

    /// When the last check at the head finished, green or red; none while none has.
    #[must_use]
    pub fn last_check_at(&self) -> Option<SystemTime> {
        self.checks
            .iter()
            .filter_map(|check| match check.result {
                CheckResult::Passed(at) | CheckResult::Failed(at) => Some(at),
                CheckResult::Pending => None,
            })
            .max()
    }
}

/// What the core reads of the ticket's epic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpicFacts {
    /// The epic.
    pub id: TicketId,
    /// Its latest signature holds the hash of its body as it reads now.
    pub signed: bool,
    /// The challenger's report is on it.
    pub challenged: bool,
}

/// One ticket this ticket waits on completely, and where that one stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dependency {
    /// The ticket depended on.
    pub ticket: TicketId,
    /// Where it stands, read from its issue: done means labelled `done` or closed as completed.
    pub standing: Standing,
}

/// A ruled question's marker: the start of the last run of `action` that the question counted,
/// read from the question's text (`Last run: <stamp>.`). Runs that started after it count again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuledMarker {
    /// The action the question named.
    pub action: ActionName,
    /// When the last run it counted started.
    pub last_run: SystemTime,
}

/// Everything [`super::place()`] reads for one ticket. The caller gathers it; the core reads no
/// file, forge, clock or process table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketFacts {
    /// The ticket.
    pub ticket: TicketId,
    /// Its epic, when it has one the caller could read as an open epic; none otherwise, which
    /// holds nothing back (there is nothing to judge it by, as the gate of today reads it).
    pub epic: Option<EpicFacts>,
    /// The open questions raised on the ticket or on its epic, by their issue numbers.
    pub open_questions: BTreeSet<TicketId>,
    /// The tickets it waits on completely.
    pub dependencies: Vec<Dependency>,
    /// Its pull request, when one is recorded.
    pub pull: Option<PullFacts>,
    /// Its phase run folders, in any order.
    pub runs: Vec<RunRecord>,
    /// Its phases, in order: its type's list with adjacent model actions already folded into one
    /// [`Phase`] (the caller's fold).
    pub phases: Vec<Phase>,
    /// What a model phase of it costs.
    pub size: TaskSize,
    /// The Engineer's priority on it, when set.
    pub priority: Option<Priority>,
    /// The markers of the ruled questions on it.
    pub ruled: Vec<RuledMarker>,
    /// When it became ready for its first phase, when the caller knows; none falls back to the
    /// time handed in, as the tick of today does for a ticket never started.
    pub ready_since: Option<SystemTime>,
}

impl TicketFacts {
    /// The pull request's current head; none without a pull request or a head.
    #[must_use]
    pub fn head(&self) -> Option<&CommitId> {
        self.pull.as_ref()?.head.as_ref()
    }

    /// Whether the pull request is merged.
    #[must_use]
    pub fn merged(&self) -> bool {
        self.pull
            .as_ref()
            .is_some_and(|pull| pull.merged_at.is_some())
    }

    /// The run that is live, if one is: the earliest started, should there be two.
    #[must_use]
    pub fn live_run(&self) -> Option<&RunRecord> {
        self.runs
            .iter()
            .filter(|run| run.live())
            .min_by_key(|run| run.started_at)
    }

    /// The head whose checks the ticket waits on: its phases wait on checks (one ends at
    /// `ci-done`), the pull request has a head, and the checks there have not all ended (none
    /// registered counts as not ended). None otherwise.
    #[must_use]
    pub fn awaited_checks(&self) -> Option<&CommitId> {
        let pull = self.pull.as_ref()?;
        let head = pull.head.as_ref()?;
        let waits_on_checks = self
            .phases
            .iter()
            .any(|phase| phase.ends.contains(&PhaseEnd::CiDone));
        (waits_on_checks && !pull.checks_ended()).then_some(head)
    }

    /// When the last run of `action` a ruled question counted started: the latest marker for it.
    #[must_use]
    pub fn ruled_through(&self, action: &ActionName) -> Option<SystemTime> {
        self.ruled
            .iter()
            .filter(|marker| &marker.action == action)
            .map(|marker| marker.last_run)
            .max()
    }
}

#[cfg(test)]
mod tests;
