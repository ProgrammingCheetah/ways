//! What a plan measures: its layers, its chain, the width of each layer, its work and its
//! critical path (I5, D15).
//!
//! The layers come by longest path ([`Dag::layers`]): the chain is their number and the width the
//! size of each. The critical length is the heaviest chain weighed by size (an interface node
//! weighs 1 a signature), and [`critical_path`] names it node by node as the reference's
//! `critical_path()` does, with every tie to the lower id. rustworkx-core's `longest_path` is
//! never used: its ties change between runs.

use std::collections::{BTreeMap, BTreeSet};

use super::graph::Dag;
use super::names::normalise;
use super::optimise::Working;
use super::optimise::derive::derive;
use super::refuse::refuse;
use super::{
    Measures, Node, NodeId, NodeKind, Optimised, Options, PlanInput, Refusal, RefusalCode, Weight,
};

/// What a node weighs: the sum of its behaviours' sizes. A behaviour whose size is no integer
/// weighs the default 1; the refusal pass has turned such a plan away before it is measured.
fn weight(node: &Node) -> u32 {
    node.behaviours
        .iter()
        .map(|behaviour| behaviour.size.weight().unwrap_or(Weight(1)).0)
        .sum()
}

/// Measures a plan for this many lanes, over the waits inside it (a wait on another epic's task
/// is kept and weighs nothing here).
///
/// # Panics
/// When the plan's waits go round: the refusal pass has turned such a plan away.
pub fn measure(plan: &Optimised, lanes: u32) -> Measures {
    let ids: Vec<NodeId> = plan.nodes.iter().map(|node| node.id.clone()).collect();
    let waits: Vec<(NodeId, NodeId)> = plan
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    let layers = Dag::new(&ids, &waits).layers();
    let by_id: BTreeMap<&NodeId, &Node> = plan.nodes.iter().map(|node| (&node.id, node)).collect();
    let mut preds: BTreeMap<&NodeId, BTreeSet<&NodeId>> =
        by_id.keys().map(|&id| (id, BTreeSet::new())).collect();
    for (from, to) in &waits {
        if let (Some((&from, _)), Some(to)) = (by_id.get_key_value(from), preds.get_mut(to)) {
            to.insert(from);
        }
    }

    // A node's heaviest chain, built layer by layer: every wait of a node is on an earlier layer.
    let mut longest: BTreeMap<&NodeId, u32> = BTreeMap::new();
    for id in layers.iter().flatten() {
        let (&id, node) = by_id.get_key_value(id).expect("a layer holds only nodes");
        let before = preds[id]
            .iter()
            .map(|pred| longest[pred])
            .max()
            .unwrap_or(0);
        longest.insert(id, weight(node) + before);
    }

    let width: Vec<usize> = layers.iter().map(Vec::len).collect();
    let lanes = usize::try_from(lanes).unwrap_or(usize::MAX);
    Measures {
        nodes: plan.nodes.len(),
        interfaces: plan
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Interface)
            .count(),
        chain: layers.len(),
        narrow_layers: width
            .iter()
            .enumerate()
            .filter(|&(_, &size)| size < lanes)
            .map(|(at, _)| at)
            .collect(),
        width,
        work: plan.nodes.iter().map(weight).sum(),
        critical: longest.values().copied().max().unwrap_or(0),
        critical_path: critical_path(&by_id, &preds, &longest),
    }
}

/// The heaviest chain's nodes, first to last (D15). It ends at the node where the heaviest chain
/// ends, the lowest id on a tie; each step back goes to a node waited on whose heaviest chain is
/// shorter by exactly this node's weight, the lowest id on a tie.
fn critical_path(
    by_id: &BTreeMap<&NodeId, &Node>,
    preds: &BTreeMap<&NodeId, BTreeSet<&NodeId>>,
    longest: &BTreeMap<&NodeId, u32>,
) -> Vec<NodeId> {
    let Some(top) = longest.values().copied().max() else {
        return Vec::new();
    };
    let mut current = longest
        .iter()
        .filter(|&(_, &length)| length == top)
        .map(|(&id, _)| id)
        .min();
    let mut path = Vec::new();
    while let Some(id) = current {
        path.push(id.clone());
        let want = longest[id] - weight(by_id[id]);
        current = preds[id]
            .iter()
            .copied()
            .filter(|pred| longest[pred] == want)
            .min();
    }
    path.reverse();
    path
}

/// Measures the plan before any pass (the reference's `baseline`): the plan read as the
/// optimiser reads it, with the waits its names derive and nothing split, lifted, settled or
/// thinned. A plan the optimiser would refuse is refused here, with the first thing wrong.
pub fn baseline(plan: &PlanInput) -> Result<Measures, Refusal> {
    // The reference's order: the plan is normalised first, and what is refused is that plan
    // (`refuse` reads a normalised plan). Each names everything wrong; the first is the one given.
    let first = |refusals: Vec<Refusal>| {
        refusals
            .into_iter()
            .next()
            .expect("a refused plan names a refusal")
    };
    let plan = normalise(plan).map_err(first)?;
    refuse(&plan).map_err(first)?;
    let lanes = plan.lanes;
    let mut working = Working {
        plan,
        options: Options::default(),
        orders: BTreeSet::new(),
    };
    derive(&mut working);
    let ids: Vec<NodeId> = working.plan.nodes.iter().map(|n| n.id.clone()).collect();
    let waits: Vec<(NodeId, NodeId)> = working
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    if let Some(cycle) = Dag::new(&ids, &waits).cycle() {
        let names: Vec<&str> = cycle.iter().map(NodeId::as_str).collect();
        return Err(Refusal::new(RefusalCode::Cycle, names.join(" ")));
    }
    let optimised = Optimised {
        nodes: working.plan.nodes,
        orders: working.orders,
        kept: BTreeSet::new(),
    };
    Ok(measure(&optimised, lanes))
}

#[cfg(test)]
mod tests;
