//! Gathers what [`derive_phase`](super::derive_phase) decides on, from the board, the slice and
//! check records, the forge and the workspace (docs/zaf-actions.md §3).
//!
//! This guards the merge path, so nothing here reads the environment: a process can set a
//! variable, and no variable can move a task into `land`.

use super::{PhaseFacts, PrFacts, WorkspaceFacts};
use crate::app::actions::{RunOutcome, SliceRecords, Tier};
use crate::domain::{IssueNumber, TaskLabel};
use crate::ports::{Board, Forge, PortError, Vcs};

/// A read the phase needs that failed: the board, the forge or the workspace could not say.
///
/// A failed read is not a fact about the task, so the phase is not derived and the action is
/// refused (`NOT_RUN`, exit 125): a guard that cannot see the state lets nothing through.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{task}'s phase cannot be read: {source}")]
pub struct PhaseUnreadable {
    /// The task whose phase was being derived.
    pub task: IssueNumber,
    /// What the failed read answered.
    pub source: PortError,
}

impl PhaseUnreadable {
    /// The refusal of the action `name`: it never ran, exit 125.
    pub fn not_run(&self, name: &str, tier: Option<Tier>) -> RunOutcome {
        RunOutcome::not_run(name, tier, self.to_string())
    }
}

/// The facts about `task` as they stand now.
///
/// A record that does not hold is no record, a recorded pull request the forge reports no head
/// for is no head, and a workspace record whose directory is gone is no workspace, so each falls
/// to an earlier phase. A read the board, the forge or the workspace fails is not a fact: it is
/// the [`PhaseUnreadable`] error, for the caller to refuse with.
pub fn gather_phase_facts(
    board: &dyn Board,
    forge: &dyn Forge,
    vcs: &dyn Vcs,
    records: &SliceRecords,
    task: IssueNumber,
) -> Result<PhaseFacts, PhaseUnreadable> {
    let unreadable = |source| PhaseUnreadable { task, source };
    let label = board.task(task).map_err(unreadable)?.label;
    let claimed = label == Some(TaskLabel::Claimed);
    let claim_comment = claimed
        && board
            .claims()
            .map_err(unreadable)?
            .iter()
            .any(|claim| claim.task == task);
    let workspace = match records.workspace(task) {
        Some(workspace) => Some(WorkspaceFacts {
            latest_commit: Some(
                vcs.latest_commit(&workspace)
                    .map_err(unreadable)?
                    .to_string(),
            ),
        }),
        None => None,
    };
    let pr_number = records.pr(task);
    let pr = match pr_number {
        Some(pr) => Some(PrFacts {
            recorded_at: records.pr_recorded_at(task),
            head: forge.head(pr).map_err(unreadable)?,
        }),
        None => None,
    };
    let facts = PhaseFacts {
        done: label == Some(TaskLabel::Done),
        claimed,
        claim_comment,
        workspace,
        pr,
        task_check: records.task_check(task),
        pr_check: pr_number.and_then(|pr| records.pr_check(pr)),
    };
    Ok(facts)
}

#[cfg(test)]
mod tests;
