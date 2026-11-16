//! Step 4 of the tick: launch every ready `wait` action detached (docs/zaf-actions.md §5.1).
//!
//! A `wait`-kind action (review, which runs `zaf check pr`, is one) waits on the outside and costs
//! nothing: no model starts for it and it takes no part of the budget. For each lane entry the tick
//! hands it, the ticket in the lane at such an action and let through by the gate
//! ([`crate::app::schedule::tick::may_start`]), [`launch_waits`] starts it with `zaf spawn phase
//! <task> <action>` and never waits for it: the call returns once the launcher is started, so the
//! tick returns before any wait ends. The ticket then runs until the wait's process ends, and
//! waits on the outside until the wait's `ends` fact holds, or the wait fails and its `on-fail`
//! starts through the one-model order (#442).
//!
//! A wait that has run at the pull request's current head waits on that head, so it is not in the
//! lane and not launched again: the placement is worked out from the forge and the run folders,
//! and nothing is stored. A launch that fails is logged and its ticket is left as it was, still in
//! the lane, so the next tick tries again; the others still go (#421 rulings 6 and 22). These
//! launches are not the tick's one model start, so [`TickOutcome::started`] stays empty here.
//!
//! The module is pure: the caller hands in the lane entries, a `launch` that performs `zaf spawn
//! phase <task> <action>` and a `log` for what failed.

use super::outcome::{Launched, TickOutcome};
use crate::app::actions::ActionName;
use crate::domain::TicketId;
use crate::domain::schedule::LaneEntry;

/// Launches the next action of each of `entries`, in the order given, and lists each one
/// launched, in that order. `launch(task, action)` performs `zaf spawn phase <task> <action>` and
/// gives `Err(why)` when it could not start; the failure is passed to `log` and the next entry's
/// wait still launches. A failed launch is not listed.
pub fn launch_waits(
    entries: &[LaneEntry],
    launch: &mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> Vec<Launched> {
    let mut launched = Vec::new();
    for entry in entries {
        let (task, action) = (entry.ticket, &entry.next);
        match launch(task, action) {
            Ok(()) => launched.push(Launched {
                task,
                action: action.clone(),
            }),
            Err(why) => log(&format!("could not launch {action} for {task}: {why}")),
        }
    }
    launched
}

/// The outcome of a tick that did step 4 alone: [`TickOutcome::waits`] filled, the rest empty
/// for the steps that fill them.
pub fn launch_outcome(
    entries: &[LaneEntry],
    launch: &mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> TickOutcome {
    TickOutcome {
        waits: launch_waits(entries, launch, log),
        ..TickOutcome::default()
    }
}

#[cfg(test)]
mod tests;
