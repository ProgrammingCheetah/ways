//! What a re-plan adds to the optimiser's input (D29, Q16, M10): the epic's filed tickets as fixed
//! nodes, the record number, and the check that a `Drop:` ticket has not started.
//!
//! An epic that has a plan record is planned as a graph. Its tickets that carry a `Node:` line
//! enter the input as they stand:
//!
//! - an open ticket is a fixed node (`filed: open`), with the sections' writes, provides and needs
//!   (a name a section implements is a need) and its `Turn` lines;
//! - a done ticket is a node with `filed: done`, which the optimiser takes out and whose provided
//!   names it counts as existing;
//! - a ticket closed as not planned is no node, and neither is a ticket of the older template,
//!   which holds no sections;
//! - a ticket the plan drops is no node either: it is closed once the new tickets are filed, so
//!   a name only it provided must be provided again, or the plan is `unprovided`.
//!
//! The plan's record number is the next one, so the signatures the plan lifts go into
//! `I@<epic>.<k>` and never into a filed interface ticket.
//!
//! Reading phase runs and pull requests is the filesystem's: a run folder of the task under
//! `.zaf/phases/` or a pull request record under `.zaf/slices/` means the ticket has started.
//! Both are read here, and nothing is written.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::app::actions::SliceRecords;
use crate::app::schedule::record::{PHASES_DIR, read_runs};
use crate::composition::STATE_DIR;
use crate::domain::plan::{
    Area, Behaviour, EpicKey, Filed, Name, Node, NodeId, NodeKind, PlannedTicket, Size, Weight,
};
use crate::domain::{Epic, Standing, TaskBody, TaskSize, TicketId};

/// The number of an epic's next plan record: 1 for an epic with none.
pub fn next_record(epic: &Epic) -> u32 {
    let next = epic.records.last().map_or(1, |last| last.record.k + 1);
    u32::try_from(next).unwrap_or(u32::MAX)
}

/// The fixed nodes of `epic`'s filed tickets, by ticket number, less the tickets in `dropped`.
///
/// None for an epic with no plan record: its tickets were not filed from a plan.
///
/// A ticket for which `own` holds is the plan's own, half-filed by the record being finished, and
/// is no fixed node.
pub fn fixed_nodes(
    epic: &Epic,
    key: &EpicKey,
    dropped: &[TicketId],
    own: &dyn Fn(&NodeId) -> bool,
) -> Vec<Node> {
    if epic.records.is_empty() {
        return Vec::new();
    }
    epic.tasks
        .iter()
        .filter(|task| !dropped.contains(&task.number))
        .filter_map(|task| {
            let filed = match task.standing {
                Standing::Open => Filed::Open,
                Standing::Done => Filed::Done,
                _ => return None,
            };
            match TaskBody::parse(task.body.as_str()) {
                Ok(TaskBody::Planned(ticket)) if !own(&ticket.node) => {
                    Some(node_of(&ticket, filed, key))
                }
                _ => None,
            }
        })
        .collect()
}

/// The node a filed ticket is: what its sections say, as the optimiser reads a plan.
fn node_of(ticket: &PlannedTicket, filed: Filed, key: &EpicKey) -> Node {
    let behaviours = match ticket.kind {
        NodeKind::Interface => signatures(ticket),
        NodeKind::Body => ticket
            .sections
            .iter()
            .map(|section| Behaviour {
                text: section.template.goal.clone(),
                size: Size::Weight(Weight(match section.template.size {
                    TaskSize::XS | TaskSize::S => 1,
                    _ => 2,
                })),
                writes: section.writes.clone(),
                provides: section.provides.clone(),
                needs: section.needs.union(&section.implements).cloned().collect(),
            })
            .collect(),
    };
    Node {
        id: ticket.node.clone(),
        kind: ticket.kind,
        filed: Some(filed),
        turns: ticket.turns.clone(),
        blocked_by: Vec::new(),
        epic: Some(key.clone()),
        behaviours,
    }
}

/// An interface ticket's behaviours, one for each signature, as the lift writes them.
fn signatures(ticket: &PlannedTicket) -> Vec<Behaviour> {
    let named: BTreeSet<Name> = if ticket.signatures.is_empty() {
        ticket
            .sections
            .iter()
            .flat_map(|section| section.provides.iter().cloned())
            .collect()
    } else {
        ticket.signatures.iter().map(|s| s.name.clone()).collect()
    };
    named
        .into_iter()
        .map(|name| Behaviour {
            text: format!("the signature of {name}"),
            size: Size::default(),
            writes: BTreeSet::from([Area::from(format!("sig:{name}"))]),
            provides: BTreeSet::from([name]),
            needs: BTreeSet::new(),
        })
        .collect()
}

/// Whether a ticket's node is one that record `k` of `epic` files from `plan`: a node the plan's
/// keys make ([`made_of`]), or the record's own interface ticket. Finishing a half-filed record
/// builds on the tickets it filed already, so these are no fixed nodes.
pub fn of_record(node: &NodeId, plan: &[NodeId], epic: TicketId, k: u64) -> bool {
    node.as_str() == format!("I@{}.{k}", epic.get()) || made_of(node, plan)
}

/// Whether the optimiser can give a node of `plan` the id `node`: a key of the plan as written, a
/// part of one (`A.a`), or a merge of such (`A+B`), which is every id it makes from keys (D9).
fn made_of(node: &NodeId, plan: &[NodeId]) -> bool {
    let held = |part: &str| {
        plan.iter().any(|id| {
            let id = id.as_str();
            part == id || part.rsplit_once('.').is_some_and(|(whole, _)| whole == id)
        })
    };
    node.as_str().split('+').all(held)
}

/// Why a ticket in the `Drop:` list cannot be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropBar {
    /// The ticket is not open: it is done, or closed already.
    NotOpen,
    /// A phase run of the ticket's is on record.
    PhaseRun,
    /// A pull request of the ticket's is on record.
    PullRequest,
    /// A record that says whether it has started cannot be read: it is not guessed.
    Unreadable(String),
}

impl fmt::Display for DropBar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DropBar::NotOpen => formatter.write_str("is not open"),
            DropBar::PhaseRun => formatter.write_str("has a phase run"),
            DropBar::PullRequest => formatter.write_str("has a pull request"),
            DropBar::Unreadable(why) => write!(formatter, "cannot be told started: {why}"),
        }
    }
}

/// Every ticket of the `Drop:` list that is no open ticket without a phase run or pull request,
/// each with what bars it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "Drop: {}: only an open ticket with no phase run and no pull request can be dropped",
    .0.iter().map(|(ticket, bar)| format!("{ticket} {bar}")).collect::<Vec<_>>().join("; ")
)]
pub struct DropBarred(pub Vec<(TicketId, DropBar)>);

/// Checks each ticket of `dropped` that is a task of `epic`: it must be open, have no phase run
/// folder under `.zaf/phases/` and no pull request record. A ticket that is no task of the epic is
/// left to `plan file`, which refuses it as no task of the epic.
///
/// # Errors
///
/// [`DropBarred`] naming every ticket that cannot be dropped.
pub fn check_drops(root: &Path, epic: &Epic, dropped: &[TicketId]) -> Result<(), DropBarred> {
    let state = root.join(STATE_DIR);
    let records = SliceRecords::new(root);
    let with_pr = records.tasks_with_pr();
    let mut barred = Vec::new();
    for ticket in dropped {
        let Some(task) = epic.tasks.iter().find(|task| task.number == *ticket) else {
            continue;
        };
        let bar = if task.standing != Standing::Open {
            Some(DropBar::NotOpen)
        } else if with_pr.contains(ticket) {
            Some(DropBar::PullRequest)
        } else {
            match read_runs(&state.join(PHASES_DIR), *ticket, &|_| false) {
                Ok(runs) if runs.is_empty() => None,
                Ok(_) => Some(DropBar::PhaseRun),
                Err(why) => Some(DropBar::Unreadable(why)),
            }
        };
        if let Some(bar) = bar {
            barred.push((*ticket, bar));
        }
    }
    if barred.is_empty() {
        Ok(())
    } else {
        Err(DropBarred(barred))
    }
}

/// The ids a ticket of `epic` holds already that the plan's keys can make again ([`made_of`]): the
/// key itself, or the merge of several (`A+B` of the keys A and B), each with its ticket. Filing
/// such a node would find its id filed and skip it, so the plan is refused before it is optimised
/// and names the ticket, whatever the ticket's standing: done, closed as not planned, dropped by
/// this plan, or open.
///
/// One case is left to the optimiser, which refuses it as a duplicate id: a key written again
/// that an open ticket, a fixed node of the input, holds as its whole id.
pub fn filed_ids(
    epic: &Epic,
    nodes: &[NodeId],
    dropped: &[TicketId],
    own: &dyn Fn(&NodeId) -> bool,
) -> Vec<(NodeId, TicketId)> {
    let mut found = Vec::new();
    for task in &epic.tasks {
        let Ok(TaskBody::Planned(ticket)) = TaskBody::parse(task.body.as_str()) else {
            continue;
        };
        let fixed = task.standing == Standing::Open && !dropped.contains(&task.number);
        let written = nodes.contains(&ticket.node);
        if !own(&ticket.node) && made_of(&ticket.node, nodes) && !(fixed && written) {
            found.push((ticket.node, task.number));
        }
    }
    found
}

/// Why a plan's nodes cannot be filed: an id they would take is a ticket's already.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{}: a ticket of the epic holds that id already (open, done, closed or dropped here), and the plan's keys would make it again: give the new tasks other keys",
    .0.iter().map(|(node, ticket)| format!("node {node} is {ticket}")).collect::<Vec<_>>().join("; ")
)]
pub struct NodesFiled(pub Vec<(NodeId, TicketId)>);

#[cfg(test)]
mod tests;
