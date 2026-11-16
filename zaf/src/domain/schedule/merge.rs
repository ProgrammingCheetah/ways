//! The order of merges on one integration branch (R29, R30): which tickets stand at their merge,
//! which one is fresh from a fix, and who merges first.
//!
//! Every merge moves the base under the pull requests still open, so two rules order them:
//!
//! - **R29, one merge at a time:** a ticket about to merge waits while the integration branch
//!   has a check still pending at its head ([`BaseFacts::awaited`](super::BaseFacts::awaited)):
//!   the last merge is still being checked there.
//! - **R30, a ticket out of its fix merges before any other:** while a ticket is fresh from a fix
//!   ([`fresh_from_fix`]), every other ticket about to merge waits on it, so the base does not
//!   move under it again while its checks and its review run.
//!
//! Both hold a ticket only where it would otherwise be in the lane at its merge ([`at_merge`]);
//! [`super::place()`] applies them. R30 reads more than one ticket, and a ticket is placed from
//! its own facts, so [`order_merges`] works the order out across the tickets once and writes
//! each ticket's answer into its facts ([`MergeFacts::first`](super::MergeFacts::first)):
//! everything that places a ticket then reads the same answer. The tickets handed in are those of
//! one integration branch, as one tick gathers them.
//!
//! The hold of a ticket fresh from a fix cannot last: it ends when the ticket merges, when its
//! current phase fails at the head the fix left (a red check, a new conflict), when a question
//! opens on it or its epic, when it comes to wait on a ticket, and when its head moves by
//! anything other than a fix. Two fresh tickets never wait on each other: the order is total, by
//! when each fix ended and then by ticket.

use std::time::SystemTime;

use super::facts::TicketFacts;
use super::outcome::is_machinery;
use super::phase::{PhaseEnd, PhaseStatus, current_phase};
use crate::domain::{Standing, TicketId};

/// Whether the ticket stands at its merge: its next run is the own action of a phase one of
/// whose end facts is `merged`, not that phase's `on_fail`. False when every phase ended.
#[must_use]
pub fn at_merge(facts: &TicketFacts) -> bool {
    current_phase(facts).is_some_and(|current| {
        current.phase.ends.contains(&PhaseEnd::Merged) && current.next() == &current.phase.name
    })
}

/// When the fix the ticket is fresh from ended; none when it is not fresh from a fix.
///
/// It is fresh from a fix when all of these hold:
///
/// - it has a pull request with a head, not merged, and a phase still to end;
/// - the latest run that brought the pull request to its current head (it ended, not by the
///   machinery ([`is_machinery`]), started at another head and left this one) is a run of an
///   action one of its phases names as `on_fail`, which exited 0 and raised no question. A head
///   nothing on record brought there (a push by hand) is not a fix's;
/// - its current phase has not failed at that head ([`PhaseStatus::Failed`]);
/// - it is not with the Engineer: no question is open on it or its epic, and its epic is signed
///   and challenged;
/// - it waits on no ticket: a dependency not done could be one of the tickets it holds.
#[must_use]
pub fn fresh_from_fix(facts: &TicketFacts) -> Option<SystemTime> {
    let pull = facts.pull.as_ref()?;
    if pull.merged_at.is_some() {
        return None;
    }
    let head = pull.head.as_ref()?;
    let current = current_phase(facts)?;
    if matches!(current.status, PhaseStatus::Failed(_)) {
        return None;
    }
    let with_the_engineer = !facts.open_questions.is_empty()
        || facts
            .epic
            .is_some_and(|epic| !epic.signed || !epic.challenged);
    let waits_on_a_ticket = facts
        .dependencies
        .iter()
        .any(|dependency| dependency.standing != Standing::Done);
    if with_the_engineer || waits_on_a_ticket {
        return None;
    }
    let brought = facts
        .runs
        .iter()
        .filter(|run| !run.live() && !is_machinery(run.exit.map(|exit| exit.code)))
        .filter(|run| run.left() == Some(head) && run.head.as_ref() != Some(head))
        .max_by_key(|run| run.started_at)?;
    let a_fix = facts
        .phases
        .iter()
        .any(|phase| phase.on_fail.as_ref() == Some(&brought.action));
    let exit = brought.exit?;
    (a_fix && exit.code.is_zero() && brought.questions.is_empty()).then_some(exit.at)
}

/// The ticket of `tickets` that merges before every other: the one fresh from a fix
/// ([`fresh_from_fix`]) whose fix ended first, the lower ticket when two ended together. None
/// when no ticket is fresh from a fix.
#[must_use]
pub fn merges_first(tickets: &[TicketFacts]) -> Option<TicketId> {
    tickets
        .iter()
        .filter_map(|facts| Some((fresh_from_fix(facts)?, facts.ticket)))
        .min()
        .map(|(_, ticket)| ticket)
}

/// Writes into each of `tickets` the ticket that merges before it
/// ([`MergeFacts::first`](super::MergeFacts::first)): [`merges_first`] for every ticket but that
/// one itself, which waits on nobody. A second ticket fresh from a fix so waits on the first, and
/// the first never on the second.
pub fn order_merges(tickets: &mut [TicketFacts]) {
    let first = merges_first(tickets);
    for facts in tickets {
        facts.merging.first = first.filter(|first| *first != facts.ticket);
    }
}

#[cfg(test)]
mod tests;
