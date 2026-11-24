//! O3's structural check on an epic's graph (D24, I13): the rules `zaf plan verify` holds a graph
//! to, each failure named in one line.
//!
//! - Each wait on a name with a signature goes through an interface, kept waits included.
//! - Every other wait is on a name with no signature, or is a turn.
//! - No body node passes the limit.
//! - There is no cycle.
//! - No two writers of one area are unordered.
//!
//! - The optimiser run on the graph changes nothing (the rerun, below).
//!
//! One more rule holds the graph to the epic as zaf filed it (D17, D18): its hash is the one the
//! last plan record's filing mark holds. A last record with no mark is half filed, a failure of
//! its own. This one is pure: it reads the graph (I6), the release's vocabulary and the plan
//! records with their marks (I8), and nothing else.
//!
//! A name has a signature when the vocabulary gives it one or a plan record added it as one
//! (D6); the same holds for a name with none, and a filed task's number `#<n>` never has one
//! (D4). A name in the vocabulary's `existing` list is code
//! already and has no wait to go through an interface. Only open tickets count for the limit and
//! for the writers of an area: a done one is code by now, and one closed as not planned was
//! folded into another. A wait of a kept line that still names an epic (it has filed no ticket
//! that holds the name) is not judged: there is no ticket yet to be an interface or not.
//!
//! The rerun builds f's input from the graph as a re-plan does (D29): an open ticket is a fixed
//! node, with its waits as the names it needs and provides and its turns as `Turn` lines; a done
//! ticket is code by now and a ticket closed as not planned was folded away, so neither is in the
//! input and no wait on one is judged; a node no ticket holds (`filed` is `None`, as in a graph
//! of a plan not filed) is a new task. f runs with production options, and what it gives back is
//! compared with the graph: each node it adds, drops or renames, each wait it adds or drops and
//! each kept wait it adds or drops. Nodes that change together (two merged into one, one split
//! into two, one renamed) are one failure, and a wait that touches a node that changed is part
//! of that failure, not one more. A graph f refuses is given each refusal, except a cycle, which
//! the fourth rule names. A graph read from tickets holds only fixed nodes, so f can only thin a
//! wait it left in or refuse; a node no ticket holds is where it merges, splits, orders and
//! lifts.
//!
//! One failure is given for a wait: a wait on a name with a signature that is not through an
//! interface breaks the first rule only. A cycle is named once, by the group of nodes whose first
//! comes first, and the writers of an area are not looked at while the graph has one, since the
//! order between two nodes is not defined then.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::body::BODY_WEIGHT_LIMIT;
use super::graph::Dag;
use super::names::Added;
use super::optimise::optimise;
use super::read::{PlanGraph, PlanNode, WaitTarget};
use super::record::GraphHash;
use super::{
    Area, Behaviour, Filed, Interfaces, KeptWait, Lift, Name, Node, NodeId, NodeKind, Options,
    Order, PlanInput, RefusalCode, Size, Turn, Vocabulary, Weight,
};
use crate::domain::{EpicRecord, Standing, TicketId};

/// The prefix of the label of a wait that a turn gives (D12).
const TURN_PREFIX: &str = "turn:";

/// The options the verbs run the optimiser with in production (I4): `plan check`'s own.
const PRODUCTION: Options = Options {
    lift: Lift::All,
    interfaces: Interfaces::One,
};

/// The rule a [`ShapeFailure`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ShapeRule {
    /// A wait on a name with a signature that is not through an interface.
    SignatureThroughInterface,
    /// A wait, not through an interface, on a name that is not known to have no signature.
    UnsignedWait,
    /// A body node over the limit.
    BodyLimit,
    /// Waits that go round.
    Cycle,
    /// Two writers of one area with no order between them.
    UnorderedWriters,
    /// A graph whose hash is not the one the last filing mark holds.
    GraphHash,
    /// The last plan record has no filing mark.
    HalfFiled,
    /// A node or wait the optimiser would add, drop or rename, or a graph it refuses.
    Rerun,
}

/// Whether the rerun adds a wait or drops one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Change {
    /// The graph does not hold it and the rerun makes it.
    Add,
    /// The graph holds it and the rerun leaves it out.
    Drop,
}

/// What a wait is on: a node of the graph, or a ticket or an epic of the board.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Waited {
    /// A node of this epic's graph.
    Node(NodeId),
    /// A ticket of another epic, which no node of this graph is.
    Ticket(TicketId),
    /// An epic that has filed no ticket that holds the name.
    Epic(TicketId),
}

impl fmt::Display for Waited {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(node) => write!(formatter, "{node}"),
            Self::Ticket(ticket) => write!(formatter, "{ticket}"),
            Self::Epic(epic) => write!(formatter, "epic {epic}"),
        }
    }
}

/// One way a graph breaks O3's structural rules (I13). Its `Display` is one line that names the
/// rule, the node or nodes, and the name or the area.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShapeFailure {
    /// A node waits for a name with a signature on something that is not an interface.
    SignatureThroughInterface {
        /// The node that waits.
        to: NodeId,
        /// What it waits on.
        on: Waited,
        /// The name.
        name: Name,
    },
    /// A node waits, not through an interface, on a name that is not known to have no signature.
    UnsignedWait {
        /// The node that waits.
        to: NodeId,
        /// What it waits on.
        on: Waited,
        /// The name.
        name: Name,
    },
    /// A body node weighs more than the limit.
    BodyLimit {
        /// The node.
        node: NodeId,
        /// What it weighs.
        weight: Weight,
        /// The limit.
        limit: Weight,
    },
    /// The waits of these nodes go round.
    Cycle {
        /// The nodes, sorted.
        nodes: Vec<NodeId>,
    },
    /// Two nodes write one area and neither is before the other.
    UnorderedWriters {
        /// The node with the lower id.
        first: NodeId,
        /// The other.
        second: NodeId,
        /// The lowest area they both write.
        area: Area,
    },
    /// The graph is not as zaf filed it: its hash is not the last filing mark's.
    GraphHash {
        /// The number of the record the mark is for.
        record: u64,
        /// The hash the mark holds.
        marked: GraphHash,
        /// The hash of the graph as it reads now.
        graph: GraphHash,
    },
    /// The last plan record has no filing mark: the filing did not finish.
    HalfFiled {
        /// The number of the last record.
        record: u64,
    },
    /// The rerun replaces `dropped` with `added`. A merge has two dropped and one added, a split
    /// the other way round, a rename one of each, and a node it makes or takes out one side.
    RerunNodes {
        /// The nodes of the graph the rerun does not give back, sorted.
        dropped: Vec<NodeId>,
        /// The nodes the rerun gives that the graph does not hold, sorted.
        added: Vec<NodeId>,
    },
    /// The rerun adds or drops a wait between nodes it leaves as they are.
    RerunWait {
        /// Add or drop.
        change: Change,
        /// The wait.
        wait: Order,
    },
    /// The rerun adds or drops a wait on another epic's task of a node it leaves as it is.
    RerunKept {
        /// Add or drop.
        change: Change,
        /// The wait.
        wait: KeptWait,
    },
    /// The optimiser refuses the graph.
    RerunRefused {
        /// The refusal's code.
        code: RefusalCode,
        /// The node, name or nodes it is about.
        detail: String,
    },
}

impl ShapeFailure {
    /// The rule it breaks.
    pub fn rule(&self) -> ShapeRule {
        match self {
            Self::SignatureThroughInterface { .. } => ShapeRule::SignatureThroughInterface,
            Self::UnsignedWait { .. } => ShapeRule::UnsignedWait,
            Self::BodyLimit { .. } => ShapeRule::BodyLimit,
            Self::Cycle { .. } => ShapeRule::Cycle,
            Self::UnorderedWriters { .. } => ShapeRule::UnorderedWriters,
            Self::GraphHash { .. } => ShapeRule::GraphHash,
            Self::HalfFiled { .. } => ShapeRule::HalfFiled,
            Self::RerunNodes { .. }
            | Self::RerunWait { .. }
            | Self::RerunKept { .. }
            | Self::RerunRefused { .. } => ShapeRule::Rerun,
        }
    }

    /// The nodes of this graph it names: the node that waits (and the node it waits on, when it
    /// is one), the body over the limit, the nodes of the cycle, the two writers, or the nodes a
    /// rerun drops and adds; none for a failure of the hash, which is the whole graph's, nor for a
    /// refusal.
    pub fn nodes(&self) -> Vec<&NodeId> {
        match self {
            Self::SignatureThroughInterface { to, on, .. } | Self::UnsignedWait { to, on, .. } => {
                let mut nodes = vec![to];
                if let Waited::Node(from) = on {
                    nodes.push(from);
                }
                nodes
            }
            Self::BodyLimit { node, .. } => vec![node],
            Self::Cycle { nodes } => nodes.iter().collect(),
            Self::UnorderedWriters { first, second, .. } => vec![first, second],
            Self::RerunNodes { dropped, added } => dropped.iter().chain(added).collect(),
            Self::RerunWait { wait, .. } => vec![&wait.from, &wait.to],
            Self::RerunKept { wait, .. } => vec![&wait.to],
            Self::GraphHash { .. } | Self::HalfFiled { .. } | Self::RerunRefused { .. } => {
                Vec::new()
            }
        }
    }
}

/// The ids as a phrase: `A`, `A and B`, `A, B and C`.
fn listed(ids: &[NodeId]) -> String {
    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    match ids.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

impl fmt::Display for ShapeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = self.rule();
        match self {
            Self::SignatureThroughInterface { to, on, name } => write!(
                formatter,
                "{rule}: {to} waits on {on} for {name}, a name with a signature, and {on} is not \
                 an interface ticket"
            ),
            Self::UnsignedWait { to, on, name } => write!(
                formatter,
                "{rule}: {to} waits on {on} for {name}, which is not a turn or a name with no \
                 signature"
            ),
            Self::BodyLimit {
                node,
                weight,
                limit,
            } => write!(
                formatter,
                "{rule}: {node} weighs {weight}, over the limit of {limit}"
            ),
            Self::Cycle { nodes } => {
                let nodes: Vec<String> = nodes.iter().map(ToString::to_string).collect();
                write!(
                    formatter,
                    "{rule}: the waits of {} go round",
                    nodes.join(", ")
                )
            }
            Self::UnorderedWriters {
                first,
                second,
                area,
            } => write!(
                formatter,
                "{rule}: {first} and {second} both write {area} and neither waits for the other"
            ),
            Self::GraphHash {
                record,
                marked,
                graph,
            } => write!(
                formatter,
                "{rule}: the graph hashes to {graph}, not to {marked}, which the filing mark of \
                 plan record {record} holds: the epic is not as zaf filed it"
            ),
            Self::HalfFiled { record } => write!(
                formatter,
                "{rule}: plan record {record} has no filing mark, so it is half filed: run \
                 `plan file` with empty stdin to finish it"
            ),
            Self::RerunNodes { dropped, added } => match (dropped.is_empty(), added.is_empty()) {
                (false, false) => write!(
                    formatter,
                    "{rule}: the optimiser would replace {} with {}",
                    listed(dropped),
                    listed(added)
                ),
                (false, true) => write!(
                    formatter,
                    "{rule}: the optimiser would drop {}",
                    listed(dropped)
                ),
                _ => write!(
                    formatter,
                    "{rule}: the optimiser would add {}",
                    listed(added)
                ),
            },
            Self::RerunWait { change, wait } => write!(
                formatter,
                "{rule}: the optimiser would {change} the wait of {} on {} for {}",
                wait.to, wait.from, wait.needs
            ),
            Self::RerunKept { change, wait } => write!(
                formatter,
                "{rule}: the optimiser would {change} the wait of {} on {} for {}, a wait on \
                 another epic",
                wait.to, wait.from, wait.needs
            ),
            Self::RerunRefused { code, detail } => write!(
                formatter,
                "{rule}: the optimiser refuses the graph: {code}: {detail}"
            ),
        }
    }
}

/// What the check knows of the names: which have a signature, and which are known to have none.
struct Names {
    signature: BTreeSet<Name>,
    unsigned: BTreeSet<Name>,
}

impl Names {
    fn of(vocabulary: &Vocabulary, records: &[EpicRecord]) -> Self {
        // What a record's `New names:` added lasts (D6), as a re-plan's check reads it too.
        let vocabulary =
            Added::by(records.iter().map(|entry| &entry.record)).onto(vocabulary.clone());
        Self {
            unsigned: vocabulary
                .no_signature
                .union(&vocabulary.existing)
                .cloned()
                .collect(),
            signature: vocabulary.signature,
        }
    }

    /// Whether a name is known to have no signature: one the lists say so, or a filed task's
    /// number `#<n>`, which never has one (D4).
    fn is_unsigned(&self, name: &Name) -> bool {
        self.unsigned.contains(name) || name.task().is_some()
    }
}

/// Whether the node is an open ticket: the only kind the limit and the writers concern.
fn is_open(node: &PlanNode) -> bool {
    node.filed
        .as_ref()
        .is_some_and(|filed| filed.standing == Standing::Open)
}

/// The first two rules, for every wait of the graph: those inside it, then the kept ones.
fn check_waits(graph: &PlanGraph, names: &Names, out: &mut Vec<ShapeFailure>) {
    let interface = |id: &NodeId| {
        graph
            .nodes
            .iter()
            .any(|node| node.id == *id && node.kind == NodeKind::Interface)
    };
    for order in &graph.orders {
        if order.needs.as_str().starts_with(TURN_PREFIX) {
            continue;
        }
        let failure = if names.signature.contains(&order.needs) {
            (!interface(&order.from)).then(|| ShapeFailure::SignatureThroughInterface {
                to: order.to.clone(),
                on: Waited::Node(order.from.clone()),
                name: order.needs.clone(),
            })
        } else {
            (!names.is_unsigned(&order.needs) && !interface(&order.from)).then(|| {
                ShapeFailure::UnsignedWait {
                    to: order.to.clone(),
                    on: Waited::Node(order.from.clone()),
                    name: order.needs.clone(),
                }
            })
        };
        out.extend(failure);
    }
    for kept in &graph.kept {
        let (on, through_interface) = match &kept.on {
            WaitTarget::Ticket {
                ticket,
                node,
                interface: holds_signatures,
            } => match node {
                Some(node) if graph.nodes.iter().any(|own| own.id == *node) => {
                    (Waited::Node(node.clone()), interface(node))
                }
                _ => (Waited::Ticket(*ticket), *holds_signatures),
            },
            WaitTarget::Epic(epic) => (Waited::Epic(*epic), false),
        };
        let to = kept.to.clone();
        let name = kept.name.clone();
        let failure = if names.signature.contains(&name) {
            // An epic that has filed no ticket yet gives nothing to judge.
            (!through_interface && !matches!(on, Waited::Epic(_)))
                .then_some(ShapeFailure::SignatureThroughInterface { to, on, name })
        } else {
            (!names.is_unsigned(&name) && !through_interface)
                .then_some(ShapeFailure::UnsignedWait { to, on, name })
        };
        out.extend(failure);
    }
}

/// The third rule: no open body weighs more than the limit.
fn check_limit(graph: &PlanGraph, out: &mut Vec<ShapeFailure>) {
    let limit = Weight(BODY_WEIGHT_LIMIT);
    for node in &graph.nodes {
        if is_open(node) && node.kind == NodeKind::Body && node.weight > limit {
            out.push(ShapeFailure::BodyLimit {
                node: node.id.clone(),
                weight: node.weight,
                limit,
            });
        }
    }
}

/// The fourth and fifth rules: no cycle, and an order between any two open writers of one area.
fn check_order(graph: &PlanGraph, out: &mut Vec<ShapeFailure>) {
    let ids: Vec<NodeId> = graph.nodes.iter().map(|node| node.id.clone()).collect();
    let edges: Vec<(NodeId, NodeId)> = graph
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    let dag = Dag::new(&ids, &edges);
    if let Some(nodes) = dag.cycle() {
        out.push(ShapeFailure::Cycle { nodes });
        return;
    }
    let closure = dag.closure();
    let reaches = |from: &NodeId, to: &NodeId| {
        closure
            .get(from)
            .is_some_and(|reached| reached.contains(to))
    };
    let writers: Vec<&PlanNode> = graph.nodes.iter().filter(|node| is_open(node)).collect();
    for (at, first) in writers.iter().enumerate() {
        for second in &writers[at + 1..] {
            let Some(area) = first.writes.intersection(&second.writes).next() else {
                continue;
            };
            if !reaches(&first.id, &second.id) && !reaches(&second.id, &first.id) {
                out.push(ShapeFailure::UnorderedWriters {
                    first: first.id.clone(),
                    second: second.id.clone(),
                    area: area.clone(),
                });
            }
        }
    }
}

/// How f takes a node of the graph (D29).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixing {
    /// An open ticket: a fixed node.
    Fixed,
    /// No ticket holds it: a new task.
    New,
    /// A done ticket (code now) or one closed as not planned: not in f's input.
    Out,
}

fn fixing(node: &PlanNode) -> Fixing {
    match &node.filed {
        None => Fixing::New,
        Some(filed) if filed.standing == Standing::Open => Fixing::Fixed,
        Some(_) => Fixing::Out,
    }
}

/// The node f's `outside` gives a kept wait as its source: the node of the other epic the line
/// resolves to, else the ticket, else the epic.
fn kept_source(on: &WaitTarget) -> NodeId {
    match on {
        WaitTarget::Ticket {
            node: Some(node), ..
        } => node.clone(),
        WaitTarget::Ticket { ticket, .. } => NodeId::from(Waited::Ticket(*ticket).to_string()),
        WaitTarget::Epic(epic) => NodeId::from(Waited::Epic(*epic).to_string()),
    }
}

/// What the graph says a node needs, provides and turns on, collected from its waits.
#[derive(Default)]
struct Ties {
    needs: BTreeSet<Name>,
    provides: BTreeSet<Name>,
    turns: Vec<Turn>,
}

/// How many behaviours f's input gives a node: a merged node `A+B` holds one for each part of its
/// id (D9), any other node one.
fn behaviour_count(node: &PlanNode) -> usize {
    match node.kind {
        NodeKind::Body => node.id.as_str().split('+').count(),
        NodeKind::Interface => 1,
    }
}

/// f's input for the rerun: the graph as D29 gives a re-plan's. An open ticket is fixed; a node no
/// ticket holds is new, its weight shared among its parts; a done ticket and one closed as not
/// planned are left out, with every wait on them. The names with no signature are the ones the
/// vocabulary and the plan records give.
fn rerun_input(graph: &PlanGraph, names: &Names) -> PlanInput {
    let taken: BTreeSet<&NodeId> = graph
        .nodes
        .iter()
        .filter(|node| fixing(node) != Fixing::Out)
        .map(|node| &node.id)
        .collect();
    let mut ties: BTreeMap<&NodeId, Ties> = BTreeMap::new();
    let mut outside = BTreeMap::new();
    for order in &graph.orders {
        if !taken.contains(&order.from) || !taken.contains(&order.to) {
            continue;
        }
        if let Some(area) = order.needs.as_str().strip_prefix(TURN_PREFIX) {
            ties.entry(&order.to)
                .or_default()
                .turns
                .push(Turn(order.from.clone(), Area::from(area)));
        } else {
            ties.entry(&order.from)
                .or_default()
                .provides
                .insert(order.needs.clone());
            ties.entry(&order.to)
                .or_default()
                .needs
                .insert(order.needs.clone());
        }
    }
    for kept in graph.kept.iter().filter(|kept| taken.contains(&kept.to)) {
        ties.entry(&kept.to)
            .or_default()
            .needs
            .insert(kept.name.clone());
        outside.insert(kept.name.clone(), kept_source(&kept.on));
    }
    let nodes = graph
        .nodes
        .iter()
        .filter(|node| fixing(node) != Fixing::Out)
        .map(|node| {
            let tied = ties.remove(&node.id).unwrap_or_default();
            let count = behaviour_count(node);
            let fixed = fixing(node) == Fixing::Fixed;
            let parts = u32::try_from(count).unwrap_or(u32::MAX);
            // A fixed node is never merged, so its size is of no use to f; a new one shares its
            // weight, at least 1 a part, the first parts taking what does not divide.
            let total = node.weight.0.max(parts);
            let behaviours = (0..parts)
                .map(|part| Behaviour {
                    text: format!("{} as the graph holds it", node.id),
                    size: Size::Weight(Weight(if fixed {
                        1
                    } else {
                        total / parts + u32::from(part < total % parts)
                    })),
                    writes: node.writes.clone(),
                    provides: if part == 0 {
                        tied.provides.clone()
                    } else {
                        BTreeSet::new()
                    },
                    needs: if part == 0 {
                        tied.needs.clone()
                    } else {
                        BTreeSet::new()
                    },
                })
                .collect();
            Node {
                id: node.id.clone(),
                kind: node.kind,
                filed: fixed.then_some(Filed::Open),
                turns: tied.turns,
                blocked_by: Vec::new(),
                epic: None,
                behaviours,
            }
        })
        .collect();
    let mut input = PlanInput::of(nodes);
    input.limit = Weight(BODY_WEIGHT_LIMIT);
    input.no_signature = Some(names.unsigned.clone());
    input.outside = outside;
    input
}

/// Whether two ids stand for the same part of a plan: they share a part of a merged id (`A+B`),
/// or one is a split part of the other (`A.a` of `A`).
fn related(a: &NodeId, b: &NodeId) -> bool {
    let part_of = |part: &str, whole: &str| {
        part == whole
            || part
                .strip_prefix(whole)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    a.as_str().split('+').any(|x| {
        b.as_str()
            .split('+')
            .any(|y| part_of(x, y) || part_of(y, x))
    })
}

/// The nodes the rerun changes, grouped: the ones that stand for the same part of the plan are one
/// failure.
fn node_failures(dropped: &BTreeSet<NodeId>, added: &BTreeSet<NodeId>) -> Vec<ShapeFailure> {
    let items: Vec<(&NodeId, bool)> = dropped
        .iter()
        .map(|id| (id, true))
        .chain(added.iter().map(|id| (id, false)))
        .collect();
    let mut groups: Vec<Vec<(&NodeId, bool)>> = Vec::new();
    for item in items {
        let (linked, rest): (Vec<_>, Vec<_>) = groups
            .into_iter()
            .partition(|group| group.iter().any(|(id, _)| related(id, item.0)));
        groups = rest;
        let mut merged: Vec<(&NodeId, bool)> = linked.into_iter().flatten().collect();
        merged.push(item);
        groups.push(merged);
    }
    let mut failures: Vec<ShapeFailure> = groups
        .into_iter()
        .map(|group| {
            let side = |gone: bool| {
                let mut ids: Vec<NodeId> = group
                    .iter()
                    .filter(|(_, was_dropped)| *was_dropped == gone)
                    .map(|(id, _)| (*id).clone())
                    .collect();
                ids.sort();
                ids
            };
            ShapeFailure::RerunNodes {
                dropped: side(true),
                added: side(false),
            }
        })
        .collect();
    failures.sort();
    failures
}

/// The rerun: f on the graph with production options changes nothing (D24, D29, I13). Each node,
/// wait or kept wait it would add, drop or rename is a failure, and a graph it refuses gives each
/// refusal.
fn check_rerun(graph: &PlanGraph, names: &Names, out: &mut Vec<ShapeFailure>) {
    let input = rerun_input(graph, names);
    let optimised = match optimise(&input, PRODUCTION) {
        Ok(optimised) => optimised,
        Err(refusals) => {
            let cycle_named = out.iter().any(|failure| failure.rule() == ShapeRule::Cycle);
            out.extend(
                refusals
                    .into_iter()
                    .filter(|refusal| !(cycle_named && refusal.code == RefusalCode::Cycle))
                    .map(|refusal| ShapeFailure::RerunRefused {
                        code: refusal.code,
                        detail: refusal.detail,
                    }),
            );
            return;
        }
    };
    let given: BTreeSet<NodeId> = input.nodes.iter().map(|node| node.id.clone()).collect();
    let made: BTreeSet<NodeId> = optimised.nodes.iter().map(|node| node.id.clone()).collect();
    let dropped: BTreeSet<NodeId> = given.difference(&made).cloned().collect();
    let added: BTreeSet<NodeId> = made.difference(&given).cloned().collect();
    let changed: BTreeSet<&NodeId> = dropped.iter().chain(&added).collect();
    out.extend(node_failures(&dropped, &added));

    let held: BTreeSet<&Order> = graph
        .orders
        .iter()
        .filter(|order| given.contains(&order.from) && given.contains(&order.to))
        .collect();
    let rerun: BTreeSet<&Order> = optimised.orders.iter().collect();
    let moved = |order: &Order| changed.contains(&order.from) || changed.contains(&order.to);
    for (change, orders) in [
        (Change::Add, rerun.difference(&held)),
        (Change::Drop, held.difference(&rerun)),
    ] {
        out.extend(
            orders
                .filter(|order| !moved(order))
                .map(|order| ShapeFailure::RerunWait {
                    change,
                    wait: (*order).clone(),
                }),
        );
    }

    // A kept wait is told by the node that waits and the name: where it comes from is whatever
    // `outside` kept for the name.
    let held: BTreeMap<(&NodeId, &Name), KeptWait> = graph
        .kept
        .iter()
        .filter(|kept| given.contains(&kept.to))
        .map(|kept| {
            (
                (&kept.to, &kept.name),
                KeptWait {
                    from: kept_source(&kept.on),
                    to: kept.to.clone(),
                    needs: kept.name.clone(),
                },
            )
        })
        .collect();
    let rerun: BTreeMap<(&NodeId, &Name), &KeptWait> = optimised
        .kept
        .iter()
        .map(|kept| ((&kept.to, &kept.needs), kept))
        .collect();
    let on_changed = |wait: &KeptWait| changed.contains(&wait.to);
    out.extend(
        rerun
            .iter()
            .filter(|(key, wait)| !held.contains_key(key) && !on_changed(wait))
            .map(|(_, wait)| ShapeFailure::RerunKept {
                change: Change::Add,
                wait: (*wait).clone(),
            }),
    );
    out.extend(
        held.iter()
            .filter(|(key, wait)| !rerun.contains_key(key) && !on_changed(wait))
            .map(|(_, wait)| ShapeFailure::RerunKept {
                change: Change::Drop,
                wait: wait.clone(),
            }),
    );
}

/// The sixth rule: the graph is as zaf filed it. The last record's filing mark holds the hash the
/// graph must have; a last record with no mark is half filed. With no record there is nothing to
/// hold the graph to.
fn check_hash(graph: &PlanGraph, records: &[EpicRecord], out: &mut Vec<ShapeFailure>) {
    let Some(last) = records.last() else {
        return;
    };
    match &last.mark {
        None => out.push(ShapeFailure::HalfFiled {
            record: last.record.k,
        }),
        Some(mark) => {
            let graph = graph.hash();
            if graph != mark.graph {
                out.push(ShapeFailure::GraphHash {
                    record: mark.k,
                    marked: mark.graph.clone(),
                    graph,
                });
            }
        }
    }
}

/// Every way `graph` breaks the rules of O3 (D24), one failure for each wait, node, cycle or pair
/// of writers, one for the hash, and one for each change the optimiser would make on a rerun;
/// empty when it breaks none (I13).
///
/// `vocabulary` says which names have a signature, and `records`, the epic's plan records with
/// their filing marks in posting order, add the names the plans declared and give the hash the
/// graph must have. The failures come in the order of the rules, and within one rule in natural
/// id order, so the same graph gives the same list.
pub fn check_shape(
    graph: &PlanGraph,
    vocabulary: &Vocabulary,
    records: &[EpicRecord],
) -> Vec<ShapeFailure> {
    let names = Names::of(vocabulary, records);
    let mut failures = Vec::new();
    check_waits(graph, &names, &mut failures);
    check_limit(graph, &mut failures);
    check_order(graph, &mut failures);
    check_hash(graph, records, &mut failures);
    check_rerun(graph, &names, &mut failures);
    failures
}
