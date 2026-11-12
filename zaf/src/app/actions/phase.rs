//! The Team Phases and the rule table that derives one (docs/zaf-actions.md §3).
//!
//! [`derive_phase`] is the guard of the merge path, so it is a pure decision: it
//! reads no file, no board and no clock, only the [`PhaseFacts`] it is handed.
//! Wherever a fact is missing it falls to the earlier phase: no head is no result,
//! and a time it cannot read is never newer. [`gather_phase_facts`] collects those facts; a
//! board, forge or workspace read that fails is no missing fact but a [`PhaseUnreadable`]
//! refusal (`NOT_RUN`, exit 125), so a guard that cannot see the state lets nothing through.

use std::time::SystemTime;

use crate::app::check::{CheckRecord, Verdict};

mod gather;

pub use gather::{PhaseUnreadable, gather_phase_facts};

mod standing;

pub use standing::{
    CheckResult, ListedPhase, MODEL_FAILURE_LIMIT, PHASES_DIR, PhaseRun, PullFacts, ReviewFact,
    RunExit, Standing, StandingFacts, StandingUnreadable, WaitFailure, derive_standing,
    ended_instances, gather_standing_facts, running_tasks, wait_failure,
};

/// Where a task stands. The seven of §3 and `Unclaimed`, in lifecycle order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum Phase {
    /// No rule held. Derived only: no `phases:` line may name it.
    Unclaimed,
    /// Claimed, no slice yet.
    Claim,
    /// Its slice open, a programmer at work.
    Build,
    /// Its task checks have passed on the workspace.
    Check,
    /// Its pull request open, review steps not yet run.
    Publish,
    /// The pull request's steps have run.
    Review,
    /// The last `check pr` result is clean for the pull request's head.
    Land,
    /// Merged.
    Done,
}

impl Phase {
    /// The phases a `phases:` line may name: the seven of §3, never `Unclaimed`.
    pub const DECLARABLE: [Self; 7] = [
        Self::Claim,
        Self::Build,
        Self::Check,
        Self::Publish,
        Self::Review,
        Self::Land,
        Self::Done,
    ];

    /// The declarable phase called `name`. This is the only parser, so a name
    /// outside [`Self::DECLARABLE`] cannot become a phase.
    pub fn declared(name: &str) -> Option<Self> {
        Self::DECLARABLE
            .into_iter()
            .find(|phase| phase.to_string() == name)
    }
}

/// The task's `.zaf/slices/<task>` record, with the workspace it names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceFacts {
    /// The workspace's latest commit; none is never the pull request's head. The gatherer
    /// always fills it: a workspace it cannot read refuses the action instead.
    pub latest_commit: Option<String>,
}

/// The task's `.zaf/slices/<task>.pr` record, with the pull request it names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrFacts {
    /// When the record was written; none when it could not be read.
    pub recorded_at: Option<SystemTime>,
    /// The pull request's current head; none when the forge reported none.
    pub head: Option<String>,
}

/// Everything the rule table reads about one task. The caller gathers it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhaseFacts {
    /// The board's `done` label.
    pub done: bool,
    /// The board's `claimed` label.
    pub claimed: bool,
    /// The board holds a claim comment for the task. It has no expiry: it stands until released.
    pub claim_comment: bool,
    /// The workspace record, when one exists.
    pub workspace: Option<WorkspaceFacts>,
    /// The pull request record, when one exists.
    pub pr: Option<PrFacts>,
    /// The last `zaf check <task>` record.
    pub task_check: Option<CheckRecord>,
    /// The last `zaf check pr` record.
    pub pr_check: Option<CheckRecord>,
}

impl PhaseFacts {
    /// The pull request's head as a head to compare: an empty one is no head.
    fn current_head(&self) -> Option<&str> {
        let head = self.pr.as_ref()?.head.as_deref()?;
        (!head.is_empty()).then_some(head)
    }

    /// The `check pr` record that counts as a result: it is for the current head.
    fn current_result(&self) -> Option<&CheckRecord> {
        let head = self.current_head()?;
        self.pr_check
            .as_ref()
            .filter(|record| record.head.as_deref() == Some(head))
    }

    /// The workspace's latest commit is the pull request's current head.
    fn workspace_at_head(&self) -> bool {
        let commit = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.latest_commit.as_deref());
        commit.is_some() && commit == self.current_head()
    }

    /// The last task check is clean.
    fn clean_task_check(&self) -> Option<&CheckRecord> {
        self.task_check
            .as_ref()
            .filter(|record| record.verdict == Verdict::Clean)
    }

    /// The last task check is not clean, whenever it ran: it is the last one.
    fn failed_task_check(&self) -> bool {
        self.task_check
            .as_ref()
            .is_some_and(|record| record.verdict != Verdict::Clean)
    }

    /// A clean task check began strictly after `time`.
    fn clean_task_check_newer_than(&self, time: Option<SystemTime>) -> bool {
        let check = self.clean_task_check().and_then(began);
        matches!((check, time), (Some(check), Some(time)) if check > time)
    }
}

/// When a record's check began; none for a record without a readable time.
fn began(record: &CheckRecord) -> Option<SystemTime> {
    humantime::parse_rfc3339(record.at.as_deref()?).ok()
}

/// The task's phase: the first rule that holds decides.
pub fn derive_phase(facts: &PhaseFacts) -> Phase {
    if facts.done {
        return Phase::Done;
    }
    if let Some(result) = facts.current_result() {
        return match result.verdict {
            Verdict::Clean if facts.workspace_at_head() => Phase::Land,
            Verdict::StepFailed => Phase::Review,
            Verdict::Clean | Verdict::MustFix => {
                if facts.clean_task_check_newer_than(began(result)) {
                    Phase::Check
                } else {
                    Phase::Build
                }
            }
        };
    }
    if let Some(pr) = &facts.pr {
        return if facts.failed_task_check() {
            Phase::Build
        } else if facts.clean_task_check_newer_than(pr.recorded_at) {
            Phase::Check
        } else {
            Phase::Publish
        };
    }
    if facts.clean_task_check().is_some() {
        return Phase::Check;
    }
    if facts.workspace.is_some() {
        return Phase::Build;
    }
    if facts.claimed && facts.claim_comment {
        return Phase::Claim;
    }
    Phase::Unclaimed
}

#[cfg(test)]
mod tests;
