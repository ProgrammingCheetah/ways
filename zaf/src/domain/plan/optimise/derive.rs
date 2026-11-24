//! The `derive` pass: the order edges, the provider of a name before each node that needs it
//! (D2, D13, D29, M3).
//!
//! It reads a normalised plan (`names::normalise`): a done filed ticket is already out of the
//! nodes and what it provided is `existing`, so a need on it finds no provider and makes no wait.

use std::collections::{BTreeMap, BTreeSet};

use super::Working;
use crate::domain::plan::{Name, Node, NodeId, Order};

/// Adds the order edges to `working.orders`, which sort by the reference's edge key (natural
/// from, natural to, name).
///
/// - For each name a node needs, an [`Order`] from the node that provides it to the node, on the
///   name. A name no node provides (existing code, or another epic's task) makes no wait, and a
///   node never waits on itself.
/// - Each of a filed node's `turns` is a wait on `turn:<area>` when the node it follows is still
///   in the plan; one after a node that is gone (a done ticket) is over and drops.
///
/// Nothing is removed: the waits already in `working.orders` stay.
pub fn derive(working: &mut Working) {
    let provider = providers(&working.plan.nodes);
    let ids: BTreeSet<&NodeId> = working.plan.nodes.iter().map(|node| &node.id).collect();
    let mut orders = Vec::new();
    for node in &working.plan.nodes {
        for turn in &node.turns {
            if ids.contains(&turn.0) {
                orders.push(Order {
                    from: turn.0.clone(),
                    to: node.id.clone(),
                    needs: Name::from(format!("turn:{}", turn.1)),
                });
            }
        }
        let needs: BTreeSet<&Name> = node
            .behaviours
            .iter()
            .flat_map(|behaviour| &behaviour.needs)
            .collect();
        for name in needs {
            if let Some(from) = provider.get(name).filter(|from| **from != &node.id) {
                orders.push(Order {
                    from: (*from).clone(),
                    to: node.id.clone(),
                    needs: name.clone(),
                });
            }
        }
    }
    working.orders.extend(orders);
}

/// The node that provides each name. A plan that gives one name two providers is refused before
/// this runs; were it not, the node listed last would win, as in the reference.
fn providers(nodes: &[Node]) -> BTreeMap<&Name, &NodeId> {
    let mut out = BTreeMap::new();
    for node in nodes {
        for name in node.behaviours.iter().flat_map(|b| &b.provides) {
            out.insert(name, &node.id);
        }
    }
    out
}

#[cfg(test)]
mod tests;
