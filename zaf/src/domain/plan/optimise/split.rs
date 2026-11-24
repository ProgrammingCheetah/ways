//! The `split` pass: one behaviour per node, with the ids D9 gives. A merged node `A+B` splits
//! back into `A` and `B`, so a rerun keeps its ids; any other node of several behaviours gives
//! `<id>.a`, `<id>.b`, and so on. An interface node and an open filed ticket (D29) stay whole.

use super::Working;
use crate::domain::plan::{Filed, Node, NodeId, NodeKind};

/// Splits the body nodes that hold several behaviours, one node per behaviour. Each part keeps
/// the node's other fields. The nodes come out in id order.
pub fn split(working: &mut Working) {
    let nodes = std::mem::take(&mut working.plan.nodes);
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        if is_whole(&node) {
            out.push(node);
            continue;
        }
        let ids = part_ids(&node);
        for (id, behaviour) in ids.into_iter().zip(node.behaviours.iter()) {
            let mut part = node.clone();
            part.id = id;
            part.behaviours = vec![behaviour.clone()];
            out.push(part);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    working.plan.nodes = out;
}

/// Whether the node stays as it is: an interface node, an open filed ticket, or a node of one
/// behaviour (or none).
fn is_whole(node: &Node) -> bool {
    node.kind != NodeKind::Body || node.behaviours.len() <= 1 || node.filed == Some(Filed::Open)
}

/// The id of the `index`-th part of `base`: `a` to `z`, then the number.
fn part_id(base: &NodeId, index: usize) -> NodeId {
    match u8::try_from(index).ok().filter(|i| *i < 26) {
        Some(i) => NodeId::from(format!("{base}.{}", char::from(b'a' + i))),
        None => NodeId::from(format!("{base}.{index}")),
    }
}

/// The id of each behaviour once split (D9). `A+B` of two behaviours gives `A` and `B`; any
/// other node gives `<id>.a`, `<id>.b`, ...
fn part_ids(node: &Node) -> Vec<NodeId> {
    let count = node.behaviours.len();
    let parts: Vec<&str> = node.id.as_str().split('+').collect();
    if count > 1 && parts.len() == count {
        return parts.into_iter().map(NodeId::from).collect();
    }
    (0..count).map(|index| part_id(&node.id, index)).collect()
}

#[cfg(test)]
mod tests;
