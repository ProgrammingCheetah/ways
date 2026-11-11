//! Step 3 of the tick: run every ready `zaf`-kind action inline (docs/zaf-actions.md §5.1).
//!
//! A `zaf`-kind action (merge is one) is run by zaf alone, with no model and no wait, so it costs
//! nothing: it holds no slot and no units, and running it changes nothing of the budget, which is
//! derived from the run folders and the process table on every read. For each task whose
//! standing is `Ready` at such an action, [`run_zaf_actions`] runs it, one task after another, and
//! records the exit code in a [`Ran`]. A failure never stops the others: the task's standing is
//! derived again at the next tick, still `Ready` at the same action, so it runs again then
//! (#421 rulings 6 and 22).
//!
//! The module is pure: the caller hands in each task's phase list and facts and a `run` that
//! performs `zaf do <action> <task>` and gives its exit code.

use super::outcome::{Ran, TickOutcome};
use crate::app::actions::{
    ActionKind, ActionName, ListedPhase, Standing, StandingFacts, derive_standing,
};
use crate::domain::TicketId;

/// One task the tick reads: its type's phase list and what is known of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickTask {
    /// The task.
    pub task: TicketId,
    /// Its type's phase list, each action with its declared `kind`, `ends` and `on-fail`.
    pub list: Vec<ListedPhase>,
    /// The facts its standing is derived from.
    pub facts: StandingFacts,
}

impl TickTask {
    /// The `zaf`-kind action the task is ready to run, if its standing is `Ready` at one.
    pub fn ready_zaf_action(&self) -> Option<ActionName> {
        let Standing::Ready { action, .. } = derive_standing(&self.list, &self.facts) else {
            return None;
        };
        self.list
            .iter()
            .any(|phase| phase.action == action && phase.kind == ActionKind::Zaf)
            .then_some(action)
    }
}

/// Runs the ready `zaf`-kind action of each of `tasks`, in the order given, and lists each in
/// the order it ran. `run(task, action)` performs `zaf do <action> <task>` and gives its exit
/// code; whatever the code, the next task's action still runs.
pub fn run_zaf_actions(
    tasks: &[TickTask],
    run: &mut dyn FnMut(TicketId, &ActionName) -> u8,
) -> Vec<Ran> {
    let ready: Vec<(TicketId, ActionName)> = tasks
        .iter()
        .filter_map(|task| Some((task.task, task.ready_zaf_action()?)))
        .collect();
    ready
        .into_iter()
        .map(|(task, action)| {
            let exit = run(task, &action);
            Ran { task, action, exit }
        })
        .collect()
}

/// The outcome of a tick that did step 3 alone: [`TickOutcome::ran`] filled, the rest empty
/// for the steps that fill them.
pub fn run_inline(
    tasks: &[TickTask],
    run: &mut dyn FnMut(TicketId, &ActionName) -> u8,
) -> TickOutcome {
    TickOutcome {
        ran: run_zaf_actions(tasks, run),
        ..TickOutcome::default()
    }
}

#[cfg(test)]
mod tests;
