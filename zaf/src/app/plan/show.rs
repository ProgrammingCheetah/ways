//! `plan show` (D24, I9): an epic's graph as its tickets say it now, or as its plan records say it
//! was planned, with its layers and measures.
//!
//! The graph is read with [`PlanGraph::read`] (the tickets) or [`PlanGraph::before`] (the plan
//! records) and measured with [`measure`]; nothing is written. An epic filed before 0.6.0 has no
//! plan record and no planned ticket: it has one view, of its tickets, and [`ViewKind::Older`]
//! says so (D4). The result is data only: each reader of the graph (the text of `plan show`, its
//! JSON, the chair) draws it itself and depends on [`PlanView`], never on the domain's graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::plan::graph::Dag;
use crate::domain::plan::read::{BadRecord, KeptRead, PlanGraph};
use crate::domain::plan::text::parse_plan;
use crate::domain::plan::{
    Behaviour, Measures, Node, NodeId, NodeKind, Optimised, Order, PlanInput, Size, Weight,
};
use crate::domain::{Epic, TaskBody, TicketId};
use crate::ports::{Board, PortError};

/// A node of the view: one planned ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewNode {
    /// Its id, as its `Node:` line says.
    pub id: NodeId,
    /// The ticket that holds it; `None` in the graph as planned, which no ticket holds yet.
    pub ticket: Option<TicketId>,
    /// The ticket's title, or the title its plan record gave.
    pub title: String,
    /// An interface ticket or a body.
    pub kind: NodeKind,
}

/// Which graph a view is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// The graph as the tickets read now.
    After,
    /// The graph as the plan records wrote it.
    Before,
    /// The graph of tickets filed before 0.6.0, the only one such an epic has.
    Older,
}

/// An epic's graph as it reads now: its nodes, its waits, its layers and what they measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanView {
    /// The epic.
    pub epic: TicketId,
    /// Which graph this is.
    pub kind: ViewKind,
    /// The nodes, by natural id.
    pub nodes: Vec<ViewNode>,
    /// The waits between them, thinned, sorted.
    pub orders: Vec<Order>,
    /// The waits on other epics' tasks, as written, sorted.
    pub kept: Vec<KeptRead>,
    /// The nodes in layers, each sorted: the first waits for none, each next one on the one
    /// before.
    pub layers: Vec<Vec<NodeId>>,
    /// The chain, the width of each layer, the work and the critical path.
    pub measures: Measures,
}

/// Why an epic's graph is not shown.
#[derive(Debug, thiserror::Error)]
pub enum ShowError {
    /// The ticket is no open epic.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// The epic has no planned ticket.
    #[error(
        "epic {0} has no planned tickets: file its plan with `zaf plan file {0}`, or see `zaf epic show {0}`"
    )]
    NoTickets(TicketId),
    /// The waits between the epic's tickets go round, so it has no layers.
    #[error("epic {epic}'s waits go round: {}; edit the tickets' Needs and Provides lines", nodes.join(" "))]
    Cycle {
        /// The epic.
        epic: TicketId,
        /// The nodes that go round.
        nodes: Vec<String>,
    },
    /// A plan record of the epic does not read.
    #[error("epic {epic}: {error}")]
    BadRecord {
        /// The epic.
        epic: TicketId,
        /// The record and what is wrong with it.
        error: BadRecord,
    },
    /// The board could not answer.
    #[error(transparent)]
    Board(PortError),
}

/// The graph of `epic` as its tickets say it now; an epic filed before 0.6.0 is
/// [`ViewKind::Older`].
///
/// # Errors
/// [`ShowError::NotAnEpic`] when `epic` is no open epic, [`ShowError::NoTickets`] when none of
/// its tickets is planned, [`ShowError::Cycle`] when its waits go round, and
/// [`ShowError::Board`] when the board could not answer.
pub fn show(board: &dyn Board, epic: TicketId) -> Result<PlanView, ShowError> {
    let (found, epics) = open_epic(board, epic)?;
    view(&found, &epics)
}

/// The graph of `epic` as its plan records say it was planned (I6). An epic with no plan record
/// has one view only, as [`show`] gives it.
///
/// # Errors
/// As [`show`], and [`ShowError::BadRecord`] when a plan record does not read.
pub fn show_before(board: &dyn Board, epic: TicketId) -> Result<PlanView, ShowError> {
    let (found, epics) = open_epic(board, epic)?;
    if found.records.is_empty() {
        return view(&found, &epics);
    }
    view_before(&found)
}

/// The open epic `epic`, with the release's open epics.
fn open_epic(board: &dyn Board, epic: TicketId) -> Result<(Epic, Vec<Epic>), ShowError> {
    let epics = board.epics().map_err(ShowError::Board)?;
    let found = epics
        .iter()
        .find(|candidate| candidate.id == epic)
        .cloned()
        .ok_or(ShowError::NotAnEpic(epic))?;
    Ok((found, epics))
}

/// One node of a view with what it weighs.
struct Weighed {
    node: ViewNode,
    weight: u32,
}

/// The view of `epic` as its plan records wrote it.
fn view_before(epic: &Epic) -> Result<PlanView, ShowError> {
    let records: Vec<_> = epic
        .records
        .iter()
        .map(|filed| filed.record.clone())
        .collect();
    let bad = |error| ShowError::BadRecord {
        epic: epic.id,
        error,
    };
    let graph = PlanGraph::before(&records).map_err(bad)?;
    // The graph keeps no size and no title: they are read from each record's plan again, which
    // `before` has just parsed.
    let mut by_id: BTreeMap<NodeId, (String, u32)> = BTreeMap::new();
    for record in &records {
        let parsed = parse_plan(&record.plan).map_err(|faults| {
            bad(BadRecord {
                record: record.k,
                faults,
            })
        })?;
        for (node, section) in parsed.input.nodes.iter().zip(&parsed.sections) {
            let weight = node
                .behaviours
                .iter()
                .map(|behaviour| behaviour.size.weight().unwrap_or(Weight(1)).0)
                .sum();
            by_id
                .entry(node.id.clone())
                .or_insert((section.title.clone(), weight));
        }
    }
    let mut seen = BTreeSet::new();
    let weighed: Vec<Weighed> = graph
        .nodes
        .iter()
        .filter(|node| seen.insert(node.id.clone()))
        .map(|node| {
            let (title, weight) = by_id.get(&node.id).cloned().unwrap_or_default();
            Weighed {
                node: ViewNode {
                    id: node.id.clone(),
                    ticket: None,
                    title,
                    kind: node.kind,
                },
                weight,
            }
        })
        .collect();
    assemble(epic.id, ViewKind::Before, weighed, graph.orders, graph.kept)
}

/// The view of `epic`, read with the release's open epics at hand.
fn view(epic: &Epic, release: &[Epic]) -> Result<PlanView, ShowError> {
    let graph = PlanGraph::read(epic, release);
    let weights = weights(epic);
    let titles: BTreeMap<TicketId, String> = epic
        .tasks
        .iter()
        .map(|task| (task.number, task.title.to_string()))
        .collect();
    // A graph read from tickets holds only filed nodes: each has its ticket.
    let weighed: Vec<Weighed> = graph
        .nodes
        .iter()
        .filter_map(|node| node.filed.as_ref().map(|filed| (node, filed.ticket)))
        .map(|(node, ticket)| Weighed {
            node: ViewNode {
                id: node.id.clone(),
                ticket: Some(ticket),
                title: titles.get(&ticket).cloned().unwrap_or_default(),
                kind: node.kind,
            },
            weight: weights.get(&ticket).copied().unwrap_or(1),
        })
        .collect();
    // No plan record and no planned ticket: the tickets are of the older template (D4).
    let kind = if epic.records.is_empty() && weights.is_empty() {
        ViewKind::Older
    } else {
        ViewKind::After
    };
    assemble(epic.id, kind, weighed, graph.orders, graph.kept)
}

/// Lays the nodes out in layers and measures them.
fn assemble(
    epic: TicketId,
    kind: ViewKind,
    weighed: Vec<Weighed>,
    orders: BTreeSet<Order>,
    kept: BTreeSet<KeptRead>,
) -> Result<PlanView, ShowError> {
    if weighed.is_empty() {
        return Err(ShowError::NoTickets(epic));
    }
    let ids: Vec<NodeId> = weighed.iter().map(|w| w.node.id.clone()).collect();
    let waits: Vec<(NodeId, NodeId)> = orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    let dag = Dag::new(&ids, &waits);
    if let Some(cycle) = dag.cycle() {
        return Err(ShowError::Cycle {
            epic,
            nodes: cycle.iter().map(|id| id.as_str().to_owned()).collect(),
        });
    }
    let measured = Optimised {
        nodes: weighed
            .iter()
            .map(|w| Node {
                id: w.node.id.clone(),
                kind: w.node.kind,
                filed: None,
                turns: Vec::new(),
                blocked_by: Vec::new(),
                epic: None,
                behaviours: vec![Behaviour {
                    text: w.node.id.to_string(),
                    size: Size::Weight(Weight(w.weight)),
                    writes: Default::default(),
                    provides: Default::default(),
                    needs: Default::default(),
                }],
            })
            .collect(),
        orders: orders.clone(),
        // `measure` weighs no kept wait.
        kept: Default::default(),
    };
    Ok(PlanView {
        epic,
        kind,
        nodes: weighed.into_iter().map(|w| w.node).collect(),
        orders: orders.into_iter().collect(),
        kept: kept.into_iter().collect(),
        layers: dag.layers(),
        measures: crate::domain::plan::measure::measure(&measured, PlanInput::of(Vec::new()).lanes),
    })
}

/// What each planned ticket of the epic weighs as a node.
fn weights(epic: &Epic) -> BTreeMap<TicketId, u32> {
    epic.tasks
        .iter()
        .filter_map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => Some((task.number, ticket.node_weight())),
            _ => None,
        })
        .collect()
}
