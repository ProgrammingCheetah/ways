//! Where a ticket sits, from facts alone (R3, R5, R15, R16, R22, R29, R30): [`place()`] for one
//! ticket, [`Placement::of`] for all of them.
//!
//! A ticket sits in exactly one place, the first of these that holds:
//!
//! 1. **Running:** a phase run of it is live.
//! 2. **Done:** its pull request is merged, or every phase's end fact holds.
//! 3. **Waiting:** its epic is unsigned or unchallenged (an epic whose challenge was voided holds
//!    back only a ticket with no phase started), a question is open on it or its epic, a
//!    dependency is not done, or the checks at its head have not ended (none registered counts as
//!    not ended), or its current phase awaits something outside. It waits on every one of these
//!    that holds, as a set. With none of them, a ticket at its merge ([`super::at_merge`]) waits
//!    on the ticket fresh from a fix that merges first (R30) and on the integration branch while
//!    a check is pending at its head (R29), unless the Engineer ruled it through at that head.
//! 4. **In the lane:** none of the above, with the next action to run and since when.
//!
//! Running comes first, so a ticket whose model still works keeps its slot even when a question
//! appears on it (R16).

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use super::facts::{EpicFacts, TicketFacts};
use super::lane::{Lane, LaneEntry, weight};
use super::phase::{ActionName, CurrentPhase, PhaseEnd, PhaseStatus, current_phase};
use crate::domain::{CommitId, Standing, TicketId};

/// A live phase run: the ticket holds its slot until the run's end is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The action it runs.
    pub phase: ActionName,
    /// The pull request's head when it started; none without one.
    pub head: Option<CommitId>,
    /// When it started.
    pub started_at: SystemTime,
}

/// Why a ticket waits. The reason is where the ticket sits, never a label on it. Ordered as the
/// placement reads them, so the first of a set is the first that holds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WaitingOn {
    /// Its epic, not signed or not challenged yet.
    Epic(TicketId),
    /// A question open on it or on its epic: the Engineer's ruling.
    Question(TicketId),
    /// Another ticket it depends on, not done.
    Ticket(TicketId),
    /// The checks at this ticket's head: CI, a review, anything outside.
    Check(TicketId, CommitId),
    /// The merge of another ticket, which is fresh from a fix and merges first (R30). Not a
    /// dependency: it ends by itself when that ticket merges or stops being fresh.
    Merge(TicketId),
    /// The checks at this commit, the integration branch's head: the last merge is still being
    /// checked there (R29).
    Base(CommitId),
}

/// Where one ticket sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A phase run of it is live.
    Running(Run),
    /// It waits on each of these, never on none.
    Waiting(BTreeSet<WaitingOn>),
    /// It may run.
    InLane {
        /// The next run: the current phase, or its `on_fail` when it has failed on something the
        /// `on_fail` answers ([`super::Failure::for_on_fail`]).
        next: ActionName,
        /// Since when it is ready: when its phase failed, or else when the phase before it
        /// ended, or else when it became ready for its first phase, or else the time handed in.
        since: SystemTime,
    },
    /// Its pull request is merged, or every phase's end fact holds.
    Done,
}

/// Where the ticket of `facts` sits at `now`, the first place that holds (see the module).
///
/// A ticket at its merge reads [`TicketFacts::merging`] for the two waits of the merge: the
/// caller fills it for all the tickets at once ([`super::order_merges`]), and left at its default
/// it holds nothing back.
///
/// `now` is only a fallback for `since`, for a ticket whose readiness no fact times. Placement
/// never reads a state label, never reads a claim and never reads getting nowhere
/// ([`super::nowhere`]): the caller holds a start that gets nowhere and raises its question,
/// which places the ticket in waiting.
#[must_use]
pub fn place(facts: &TicketFacts, now: SystemTime) -> Place {
    if let Some(run) = facts.live_run() {
        return Place::Running(Run {
            phase: run.action.clone(),
            head: run.head.clone(),
            started_at: run.started_at,
        });
    }
    if facts.merged() {
        return Place::Done;
    }
    let Some(current) = current_phase(facts) else {
        return Place::Done;
    };
    let mut waits = BTreeSet::new();
    let started = phase_started(facts, &current);
    if let Some(epic) = facts.epic.filter(|epic| epic_holds(epic, started)) {
        waits.insert(WaitingOn::Epic(epic.id));
    }
    waits.extend(
        facts
            .open_questions
            .iter()
            .copied()
            .map(WaitingOn::Question),
    );
    waits.extend(
        facts
            .dependencies
            .iter()
            .filter(|dependency| dependency.standing != Standing::Done)
            .map(|dependency| WaitingOn::Ticket(dependency.ticket)),
    );
    if let Some(head) = facts.awaited_checks() {
        waits.insert(WaitingOn::Check(facts.ticket, head.clone()));
    }
    if let PhaseStatus::Awaiting(waiting_on) = &current.status {
        waits.insert(waiting_on.clone());
    }
    // The merge's own waits hold only a ticket that would otherwise be in the lane at its merge.
    let merges_next =
        current.phase.ends.contains(&PhaseEnd::Merged) && current.next() == &current.phase.name;
    if waits.is_empty() && merges_next {
        let first = facts.merging.first.filter(|first| *first != facts.ticket);
        waits.extend(first.map(WaitingOn::Merge));
        // A ruled question on the wait at this very head is the way through it (R19).
        let base = facts.merging.base.awaited();
        let base = base.filter(|head| facts.merging.ruled_at.as_ref() != Some(*head));
        waits.extend(base.cloned().map(WaitingOn::Base));
    }
    if !waits.is_empty() {
        return Place::Waiting(waits);
    }
    Place::InLane {
        next: current.next().clone(),
        since: ready_since(&current, facts).unwrap_or(now),
    }
}

/// Whether `epic` holds back a ticket, `started` saying whether the ticket has a phase started. An
/// unsigned epic holds every ticket. A voided challenge holds only a ticket with no phase started
/// (D22, Q13): one in its phases goes on, between phases too. An epic never challenged holds every
/// ticket.
fn epic_holds(epic: &EpicFacts, started: bool) -> bool {
    if !epic.signed {
        return true;
    }
    if epic.voided {
        return !started;
    }
    !epic.challenged
}

/// Whether the ticket has a phase started: a run of it is on record (it is not live here, or the
/// ticket would be running), or a phase before the current one has ended, as a pull request opened
/// by hand ends `pr-open`.
fn phase_started(facts: &TicketFacts, current: &CurrentPhase<'_>) -> bool {
    !facts.runs.is_empty() || current.previous.is_some()
}

/// Since when a ticket whose current phase is `current` is ready for its next run, as far as a
/// fact times it: when the phase failed, or else when the phase before it ended, or else when the
/// ticket became ready for its first phase. It is the `since` of its place in the lane, and the
/// clock of a ticket held at its merge by the integration branch ([`super::base_clock`]).
pub(super) fn ready_since(current: &CurrentPhase<'_>, facts: &TicketFacts) -> Option<SystemTime> {
    let failed_at = match &current.status {
        PhaseStatus::Failed(failure) => failure.at,
        PhaseStatus::Done | PhaseStatus::Open | PhaseStatus::Awaiting(_) => None,
    };
    failed_at.or(current.previous).or(facts.ready_since)
}

/// Every ticket in its one place: the running ones by ticket, the waiting ones under each thing
/// they wait on, the lane in order and the done ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    /// The live runs, one per slot in use.
    pub running: BTreeMap<TicketId, Run>,
    /// The waiting tickets, unordered, listed under each thing they wait on.
    pub waiting: BTreeMap<WaitingOn, BTreeSet<TicketId>>,
    /// The tickets that may run, the front first.
    pub lane: Lane,
    /// The tickets that are done.
    pub done: BTreeSet<TicketId>,
}

impl Placement {
    /// Places every ticket of `tickets` at `now`, each once: a ticket whose facts come twice is
    /// placed by the first, since a ticket already in a place is not placed again.
    #[must_use]
    pub fn of(tickets: &[TicketFacts], now: SystemTime) -> Self {
        let mut placement = Self::default();
        let mut seen = BTreeSet::new();
        for facts in tickets {
            if !seen.insert(facts.ticket) {
                continue;
            }
            match place(facts, now) {
                Place::Running(run) => {
                    placement.running.insert(facts.ticket, run);
                }
                Place::Waiting(waits) => {
                    for waiting_on in waits {
                        placement
                            .waiting
                            .entry(waiting_on)
                            .or_default()
                            .insert(facts.ticket);
                    }
                }
                Place::InLane { next, since } => {
                    placement.lane.enqueue(LaneEntry {
                        ticket: facts.ticket,
                        next,
                        since,
                        size: facts.size,
                        weight: weight(since, facts.priority, now),
                    });
                }
                Place::Done => {
                    placement.done.insert(facts.ticket);
                }
            }
        }
        placement
    }

    /// The tickets that wait, each once.
    #[must_use]
    pub fn waiting_tickets(&self) -> BTreeSet<TicketId> {
        self.waiting.values().flatten().copied().collect()
    }
}

#[cfg(test)]
mod tests;
