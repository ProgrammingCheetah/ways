//! Waits that must not last forever without the Engineer hearing of them (R19): an overdue check
//! wait, a dependency that can never complete, and an epic planned but not challenged.
//!
//! All are detected here as facts; raising the question is the caller's. A wait on a question or
//! on an unsigned epic has no clock (the Engineer already has those), so a ticket that also waits
//! on one is never overdue; and a dependency that can still complete sits in a place that has one
//! of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use super::facts::{CheckName, CheckResult, TicketFacts};
use super::phase::{ActionName, PhaseEnd, current_phase};
use super::place::{Place, WaitingOn, place};
use crate::domain::{CommitId, Epic, EpicStage, Standing, TicketId};

/// How long a ticket may wait on the checks at one head before a question is raised: 2 hours
/// (the plan's proposal), a constant as `STARVE_AFTER` is.
pub const CHECK_DEADLINE: Duration = Duration::from_secs(2 * 60 * 60);

/// How long an epic may be planned and not challenged before a question is raised on it: 24 hours
/// (the plan's proposal), from when planning ended.
pub const CHALLENGE_DEADLINE: Duration = Duration::from_secs(24 * 60 * 60);

/// A check wait past its deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverdueCheck {
    /// The head whose checks have not ended.
    pub head: CommitId,
    /// When the wait's clock started (see [`check_clock`]).
    pub since: SystemTime,
    /// What has not come at the head.
    pub awaited: Awaited,
}

/// What an overdue check wait still waits for at the head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Awaited {
    /// No check has registered at the head: the runner may be down, or a path filter skipped the
    /// change.
    NoCheck,
    /// These checks at the head are still pending, in the forge's order.
    Pending(Vec<CheckName>),
    /// Every check at the head has ended, and the current phase, its run having exited 0, awaits
    /// these facts from outside (a review, a merge).
    Outside {
        /// The current phase.
        phase: ActionName,
        /// Its end facts that come from outside.
        ends: Vec<PhaseEnd>,
    },
}

/// When the clock of a wait on the checks at the head started: the latest of the pull request
/// opening, the end of the latest run that left the head and the last check finishing there.
/// None without a pull request.
#[must_use]
pub fn check_clock(facts: &TicketFacts) -> Option<SystemTime> {
    let pull = facts.pull.as_ref()?;
    let head = facts.head();
    let run_end = facts
        .runs
        .iter()
        .filter(|run| run.left() == head)
        .filter_map(|run| run.exit.map(|exit| exit.at))
        .max();
    [Some(pull.opened_at), run_end, pull.last_check_at()]
        .into_iter()
        .flatten()
        .max()
}

/// The ticket's check wait, when it waits on the checks at its head at `now` and has for
/// [`CHECK_DEADLINE`] or more by [`check_clock`]: a check that never registered, a runner that
/// is down. None otherwise: a ticket that is not waiting on a check, and one that also waits on a
/// question or on its epic, whose wait has no clock (the Engineer already has it; the epic's own
/// deadline is [`unchallenged`]).
#[must_use]
pub fn overdue(facts: &TicketFacts, now: SystemTime) -> Option<OverdueCheck> {
    let Place::Waiting(waits) = place(facts, now) else {
        return None;
    };
    let with_the_engineer = waits
        .iter()
        .any(|waiting_on| matches!(waiting_on, WaitingOn::Epic(_) | WaitingOn::Question(_)));
    if with_the_engineer {
        return None;
    }
    let head = waits.into_iter().find_map(|waiting_on| match waiting_on {
        WaitingOn::Check(_, head) => Some(head),
        WaitingOn::Epic(_) | WaitingOn::Question(_) | WaitingOn::Ticket(_) => None,
    })?;
    let since = check_clock(facts)?;
    let waited = now.duration_since(since).unwrap_or_default();
    (waited >= CHECK_DEADLINE).then(|| OverdueCheck {
        head,
        since,
        awaited: awaited(facts),
    })
}

/// What the ticket's check wait still waits for: no check at the head, the pending ones, or, with
/// every check ended, what the current phase awaits from outside.
fn awaited(facts: &TicketFacts) -> Awaited {
    let checks = facts
        .pull
        .as_ref()
        .map(|pull| pull.checks.as_slice())
        .unwrap_or_default();
    if facts.awaited_checks().is_some() {
        let pending: Vec<CheckName> = checks
            .iter()
            .filter(|check| check.result == CheckResult::Pending)
            .map(|check| check.name.clone())
            .collect();
        return if pending.is_empty() {
            Awaited::NoCheck
        } else {
            Awaited::Pending(pending)
        };
    }
    match current_phase(facts) {
        Some(current) => Awaited::Outside {
            phase: current.phase.name.clone(),
            ends: current
                .phase
                .ends
                .iter()
                .copied()
                .filter(|end| end.comes_from_outside())
                .collect(),
        },
        None => Awaited::NoCheck,
    }
}

/// An epic planned and not challenged past its deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverdueChallenge {
    /// The epic.
    pub epic: TicketId,
    /// When its clock started: when planning ended, or its signature for an epic whose planning
    /// left no time.
    pub since: SystemTime,
}

/// The epic's challenge wait, when it is planned (signed, with tasks, not challenged) at `now`
/// and has been for [`CHALLENGE_DEADLINE`] or more, counted from when planning ended
/// ([`Epic::planned_at`]) or, with no such time, from its signature: a challenger that ended with
/// no report, or never got a slot. None otherwise: an epic that is unsigned (the Engineer's to
/// sign, so no clock), challenged, done or still with no task; and one with an open question,
/// which `questioned` says, since the Engineer already has it.
#[must_use]
pub fn unchallenged(epic: &Epic, questioned: bool, now: SystemTime) -> Option<OverdueChallenge> {
    if questioned || epic.stage() != EpicStage::Planned {
        return None;
    }
    let since = epic
        .planned_at
        .or_else(|| epic.signature.as_ref().map(|signature| signature.at))?;
    let waited = now.duration_since(since).unwrap_or_default();
    (waited >= CHALLENGE_DEADLINE).then_some(OverdueChallenge {
        epic: epic.id,
        since,
    })
}

/// Each ticket and the dependencies it still waits on, as the dependency facts name them.
pub type DependencyGraph = BTreeMap<TicketId, Vec<TicketId>>;

/// The dependency graph of `tickets`: every ticket and its dependencies that are not done, since
/// a done one closes any cycle through it.
#[must_use]
pub fn dependency_graph(tickets: &[TicketFacts]) -> DependencyGraph {
    tickets
        .iter()
        .map(|facts| {
            let named = facts
                .dependencies
                .iter()
                .filter(|dependency| dependency.standing != Standing::Done)
                .map(|dependency| dependency.ticket)
                .collect();
            (facts.ticket, named)
        })
        .collect()
}

/// Why a dependency can never complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display)]
pub enum DeadReason {
    /// It is closed as not planned or as a duplicate.
    #[strum(to_string = "closed as not planned")]
    ClosedNotPlanned,
    /// There is no such issue, or the number is a pull request.
    #[strum(to_string = "not a ticket")]
    Missing,
    /// It waits, through its own dependencies, on the ticket that waits on it.
    #[strum(to_string = "in a cycle")]
    Cycle,
}

/// A dependency of the ticket that can never complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadDependency {
    /// The dependency.
    pub dependency: TicketId,
    /// Why it never completes.
    pub reason: DeadReason,
}

/// The dependencies of `facts`'s ticket that can never complete, in the order named: closed as
/// not planned, missing, or open and leading back to the ticket through `graph` (a ticket that
/// names itself included). A done dependency is never dead.
#[must_use]
pub fn dead_dependencies(facts: &TicketFacts, graph: &DependencyGraph) -> Vec<DeadDependency> {
    facts
        .dependencies
        .iter()
        .filter_map(|dependency| {
            let reason = match dependency.standing {
                Standing::Done => return None,
                Standing::ClosedNotPlanned => DeadReason::ClosedNotPlanned,
                Standing::Missing => DeadReason::Missing,
                Standing::Open if reaches(graph, dependency.ticket, facts.ticket) => {
                    DeadReason::Cycle
                }
                Standing::Open => return None,
            };
            Some(DeadDependency {
                dependency: dependency.ticket,
                reason,
            })
        })
        .collect()
}

/// Whether `target` is `from` or a dependency of it, directly or through others, in `graph`.
fn reaches(graph: &DependencyGraph, from: TicketId, target: TicketId) -> bool {
    let mut seen = BTreeSet::new();
    let mut pending = vec![from];
    while let Some(at) = pending.pop() {
        if at == target {
            return true;
        }
        if seen.insert(at) {
            pending.extend(graph.get(&at).into_iter().flatten().copied());
        }
    }
    false
}

#[cfg(test)]
mod tests;
