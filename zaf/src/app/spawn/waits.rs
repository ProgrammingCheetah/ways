//! Step 4 of the tick: launch every ready `wait` action detached (docs/zaf-actions.md §5.1).
//!
//! A `wait`-kind action (review, which runs `zaf check pr`, is one) waits on the outside and costs
//! nothing: no model starts for it and it takes no part of the budget. For each task whose standing
//! is `Ready` at such an action, [`launch_waits`] starts it with `zaf spawn phase <task> <action>`
//! and never waits for it: the call returns once the launcher is started, so the tick returns
//! before any wait ends. The wait then reads as `Waiting` until its `ends` fact holds, or fails and
//! its `on-fail` starts through the one-model order (#442).
//!
//! A wait that has run at the pull request's current head is `Waiting`, not `Ready`, so it is not
//! launched again: the standing is derived from the forge and the run folders, and nothing is
//! stored. A launch that fails is logged and its task is left as it was, still `Ready`, so the next
//! tick tries again; the others still go (#421 rulings 6 and 22). These launches are not the tick's
//! one model start, so [`TickOutcome::started`] stays empty here.
//!
//! The module is pure: the caller hands in each task's phase list and facts, a `launch` that
//! performs `zaf spawn phase <task> <action>` and a `log` for what failed.

use super::inline::TickTask;
use super::outcome::{Launched, TickOutcome};
use crate::app::actions::{ActionKind, ActionName, Standing, derive_standing};
use crate::domain::TicketId;

impl TickTask {
    /// The `wait` action the task is ready to launch, if its standing is `Ready` at one.
    pub fn ready_wait_action(&self) -> Option<ActionName> {
        let Standing::Ready { action, .. } = derive_standing(&self.list, &self.facts) else {
            return None;
        };
        self.list
            .iter()
            .any(|phase| phase.action == action && phase.kind == ActionKind::Wait)
            .then_some(action)
    }
}

/// Launches the ready `wait` action of each of `tasks`, in the order given, and lists each one
/// launched, in that order. `launch(task, action)` performs `zaf spawn phase <task> <action>` and
/// gives `Err(why)` when it could not start; the failure is passed to `log` and the next task's
/// wait still launches. A failed launch is not listed.
pub fn launch_waits(
    tasks: &[TickTask],
    launch: &mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> Vec<Launched> {
    let ready: Vec<(TicketId, ActionName)> = tasks
        .iter()
        .filter_map(|task| Some((task.task, task.ready_wait_action()?)))
        .collect();
    let mut launched = Vec::new();
    for (task, action) in ready {
        match launch(task, &action) {
            Ok(()) => launched.push(Launched { task, action }),
            Err(why) => log(&format!("could not launch {action} for {task}: {why}")),
        }
    }
    launched
}

/// The outcome of a tick that did step 4 alone: [`TickOutcome::waits`] filled, the rest empty
/// for the steps that fill them.
pub fn launch_outcome(
    tasks: &[TickTask],
    launch: &mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> TickOutcome {
    TickOutcome {
        waits: launch_waits(tasks, launch, log),
        ..TickOutcome::default()
    }
}

#[cfg(test)]
mod tests;
