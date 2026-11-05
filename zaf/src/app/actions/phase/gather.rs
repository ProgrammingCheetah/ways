//! Gathers what [`derive_phase`](super::derive_phase) decides on, from the board, the slice and
//! check records, the forge and the workspace (docs/zaf-actions.md §3).
//!
//! This guards the merge path, so nothing here reads the environment: a process can set a
//! variable, and no variable can move a task into `land`.

use super::{PhaseFacts, PrFacts, WorkspaceFacts};
use crate::app::actions::SliceRecords;
use crate::domain::{IssueNumber, TaskLabel};
use crate::ports::{Board, Forge, PortError, Vcs};

/// The facts about `task` as they stand now.
///
/// A record that does not hold is no record, and a workspace whose latest commit cannot be read
/// has none, so either falls to an earlier phase. A board or forge that cannot answer is the
/// error, for the caller to act on: a failed read is not a fact about the task.
pub fn gather_phase_facts(
    board: &dyn Board,
    forge: &dyn Forge,
    vcs: &dyn Vcs,
    records: &SliceRecords,
    task: IssueNumber,
) -> Result<PhaseFacts, PortError> {
    let label = board.task(task)?.label;
    let claimed = label == Some(TaskLabel::Claimed);
    let live_lease = claimed && board.claims()?.iter().any(|claim| claim.task == task);
    let workspace = records.workspace(task).map(|workspace| WorkspaceFacts {
        latest_commit: vcs
            .latest_commit(&workspace)
            .ok()
            .map(|commit| commit.to_string()),
    });
    let pr_number = records.pr(task);
    let pr = match pr_number {
        Some(pr) => Some(PrFacts {
            recorded_at: records.pr_recorded_at(task),
            head: forge.head(pr)?,
        }),
        None => None,
    };
    let facts = PhaseFacts {
        done: label == Some(TaskLabel::Done),
        claimed,
        live_lease,
        workspace,
        pr,
        task_check: records.task_check(task),
        pr_check: pr_number.and_then(|pr| records.pr_check(pr)),
    };
    Ok(facts)
}

#[cfg(test)]
mod tests;
