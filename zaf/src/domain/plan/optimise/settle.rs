//! The `settle` pass: two unordered nodes that write the same area are merged if they fit the
//! limit, else put in order (D12, D29, Q8).
//!
//! The reference's loop: walk the pairs in natural id order, take the first that shares a written
//! area with no wait either way, settle it, and walk again from the start until no pair is left.
//! The graph is never edited in place: [`Dag`] is built afresh from the nodes and waits at each
//! step, so the reach of every node is the closure of what stands then.

use std::collections::{BTreeMap, BTreeSet};

use super::Working;
use crate::domain::plan::graph::Dag;
use crate::domain::plan::{
    Area, Filed, Name, Node, NodeId, NodeKind, Order, Refusal, RefusalCode, Weight,
};

/// Every node with the nodes that come after it, through any chain of waits.
type Reach = BTreeMap<NodeId, BTreeSet<NodeId>>;

/// A pair of nodes (by index in the plan's nodes) that write one area and are in no order.
struct Conflict {
    first: usize,
    second: usize,
    /// The lowest area they both write.
    area: Area,
}

/// Settles the conflicts: each pair of unordered writers of one area is merged into `A+B` when
/// both are bodies, neither is a filed ticket, and their weights together fit the limit. Any
/// other pair takes turns on its lowest shared area, with a wait `turn:<area>`: the filed ticket
/// first when there is one, else the node more nodes wait on, the lower id on a tie. Two open
/// filed tickets were settled when they were filed and are skipped.
///
/// # Errors
///
/// `duplicate-id` when a merge would be `A+B` and a node of the plan holds that id already: a
/// filed ticket that was merged from the same keys (D29). Nothing is repaired; the planner gives
/// the new tasks other keys.
pub fn settle(working: &mut Working) -> Result<(), Refusal> {
    working.plan.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    loop {
        let Some(closure) = closure(working) else {
            return Ok(());
        };
        let Some(conflict) = first_conflict(&working.plan.nodes, &closure) else {
            return Ok(());
        };
        let a = &working.plan.nodes[conflict.first];
        let b = &working.plan.nodes[conflict.second];
        if is_filed(a) || is_filed(b) {
            let (first, second) = if is_filed(a) { (a, b) } else { (b, a) };
            let order = turn(first, second, &conflict.area);
            working.orders.insert(order);
        } else if fits(a, b, working.plan.limit) {
            let id = merged_id(a, b);
            if working.plan.nodes.iter().any(|node| node.id == id) {
                return Err(Refusal::new(
                    RefusalCode::DuplicateId,
                    format!(
                        "{id}: {} and {} would merge into the id a filed ticket holds: give the \
                         new tasks other keys",
                        a.id, b.id
                    ),
                ));
            }
            merge(working, conflict.first, conflict.second);
        } else {
            let waiting = |node: &Node| closure.get(&node.id).map_or(0, BTreeSet::len);
            // The node more nodes wait on goes first; `a` has the lower id.
            let (first, second) = if waiting(b) > waiting(a) {
                (b, a)
            } else {
                (a, b)
            };
            let order = turn(first, second, &conflict.area);
            working.orders.insert(order);
        }
    }
}

/// The first pair, in natural id order, that shares a written area with no wait either way, and
/// is not two open filed tickets.
fn first_conflict(nodes: &[Node], closure: &Reach) -> Option<Conflict> {
    let reaches = |from: &Node, to: &Node| {
        closure
            .get(&from.id)
            .is_some_and(|reached| reached.contains(&to.id))
    };
    for (i, a) in nodes.iter().enumerate() {
        for (j, b) in nodes.iter().enumerate().skip(i + 1) {
            if is_filed(a) && is_filed(b) {
                continue;
            }
            let Some(area) = shared_area(a, b) else {
                continue;
            };
            if !reaches(a, b) && !reaches(b, a) {
                return Some(Conflict {
                    first: i,
                    second: j,
                    area,
                });
            }
        }
    }
    None
}

/// The reach of every node along the waits as they stand, or `None` when the waits go round: the
/// refusal pass keeps such a plan out, and a closure of one is not defined.
fn closure(working: &Working) -> Option<Reach> {
    let ids: Vec<NodeId> = working
        .plan
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect();
    let edges: Vec<(NodeId, NodeId)> = working
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    let dag = Dag::new(&ids, &edges);
    if dag.cycle().is_some() {
        return None;
    }
    Some(dag.closure())
}

/// An open filed ticket: fixed, never merged (D29).
fn is_filed(node: &Node) -> bool {
    node.filed == Some(Filed::Open)
}

/// The lowest area both nodes write, if they write one.
fn shared_area(a: &Node, b: &Node) -> Option<Area> {
    let writes = |node: &Node| -> BTreeSet<Area> {
        node.behaviours
            .iter()
            .flat_map(|behaviour| behaviour.writes.iter().cloned())
            .collect()
    };
    writes(a).intersection(&writes(b)).next().cloned()
}

/// The weight of a node: its behaviours' weights together. `None` when a size is not a weight,
/// which the refusal pass has caught before a pass runs.
fn weight(node: &Node) -> Option<Weight> {
    node.behaviours
        .iter()
        .try_fold(0u32, |sum, behaviour| {
            behaviour
                .size
                .weight()
                .and_then(|Weight(w)| sum.checked_add(w))
        })
        .map(Weight)
}

/// Whether the two nodes are bodies whose weights together are within the limit.
fn fits(a: &Node, b: &Node, limit: Weight) -> bool {
    if a.kind != NodeKind::Body || b.kind != NodeKind::Body {
        return false;
    }
    match (weight(a), weight(b)) {
        (Some(Weight(a)), Some(Weight(b))) => a.checked_add(b).is_some_and(|sum| sum <= limit.0),
        _ => false,
    }
}

/// The wait that puts `second` after `first` on `area`.
fn turn(first: &Node, second: &Node, area: &Area) -> Order {
    Order {
        from: first.id.clone(),
        to: second.id.clone(),
        needs: Name::from(format!("turn:{area}")),
    }
}

/// The id of two nodes merged: `A+B`.
fn merged_id(a: &Node, b: &Node) -> NodeId {
    NodeId::from(format!("{}+{}", a.id, b.id))
}

/// Replaces the nodes at `first` and `second` with `A+B`: the behaviours in order, the other
/// fields the first node's, every wait of either moved to it, and no wait on itself.
fn merge(working: &mut Working, first: usize, second: usize) {
    let a = working.plan.nodes[first].clone();
    let b = working.plan.nodes[second].clone();
    let id = merged_id(&a, &b);
    let mut behaviours = a.behaviours.clone();
    behaviours.extend(b.behaviours.iter().cloned());
    let merged = Node {
        id: id.clone(),
        behaviours,
        ..a.clone()
    };
    let nodes = &mut working.plan.nodes;
    nodes.retain(|node| node.id != a.id && node.id != b.id);
    nodes.push(merged);
    nodes.sort_by(|x, y| x.id.cmp(&y.id));
    let moved = |end: NodeId| {
        if end == a.id || end == b.id {
            id.clone()
        } else {
            end
        }
    };
    working.orders = std::mem::take(&mut working.orders)
        .into_iter()
        .map(|order| Order {
            from: moved(order.from),
            to: moved(order.to),
            needs: order.needs,
        })
        .filter(|order| order.from != order.to)
        .collect();
}

#[cfg(test)]
mod tests;
