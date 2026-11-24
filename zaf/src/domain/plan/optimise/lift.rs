//! The `lift` pass: a wait on a name with a signature becomes an interface (D11, D13, D29, Q6).
//!
//! The users wait on the signature and the provider implements it. A name other epics need
//! (`shared`) is lifted too, though nothing in the plan waits on it, so their waits go through
//! this epic's interface (D13).

use std::collections::{BTreeMap, BTreeSet};

use super::Working;
use crate::domain::plan::{
    Area, Behaviour, Filed, Interfaces, Lift, Name, Node, NodeId, NodeKind, Order, PlanInput,
};

/// Lifts every name that has a signature into an interface node, unless the options lift none.
///
/// A name is liftable when it has a signature (it is not in `no_signature`) and its provider is a
/// body node that is not an open filed ticket (D29). The lifted names are those on a wait and
/// the plan's `shared` names, in sorted order. Each goes into the interface node of the run
/// ([`interface_id`]) as a behaviour `the signature of <name>` of size 1 that writes `sig:<name>`
/// and provides the name. The provider stops providing the name and needs it instead; every wait
/// on it now comes from the interface node, and the provider waits on the interface too.
pub fn lift(working: &mut Working) {
    if working.options.lift == Lift::None {
        return;
    }
    let provider = providers(&working.plan.nodes);
    let skip = working.plan.no_signature.clone().unwrap_or_default();
    let liftable = |name: &Name| {
        let owner = provider
            .get(name)
            .and_then(|id| working.plan.nodes.iter().find(|node| node.id == *id));
        !skip.contains(name)
            && owner
                .is_some_and(|node| node.kind == NodeKind::Body && node.filed != Some(Filed::Open))
    };
    let lifted: BTreeSet<Name> = working
        .orders
        .iter()
        .map(|order| &order.needs)
        .chain(&working.plan.shared)
        .filter(|name| liftable(name))
        .cloned()
        .collect();
    for name in lifted {
        let owner = provider[&name].clone();
        let iface = interface_id_of(&working.plan, working.options.interfaces, &owner, &name);
        lift_name(working, &name, &owner, &iface);
    }
    working.plan.nodes.sort_by(|a, b| a.id.cmp(&b.id));
}

/// The node that provides each name. A plan that gives one name two providers is refused before
/// this runs.
fn providers(nodes: &[Node]) -> BTreeMap<Name, NodeId> {
    let mut out = BTreeMap::new();
    for node in nodes {
        for name in node.behaviours.iter().flat_map(|b| &b.provides) {
            out.insert(name.clone(), node.id.clone());
        }
    }
    out
}

/// The id of the interface node a name lands in: `I:<name>` with `Each`; with `One`, the node
/// of the owner's epic and the plan's record.
fn interface_id_of(
    plan: &PlanInput,
    interfaces: Interfaces,
    owner: &NodeId,
    name: &Name,
) -> NodeId {
    match interfaces {
        Interfaces::Each => NodeId::from(format!("I:{name}")),
        Interfaces::One => {
            let epic = plan
                .nodes
                .iter()
                .find(|node| node.id == *owner)
                .and_then(|node| node.epic.as_ref());
            interface_id(epic.map(|epic| epic.as_str()), plan.record)
        }
    }
}

/// The interface node of an epic's `record`-th plan, `I@<epic>.<record>`, so a re-plan's
/// signatures never land in a ticket already filed (D11, D29). A plan with no epic has `I`, and
/// `I.<record>` after its first.
pub fn interface_id(epic: Option<&str>, record: u32) -> NodeId {
    match epic {
        Some(epic) => NodeId::from(format!("I@{epic}.{record}")),
        None if record == 1 => NodeId::from("I"),
        None => NodeId::from(format!("I.{record}")),
    }
}

/// Puts one name into the interface node `iface` (made when the plan has none) and moves the
/// waits on it.
fn lift_name(working: &mut Working, name: &Name, owner: &NodeId, iface: &NodeId) {
    let epic = working
        .plan
        .nodes
        .iter()
        .find(|node| node.id == *owner)
        .and_then(|node| node.epic.clone())
        .filter(|_| working.options.interfaces == Interfaces::One);
    let signature = Behaviour {
        text: format!("the signature of {name}"),
        size: Default::default(),
        writes: BTreeSet::from([Area::from(format!("sig:{name}"))]),
        provides: BTreeSet::from([name.clone()]),
        needs: BTreeSet::new(),
    };
    match working.plan.nodes.iter_mut().find(|node| node.id == *iface) {
        Some(node) => node.behaviours.push(signature),
        None => working.plan.nodes.push(Node {
            id: iface.clone(),
            kind: NodeKind::Interface,
            filed: None,
            turns: Vec::new(),
            blocked_by: Vec::new(),
            epic,
            behaviours: vec![signature],
        }),
    }
    if let Some(node) = working.plan.nodes.iter_mut().find(|node| node.id == *owner) {
        for behaviour in &mut node.behaviours {
            if behaviour.provides.remove(name) {
                behaviour.needs.insert(name.clone());
            }
        }
    }
    let moved: Vec<Order> = working
        .orders
        .iter()
        .filter(|order| order.needs == *name)
        .cloned()
        .collect();
    for order in moved {
        working.orders.remove(&order);
        working.orders.insert(Order {
            from: iface.clone(),
            ..order
        });
    }
    working.orders.insert(Order {
        from: iface.clone(),
        to: owner.clone(),
        needs: name.clone(),
    });
}

#[cfg(test)]
mod tests;
