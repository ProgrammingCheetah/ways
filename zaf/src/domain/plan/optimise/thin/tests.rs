use std::collections::BTreeSet;

use super::thin;
use crate::domain::plan::optimise::Working;
use crate::domain::plan::{Behaviour, Name, Node, NodeId, NodeKind, Options, Order, PlanInput};

fn node(id: &str) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours: vec![Behaviour {
            text: "a behaviour".to_owned(),
            size: Default::default(),
            writes: BTreeSet::new(),
            provides: BTreeSet::new(),
            needs: BTreeSet::new(),
        }],
    }
}

fn order(from: &str, to: &str, needs: &str) -> Order {
    Order {
        from: NodeId::from(from),
        to: NodeId::from(to),
        needs: Name::from(needs),
    }
}

fn thinned(ids: &[&str], orders: Vec<Order>) -> Vec<Order> {
    let mut working = Working {
        plan: PlanInput::of(ids.iter().map(|id| node(id)).collect()),
        options: Options::default(),
        orders: orders.into_iter().collect(),
    };
    thin(&mut working);
    working.orders.into_iter().collect()
}

#[test]
fn fixture_11_drops_the_wait_another_path_implies() {
    let kept = thinned(
        &["A", "B", "C"],
        vec![
            order("A", "B", "a"),
            order("A", "C", "a"),
            order("B", "C", "b"),
        ],
    );
    assert_eq!(kept, [order("A", "B", "a"), order("B", "C", "b")]);
}

#[test]
fn fixture_13_two_waits_between_a_pair_become_one_with_the_greater_name() {
    let kept = thinned(
        &["A", "B", "I"],
        vec![
            order("I", "B", "a.api"),
            order("I", "B", "b.api"),
            order("I", "A", "a.api"),
        ],
    );
    assert_eq!(kept, [order("I", "A", "a.api"), order("I", "B", "b.api")]);
}

#[test]
fn a_dropped_wait_leaves_the_nodes_as_they_were() {
    // Only the orders change: the nodes, and the names they need, stay.
    let mut working = Working {
        plan: PlanInput::of(vec![node("A"), node("B"), node("C")]),
        options: Options::default(),
        orders: [
            order("A", "B", "x"),
            order("B", "C", "y"),
            order("A", "C", "z"),
        ]
        .into_iter()
        .collect(),
    };
    let nodes = working.plan.nodes.clone();
    thin(&mut working);
    assert_eq!(working.plan.nodes, nodes);
    assert_eq!(working.orders.len(), 2);
}

#[test]
fn no_waits_stay_no_waits() {
    assert_eq!(thinned(&["A", "B"], vec![]), []);
}

#[test]
fn waits_that_go_round_are_left_as_they_are() {
    let waits = vec![order("A", "B", "a"), order("B", "A", "b")];
    assert_eq!(thinned(&["A", "B"], waits.clone()), waits);
}

#[test]
fn the_same_waits_in_any_order_thin_the_same() {
    let waits = vec![
        order("A", "C", "a"),
        order("B", "C", "b"),
        order("A", "B", "a"),
    ];
    let mut reversed = waits.clone();
    reversed.reverse();
    assert_eq!(
        thinned(&["A", "B", "C"], waits),
        thinned(&["C", "B", "A"], reversed)
    );
}
