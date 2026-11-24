//! The `thin` pass: every wait that another path already implies is dropped (D8, D16, M3).
//!
//! The waits that stay are the reduction of the graph (`Dag::reduction`). Between two nodes one
//! wait stays, labelled with the greatest of its names. A node keeps the name in its needs when its
//! wait is dropped: only `orders` change.

use std::collections::{BTreeMap, BTreeSet};

use super::Working;
use crate::domain::plan::graph::Dag;
use crate::domain::plan::{NodeId, Order};

/// Thins `working.orders` to the waits of the reduction, one per pair of nodes.
///
/// A pair the reduction keeps stays with the greatest name of the waits it had. Waits that go
/// round have no reduction: the refusal pass refuses such a plan, and until it does thin leaves
/// them as they are.
pub fn thin(working: &mut Working) {
    let ids: Vec<NodeId> = working.plan.nodes.iter().map(|n| n.id.clone()).collect();
    working.orders = thinned(&ids, &working.orders);
}

/// The waits of `orders` that stay between the nodes `ids`: the reduction's pairs, each with the
/// greatest name of the waits it had. Waits that go round are returned as they are.
///
/// Interface: `thinned(ids, orders)`, which `PlanGraph::read` thins the orders it reads through.
pub fn thinned(ids: &[NodeId], orders: &BTreeSet<Order>) -> BTreeSet<Order> {
    let pairs: Vec<(NodeId, NodeId)> = orders
        .iter()
        .map(|o| (o.from.clone(), o.to.clone()))
        .collect();
    let dag = Dag::new(ids, &pairs);
    if dag.cycle().is_some() {
        return orders.clone();
    }
    let keep = dag.reduction();
    // `orders` sorts by from, to, then name, so the last wait of a pair has the greatest name.
    let mut greatest: BTreeMap<(&NodeId, &NodeId), &Order> = BTreeMap::new();
    for order in orders {
        greatest.insert((&order.from, &order.to), order);
    }
    keep.iter()
        .filter_map(|(from, to)| greatest.get(&(from, to)).map(|&o| o.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
