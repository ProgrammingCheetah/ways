//! The board's state label as output only (R5): what a place shows as, in the existing
//! [`TicketState`] vocabulary. Nothing in the core reads a label back.

use super::place::{Place, WaitingOn};
use crate::domain::TicketState;

/// The state label for a ticket at `place`, `claimed` saying whether a claim holds it (the claim
/// owns ready and claimed, so the label never takes one from it):
///
/// - running: `claimed`;
/// - done: `done`;
/// - waiting on a question: `question`, claimed or not;
/// - waiting on a ticket and not claimed: `blocked`;
/// - anything else (in the lane, or waiting on its epic, a check, or a ticket while claimed):
///   `claimed` when claimed, else `ready`.
#[must_use]
pub fn label_for(place: &Place, claimed: bool) -> TicketState {
    let held = if claimed {
        TicketState::Claimed
    } else {
        TicketState::Ready
    };
    match place {
        Place::Running(_) => TicketState::Claimed,
        Place::Done => TicketState::Done,
        Place::Waiting(waits) => {
            if waits
                .iter()
                .any(|waiting_on| matches!(waiting_on, WaitingOn::Question(_)))
            {
                TicketState::Question
            } else if !claimed
                && waits
                    .iter()
                    .any(|waiting_on| matches!(waiting_on, WaitingOn::Ticket(_)))
            {
                TicketState::Blocked
            } else {
                held
            }
        }
        Place::InLane { .. } => held,
    }
}

#[cfg(test)]
mod tests;
