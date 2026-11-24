//! The graph of an epic, read from its tickets (D18, I6): nothing is stored but the tickets, the
//! plan records and the filing marks, so the graph is derived on every read.
//!
//! Each ticket with a `Node:` line is one node, closed ones included. A name a section needs is
//! a wait on the ticket of the epic that provides it, and a name it implements is a wait on the
//! interface ticket whose signatures hold it; a `Turn:` line is a wait on its node, labelled
//! `turn:<area>`. A need no ticket of the epic provides makes no wait: the code provides it (D18),
//! or another epic does, which a `Waits on:` line says. The waits are then thinned as the
//! optimiser thins them: one stays between two nodes, with the greatest name. A ticket closed as
//! not planned provides a name only while no other ticket of the epic does, so a name a re-plan
//! provides again is waited on from its new ticket alone (D29).
//!
//! A `Waits on:` line is a kept wait, never thinned (D13). `#<n>` resolves to ticket #n;
//! `epic #<n>` resolves, among the release's epics, to that epic's interface ticket whose
//! signatures hold the name, else its ticket that provides it, and stays a wait on the epic
//! while it has filed no such ticket (Q10, Q19). A wait on a ticket closed as completed before
//! the plan record was posted is dropped, since the name was code by then (D29); one on a
//! ticket closed as not planned stays, a dead wait the scheduler raises.
//!
//! A ticket with no `Node:` line is one node whose id is its number (D4): it provides `#<n>` and
//! needs each ticket its `Blocked by` line names. A line naming an epic of the release is a need
//! of each task of that epic that counts, open or done; an epic with no task yet stays a wait on
//! itself. A number that is no node of the epic is a kept wait, as written. `#` names have no
//! signature, so nothing old is lifted or rewritten. A ticket whose body does not parse is left
//! out: `zaf board lint` names what is wrong with it.
//!
//! [`PlanGraph::before`] gives the other graph (I6): the epic's plan records as the planner wrote
//! them, with no ticket and no state, so a node there is planned and none is filed.
//!
//! Every list is sorted by natural id and built of ordered collections, so the same tickets
//! listed in another order read as the same graph (D16).

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use super::optimise::Working;
use super::optimise::derive::derive;
use super::optimise::thin::thinned;
use super::record::{GraphHash, PlanRecord};
use super::text::{PlanFaults, parse_plan};
use super::{Area, Name, NodeId, NodeKind, Options, Order, PlannedTicket, WaitOn, Weight};
use crate::domain::task::TaskBody;
use crate::domain::{BodyHash, Epic, EpicTask, Standing, TicketId};

/// A node of an epic's graph: one ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    /// Its id, as its `Node:` line says, or its number for an older ticket.
    pub id: NodeId,
    /// An interface ticket or a body.
    pub kind: NodeKind,
    /// The ticket that holds it and where it stands; `None` for a node as a plan record wrote
    /// it, which no ticket holds yet.
    pub filed: Option<Filed>,
    /// What its sections weigh together (D10); a size past the limit counts as over it.
    pub weight: Weight,
    /// The areas its sections write.
    pub writes: BTreeSet<Area>,
}

/// A node's ticket and where the ticket stands: both, or the node is not filed at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filed {
    /// The ticket that holds the node.
    pub ticket: TicketId,
    /// Where the ticket stands: open, done, or closed as not planned.
    pub standing: Standing,
    /// The hash of the ticket's body, which `PlanGraph::hash` holds.
    pub body: BodyHash,
}

/// The tickets one plan record dropped, kept as data: `before` takes no node out for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// The record's number.
    pub record: u64,
    /// The tickets, as the record lists them.
    pub tickets: Vec<TicketId>,
}

/// A plan record whose plan does not read: its number and every fault found.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("plan record {record} does not parse: {faults}")]
pub struct BadRecord {
    /// The record's number.
    pub record: u64,
    /// What is wrong with its plan.
    pub faults: PlanFaults,
}

/// What a `Waits on:` line resolves to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaitTarget {
    /// A ticket, with its node when it is a planned one the read could see.
    Ticket {
        /// The ticket.
        ticket: TicketId,
        /// Its node, from its `Node:` line.
        node: Option<NodeId>,
        /// Whether it is an interface ticket, which holds signatures (D13).
        interface: bool,
    },
    /// The epic itself: it has filed no ticket that holds the name yet.
    Epic(TicketId),
}

/// A kept wait (D13): the `Waits on:` line as written, beside what it resolves to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeptRead {
    /// The node of this epic that waits.
    pub to: NodeId,
    /// The name the wait is on.
    pub name: Name,
    /// What the line names, as written.
    pub line: WaitOn,
    /// What the line resolves to now.
    pub on: WaitTarget,
}

/// An epic's graph as its tickets say it (I6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanGraph {
    /// The nodes, by natural id.
    pub nodes: Vec<PlanNode>,
    /// The waits between them, thinned.
    pub orders: BTreeSet<Order>,
    /// The waits on other epics' tasks, as written and as resolved; never thinned.
    pub kept: BTreeSet<KeptRead>,
    /// Whether the epic's last plan record has no filing mark: a plan filed in part.
    pub half_filed: bool,
    /// The tickets each plan record dropped, in record order; empty for a graph read from
    /// tickets. A view prints it ("record 2 dropped #4"); taking those tickets out is `epic
    /// check`'s (D29).
    pub dropped: Vec<Dropped>,
    /// Every `Waits on:` line of the epic as written, whatever it resolves to, so a wait dropped
    /// because its ticket closed still counts and a state change never moves the hash; empty for
    /// a graph read from plan records. Public so that a graph can be built by hand, as the other
    /// fields allow; `PlanGraph::read` is what fills it.
    pub written: BTreeSet<(NodeId, Name, WaitOn)>,
}

/// What a ticket says about what it waits on.
enum Body {
    /// A planned ticket: its sections name what it provides and needs.
    Planned(PlannedTicket),
    /// A ticket of the older template: what its `Blocked by` line names.
    Older(Vec<TicketId>),
}

/// A ticket's node with the body that holds what it waits on.
struct Planned {
    node: PlanNode,
    /// The number of the ticket that holds the node.
    ticket: TicketId,
    body: Body,
}

fn read_task(task: &EpicTask) -> Option<Planned> {
    let (id, kind, body) = match TaskBody::parse(task.body.as_str()).ok()? {
        TaskBody::Planned(ticket) => (ticket.node.clone(), ticket.kind, Body::Planned(ticket)),
        TaskBody::Template(template) => (
            NodeId::from(task.number.get().to_string().as_str()),
            NodeKind::Body,
            Body::Older(template.blocked_by),
        ),
    };
    // A ticket of the older template has no sections: it weighs nothing and writes no area.
    let (weight, writes) = match &body {
        Body::Planned(ticket) => (
            ticket.weight(),
            ticket
                .sections
                .iter()
                .flat_map(|section| section.writes.iter().cloned())
                .collect(),
        ),
        Body::Older(_) => (0, BTreeSet::new()),
    };
    Some(Planned {
        ticket: task.number,
        node: PlanNode {
            id,
            kind,
            filed: Some(Filed {
                ticket: task.number,
                standing: task.standing,
                body: BodyHash::of(&task.body),
            }),
            weight: Weight(weight),
            writes,
        },
        body,
    })
}

/// The label of a wait that a turn gives: the area it is on, as `turn:<area>`.
fn turn_name(area: &impl std::fmt::Display) -> Name {
    Name::from(format!("turn:{area}"))
}

/// The name an older ticket provides, and a wait on it is labelled with: its number.
fn number_name(ticket: TicketId) -> Name {
    Name::from(ticket.to_string())
}

/// What a `Blocked by` entry stands for: each task of the epic it names that counts, open or
/// done, and never `node` itself; the epic itself when it has no task yet; or the entry as a
/// ticket when it names no epic of the release.
fn blockers(entry: TicketId, node: TicketId, epics: &[&Epic]) -> Vec<WaitOn> {
    let Some(epic) = epics.iter().find(|epic| epic.id == entry) else {
        return vec![WaitOn::Task(entry)];
    };
    if epic.tasks.is_empty() {
        return vec![WaitOn::Epic(entry)];
    }
    epic.tasks
        .iter()
        .filter(|task| matches!(task.standing, Standing::Open | Standing::Done))
        .map(|task| task.number)
        .filter(|number| *number != node)
        .map(WaitOn::Task)
        .collect()
}

impl PlanGraph {
    /// A graph of the given parts, not half filed, with no dropped ticket and no written wait.
    pub fn of(nodes: Vec<PlanNode>, orders: BTreeSet<Order>, kept: BTreeSet<KeptRead>) -> Self {
        Self {
            nodes,
            orders,
            kept,
            half_filed: false,
            dropped: Vec::new(),
            written: BTreeSet::new(),
        }
    }

    /// The graph of `epic` read from its tickets, closed ones included.
    ///
    /// `release` holds the release's open epics, which the waits across epics are resolved in, and
    /// which a `Blocked by` entry naming an epic is read from.
    pub fn read(epic: &Epic, release: &[Epic]) -> Self {
        let mut planned: Vec<Planned> = epic.tasks.iter().filter_map(read_task).collect();
        planned.sort_by(|a, b| {
            a.node.id.cmp(&b.node.id).then_with(|| {
                let ticket = |p: &Planned| p.node.filed.as_ref().map(|filed| filed.ticket);
                ticket(a).cmp(&ticket(b))
            })
        });
        let nodes: Vec<PlanNode> = planned.iter().map(|p| p.node.clone()).collect();
        let ids: Vec<NodeId> = nodes.iter().map(|node| node.id.clone()).collect();
        let providers = providers(&planned);
        let by_ticket: BTreeMap<TicketId, NodeId> = planned
            .iter()
            .map(|p| (p.ticket, p.node.id.clone()))
            .collect();
        let epics: Vec<&Epic> = release.iter().chain(std::iter::once(epic)).collect();
        let mut orders = BTreeSet::new();
        let mut older = BTreeSet::new();
        for Planned {
            node,
            ticket: held,
            body,
        } in &planned
        {
            match body {
                Body::Planned(ticket) => {
                    let needs = ticket
                        .sections
                        .iter()
                        .flat_map(|section| section.needs.iter().chain(section.implements.iter()));
                    for name in needs {
                        for from in providers.get(name).into_iter().flatten() {
                            if *from != node.id {
                                orders.insert(Order {
                                    from: from.clone(),
                                    to: node.id.clone(),
                                    needs: name.clone(),
                                });
                            }
                        }
                    }
                    for turn in &ticket.turns {
                        if turn.0 != node.id && ids.contains(&turn.0) {
                            orders.insert(Order {
                                from: turn.0.clone(),
                                to: node.id.clone(),
                                needs: turn_name(&turn.1),
                            });
                        }
                    }
                }
                Body::Older(blocked_by) => {
                    let entries = blocked_by
                        .iter()
                        .flat_map(|entry| blockers(*entry, *held, &epics));
                    for blocker in entries {
                        let (number, on) = match blocker {
                            WaitOn::Task(number) => (number, None),
                            WaitOn::Epic(number) => (number, Some(WaitTarget::Epic(number))),
                        };
                        if number == *held {
                            continue;
                        }
                        let from = match on {
                            Some(_) => None,
                            None => by_ticket.get(&number),
                        };
                        let name = number_name(number);
                        match from {
                            Some(from) => {
                                orders.insert(Order {
                                    from: from.clone(),
                                    to: node.id.clone(),
                                    needs: name,
                                });
                            }
                            None => {
                                let on = on.unwrap_or_else(|| {
                                    let (node, interface) = ticket_numbered(epic, release, number)
                                        .and_then(node_of)
                                        .unzip();
                                    WaitTarget::Ticket {
                                        ticket: number,
                                        node,
                                        interface: interface.unwrap_or(false),
                                    }
                                });
                                older.insert(KeptRead {
                                    to: node.id.clone(),
                                    name,
                                    line: blocker,
                                    on,
                                });
                            }
                        }
                    }
                }
            }
        }
        Self {
            orders: thinned(&ids, &orders),
            nodes,
            kept: kept(epic, release, &planned, older),
            half_filed: epic
                .records
                .last()
                .is_some_and(|record| record.mark.is_none()),
            // Only a planned ticket has `Waits on:` lines; an older one's `Blocked by` lines
            // are in its body's hash.
            written: planned
                .iter()
                .filter_map(|p| match &p.body {
                    Body::Planned(ticket) => Some((&p.node.id, &ticket.waits)),
                    Body::Older(_) => None,
                })
                .flat_map(|(id, waits)| {
                    waits
                        .iter()
                        .map(move |w| (id.clone(), w.name.clone(), w.on.clone()))
                })
                .collect(),
            dropped: Vec::new(),
        }
    }

    /// The graph as the planner wrote it, from an epic's plan records (I6): every record's plan
    /// parsed as written (I1), no ticket and no state on a node, and the waits the needs and
    /// provides of each plan derive, not thinned and not optimised.
    ///
    /// The waits of a record are those of its own plan: a name one record provides makes no wait
    /// for a node of another. A node a record drops is not taken out, whatever the dropped list
    /// says (ruling #1350); each record's dropped list is carried in `dropped`. `kept` is empty
    /// and `half_filed` false: a record says nothing of other epics or of filing. A plan whose
    /// waits go round is given as it is written; a reader that lays out layers asks the graph
    /// for a cycle first.
    ///
    /// The records are untrusted text from a model: a record whose plan does not parse is a
    /// [`BadRecord`] naming its number and its faults, the first such in the order given.
    pub fn before(records: &[PlanRecord]) -> Result<Self, BadRecord> {
        let mut nodes = Vec::new();
        let mut orders = BTreeSet::new();
        let mut dropped = Vec::new();
        for record in records {
            let parsed = parse_plan(&record.plan).map_err(|faults| BadRecord {
                record: record.k,
                faults,
            })?;
            nodes.extend(parsed.input.nodes.iter().map(|node| {
                PlanNode {
                    id: node.id.clone(),
                    kind: node.kind,
                    filed: None,
                    weight: Weight(
                        node.behaviours
                            .iter()
                            .map(|behaviour| behaviour.size.weight().map_or(u32::MAX, |w| w.0))
                            .fold(0, u32::saturating_add),
                    ),
                    writes: node
                        .behaviours
                        .iter()
                        .flat_map(|behaviour| behaviour.writes.iter().cloned())
                        .collect(),
                }
            }));
            let mut working = Working {
                plan: parsed.input,
                options: Options::default(),
                orders: BTreeSet::new(),
            };
            derive(&mut working);
            orders.extend(working.orders);
            dropped.push(Dropped {
                record: record.k,
                tickets: record.dropped.clone(),
            });
        }
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self {
            nodes,
            orders,
            kept: BTreeSet::new(),
            half_filed: false,
            dropped,
            written: BTreeSet::new(),
        })
    }

    /// The sha256 of the graph's canonical form (D18, I6): each node's id, kind, ticket number
    /// and body hash, then the waits inside the epic, then the `Waits on:` lines as written,
    /// each list sorted by natural id.
    ///
    /// A body edit moves it. A state change, a closed ticket and another epic's planning do not:
    /// no standing, no closing time and nothing a wait resolves to is in it.
    pub fn hash(&self) -> GraphHash {
        let mut form = String::new();
        for node in &self.nodes {
            let kind = match node.kind {
                NodeKind::Body => "body",
                NodeKind::Interface => "interface",
            };
            let id = node.id.as_str();
            match &node.filed {
                Some(filed) => form.push_str(&format!(
                    "node {id} {kind} {} {}\n",
                    filed.ticket, filed.body
                )),
                None => form.push_str(&format!("node {id} {kind} unfiled\n")),
            }
        }
        for order in &self.orders {
            form.push_str(&format!(
                "order {} {} {}\n",
                order.from.as_str(),
                order.to.as_str(),
                order.needs.as_str()
            ));
        }
        for (to, name, on) in &self.written {
            form.push_str(&format!("wait {} {} {on}\n", to.as_str(), name.as_str()));
        }
        GraphHash::of(&form)
    }
}

/// Each name with the nodes of the epic that give it: those that provide it, an interface
/// ticket whose signatures hold it, and an older ticket for its number.
///
/// A ticket closed as not planned gives a name only while no other ticket does (D29): once a
/// re-plan provides the name again, under any key, the dropped ticket is no provider, so nothing
/// waits on it. With no other provider it stays one, the dead wait the scheduler raises (D19).
fn providers(planned: &[Planned]) -> BTreeMap<Name, BTreeSet<NodeId>> {
    // Each node that gives the name, with whether a ticket that still counts holds it.
    let mut given: BTreeMap<Name, BTreeMap<NodeId, bool>> = BTreeMap::new();
    for Planned {
        node,
        ticket: held,
        body,
    } in planned
    {
        let counts = node
            .filed
            .as_ref()
            .is_none_or(|filed| filed.standing != Standing::ClosedNotPlanned);
        let mut give = |name: Name| {
            *given
                .entry(name)
                .or_default()
                .entry(node.id.clone())
                .or_default() |= counts;
        };
        let Body::Planned(ticket) = body else {
            give(number_name(*held));
            continue;
        };
        let provided = ticket.sections.iter().flat_map(|s| s.provides.iter());
        let signed = ticket.signatures.iter().map(|signature| &signature.name);
        for name in provided.chain(signed) {
            give(name.clone());
        }
    }
    given
        .into_iter()
        .map(|(name, nodes)| {
            let replaced = nodes.values().any(|counts| *counts);
            let nodes = nodes
                .into_iter()
                .filter(|(_, counts)| *counts || !replaced)
                .map(|(node, _)| node)
                .collect();
            (name, nodes)
        })
        .collect()
}

/// The posting time of the plan record that filed the epic's tickets.
///
/// The board does not say which record filed a ticket, so this is the epic's first record: a
/// wait closed before it was closed before every record, which is the one safe reading. `None`
/// when the epic has no record, or the board does not time its comments.
fn filed_at(epic: &Epic) -> Option<SystemTime> {
    epic.records.first().and_then(|record| record.posted_at)
}

/// Whether `ticket` was closed as completed before `posted`: the name it gave was code by then.
fn done_before(ticket: &EpicTask, posted: Option<SystemTime>) -> bool {
    match (ticket.standing, ticket.closed_at, posted) {
        (Standing::Done, Some(closed), Some(posted)) => closed < posted,
        _ => false,
    }
}

/// The ticket of `epic` that holds `name` for another epic's wait: the interface ticket whose
/// signatures hold it, else the ticket that provides it. A ticket closed as not planned comes
/// last, so a re-plan that provides the name again is the one waited on; ties go to the lowest
/// node id. `plan check` reads it too, to name the provider of another epic's name.
pub fn holder<'a>(epic: &'a Epic, name: &Name) -> Option<(&'a EpicTask, NodeId, bool)> {
    let mut signed = Vec::new();
    let mut provided = Vec::new();
    for task in &epic.tasks {
        let Ok(TaskBody::Planned(ticket)) = TaskBody::parse(task.body.as_str()) else {
            continue;
        };
        let key = (
            task.standing == Standing::ClosedNotPlanned,
            ticket.node.clone(),
            task.number,
        );
        if ticket.kind == NodeKind::Interface && ticket.signatures.iter().any(|s| s.name == *name) {
            signed.push((key, task, ticket.node.clone()));
        } else if ticket.sections.iter().any(|s| s.provides.contains(name)) {
            provided.push((key, task, ticket.node.clone()));
        }
    }
    let interface = !signed.is_empty();
    let mut found = if interface { signed } else { provided };
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
        .into_iter()
        .next()
        .map(|(_, task, node)| (task, node, interface))
}

/// The ticket numbered `number`, in this epic or in the release's.
fn ticket_numbered<'a>(
    epic: &'a Epic,
    release: &'a [Epic],
    number: TicketId,
) -> Option<&'a EpicTask> {
    std::iter::once(epic)
        .chain(release)
        .flat_map(|e| &e.tasks)
        .find(|task| task.number == number)
}

/// The node a planned ticket stands for, and whether it is an interface ticket.
fn node_of(task: &EpicTask) -> Option<(NodeId, bool)> {
    match TaskBody::parse(task.body.as_str()) {
        Ok(TaskBody::Planned(ticket)) => Some((ticket.node, ticket.kind == NodeKind::Interface)),
        _ => None,
    }
}

/// The kept waits of `epic`: each `Waits on:` line of its planned tickets, resolved by name,
/// less the waits on a ticket done before the plan record was posted.
///
/// `older` holds the waits of the older tickets, which a `Blocked by` line gave.
fn kept(
    epic: &Epic,
    release: &[Epic],
    planned: &[Planned],
    older: BTreeSet<KeptRead>,
) -> BTreeSet<KeptRead> {
    let posted = filed_at(epic);
    let mut out = older;
    for Planned { node, body, .. } in planned {
        let Body::Planned(ticket) = body else {
            continue;
        };
        for wait in &ticket.waits {
            let (provider, on) = match &wait.on {
                WaitOn::Task(number) => {
                    let task = ticket_numbered(epic, release, *number);
                    let (node, interface) = task.and_then(node_of).unzip();
                    let on = WaitTarget::Ticket {
                        ticket: *number,
                        node,
                        interface: interface.unwrap_or(false),
                    };
                    (task, on)
                }
                WaitOn::Epic(number) => {
                    let found = release
                        .iter()
                        .find(|other| other.id == *number && other.id != epic.id)
                        .and_then(|other| holder(other, &wait.name));
                    match found {
                        Some((task, node, interface)) => (
                            Some(task),
                            WaitTarget::Ticket {
                                ticket: task.number,
                                node: Some(node),
                                interface,
                            },
                        ),
                        None => (None, WaitTarget::Epic(*number)),
                    }
                }
            };
            if provider.is_some_and(|task| done_before(task, posted)) {
                continue;
            }
            out.insert(KeptRead {
                to: node.id.clone(),
                name: wait.name.clone(),
                line: wait.on.clone(),
                on,
            });
        }
    }
    out
}
