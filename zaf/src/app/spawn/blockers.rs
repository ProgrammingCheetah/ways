//! The blockers a task body names, and the ones the spawner can never see closed.
//!
//! A blocker closes when its issue closes as completed, or is labelled `done`;
//! no label has to follow the close by hand. Three kinds never close: a cycle
//! among open tasks (each waits for the next), a number that is not an issue,
//! and an issue closed as not planned or as a duplicate (dropped, or folded into
//! a unit). [`read`] walks the `Blocked by:` lines from the ready tasks, asking the
//! board for each blocker's [`Standing`]; [`cycles`] and [`dead`] name what the
//! walk found. Both are pure, so the tick, the dry run and `zaf status` agree.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::{Standing, Ticket, TicketId};
use crate::ports::{Board, PortError};

/// The field of a task body that names its blockers.
const BLOCKED_BY: &str = "Blocked by:";

/// Each open task and the issues its `Blocked by:` line names.
pub type Graph = BTreeMap<TicketId, Vec<TicketId>>;

/// The issues a task body names on its `Blocked by:` line; `—` or `none` name none.
pub fn blockers(body: &str) -> Vec<TicketId> {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix(BLOCKED_BY))
        .flat_map(|rest| {
            rest.split(|character: char| character.is_whitespace() || character == ',')
        })
        .filter_map(|word| word.parse::<TicketId>().ok())
        .collect()
}

/// Why a blocker can never be seen closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum DeadReason {
    /// No such issue, or a pull request.
    #[strum(to_string = "not an issue")]
    NotAnIssue,
    /// The issue is closed as not planned or as a duplicate.
    #[strum(to_string = "closed as not planned")]
    ClosedNotPlanned,
}

/// A blocker named on a task that can never close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadBlocker {
    /// The blocker.
    pub blocker: TicketId,
    /// The task that names it.
    pub on: TicketId,
    /// Why it never closes.
    pub reason: DeadReason,
}

impl fmt::Display for DeadBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dead blocker {} on {}: {}",
            self.blocker, self.on, self.reason
        )
    }
}

/// What the walk of the `Blocked by:` lines found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blockers {
    /// The open tasks reached (the ready ones and every open blocker, transitively).
    pub graph: Graph,
    /// Blockers that are done: closed as completed, or labelled `done`.
    pub closed: BTreeSet<TicketId>,
    /// Blockers that never close.
    pub dead: BTreeMap<TicketId, DeadReason>,
}

/// Walks the blockers of `ready` through the board: each open blocker's own body is read in
/// turn, each other one is classed as done or dead.
pub fn read(board: &dyn Board, ready: &[Ticket]) -> Result<Blockers, PortError> {
    let mut found = Blockers::default();
    let mut pending: Vec<TicketId> = Vec::new();
    for ticket in ready {
        let named = blockers(ticket.body.as_str());
        pending.extend(named.iter().copied());
        found.graph.insert(ticket.id, named);
    }
    while let Some(number) = pending.pop() {
        if found.graph.contains_key(&number)
            || found.closed.contains(&number)
            || found.dead.contains_key(&number)
        {
            continue;
        }
        match board.standing(number)? {
            Standing::Done => {
                found.closed.insert(number);
            }
            Standing::ClosedNotPlanned => {
                found.dead.insert(number, DeadReason::ClosedNotPlanned);
            }
            Standing::Missing => {
                found.dead.insert(number, DeadReason::NotAnIssue);
            }
            Standing::Open => match board.task(number) {
                Ok(ticket) => {
                    let named = blockers(ticket.body.as_str());
                    pending.extend(named.iter().copied());
                    found.graph.insert(number, named);
                }
                Err(PortError::NotFound(_)) => {
                    found.dead.insert(number, DeadReason::NotAnIssue);
                }
                Err(error) => return Err(error),
            },
        }
    }
    Ok(found)
}

/// The blocker cycles among `graph`'s tasks, each as the tasks in order, starting at its lowest
/// number; the first task waits for the second and the last for the first.
///
/// Every task on a cycle is on one of the cycles returned.
pub fn cycles(graph: &Graph) -> Vec<Vec<TicketId>> {
    let mut found: Vec<Vec<TicketId>> = Vec::new();
    let mut covered: BTreeSet<TicketId> = BTreeSet::new();
    for &start in graph.keys() {
        if covered.contains(&start) {
            continue;
        }
        let mut path = vec![start];
        let mut visited = BTreeSet::new();
        if walk(graph, start, &mut path, &mut visited) {
            covered.extend(path.iter().copied());
            if let Some(lowest) = path
                .iter()
                .enumerate()
                .min_by_key(|(_, number)| **number)
                .map(|(index, _)| index)
            {
                path.rotate_left(lowest);
            }
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Extends `path` from its last task until a task names `path[0]`.
fn walk(
    graph: &Graph,
    start: TicketId,
    path: &mut Vec<TicketId>,
    visited: &mut BTreeSet<TicketId>,
) -> bool {
    let Some(at) = path.last().copied() else {
        return false;
    };
    let mut named = graph.get(&at).cloned().unwrap_or_default();
    named.sort();
    for next in named {
        if next == start {
            return true;
        }
        if graph.contains_key(&next) && visited.insert(next) {
            path.push(next);
            if walk(graph, start, path, visited) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// A cycle as `#a → #b → #a`.
pub fn render_cycle(cycle: &[TicketId]) -> String {
    let mut names: Vec<String> = cycle.iter().map(ToString::to_string).collect();
    if let Some(first) = cycle.first() {
        names.push(first.to_string());
    }
    names.join(" → ")
}

/// A cycle as plain numbers, for JSON.
pub fn cycle_numbers(cycle: &[TicketId]) -> Vec<u64> {
    cycle.iter().map(|task| task.get()).collect()
}

/// A dead blocker as JSON prints it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DeadLine {
    /// The blocker.
    pub blocker: u64,
    /// The task that names it.
    pub on: u64,
    /// Why it never closes: `not an issue` or `closed as not planned`.
    pub reason: String,
}

impl From<&DeadBlocker> for DeadLine {
    fn from(dead: &DeadBlocker) -> Self {
        Self {
            blocker: dead.blocker.get(),
            on: dead.on.get(),
            reason: dead.reason.to_string(),
        }
    }
}

/// The dead blockers named by the tasks of `graph`, by task then blocker.
pub fn dead(graph: &Graph, dead: &BTreeMap<TicketId, DeadReason>) -> Vec<DeadBlocker> {
    let mut found = Vec::new();
    for (&on, named) in graph {
        let mut named = named.clone();
        named.sort();
        named.dedup();
        for blocker in named {
            if let Some(&reason) = dead.get(&blocker) {
                found.push(DeadBlocker {
                    blocker,
                    on,
                    reason,
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
