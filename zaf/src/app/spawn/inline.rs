//! Step 3 of the tick: run every ready `zaf`-kind action inline (docs/zaf-actions.md §5.1).
//!
//! A `zaf`-kind action (merge is one) is run by zaf alone, with no model and no wait, so it costs
//! nothing: it holds no slot and no units, and running it changes nothing of the budget, which is
//! derived from the run folders and the process table on every read. For each lane entry the tick
//! hands it, the ticket in the lane at such an action and let through by the gate
//! ([`crate::app::schedule::tick::may_start`]), [`run_zaf_actions`] runs it, one after another,
//! and records the exit code in a [`Ran`]. A failure never stops the others: the ticket is placed
//! again at the next tick, still in the lane at the same action, so it runs again then (#421
//! rulings 6 and 22), until it got nowhere three times at one head (a merge the forge refuses at a
//! head that does not move): the gate then holds it, and the tick raises a question instead
//! ([`super::failures`]).
//!
//! The module is pure: the caller hands in the lane entries and a `run` that performs `zaf do
//! <action> <task>` and gives its exit code.

use super::outcome::{Ran, TickOutcome};
use crate::app::actions::ActionName;
use crate::domain::TicketId;
use crate::domain::schedule::LaneEntry;

/// Runs the next action of each of `entries`, in the order given, and lists each in the order it
/// ran. `run(task, action)` performs `zaf do <action> <task>` and gives its exit code; whatever the
/// code, the next entry's action still runs.
pub fn run_zaf_actions(
    entries: &[LaneEntry],
    run: &mut dyn FnMut(TicketId, &ActionName) -> u8,
) -> Vec<Ran> {
    entries
        .iter()
        .map(|entry| {
            let exit = run(entry.ticket, &entry.next);
            Ran {
                task: entry.ticket,
                action: entry.next.clone(),
                exit,
            }
        })
        .collect()
}

/// The outcome of a tick that did step 3 alone: [`TickOutcome::ran`] filled, the rest empty
/// for the steps that fill them.
pub fn run_inline(
    entries: &[LaneEntry],
    run: &mut dyn FnMut(TicketId, &ActionName) -> u8,
) -> TickOutcome {
    TickOutcome {
        ran: run_zaf_actions(entries, run),
        ..TickOutcome::default()
    }
}

#[cfg(test)]
mod tests;
