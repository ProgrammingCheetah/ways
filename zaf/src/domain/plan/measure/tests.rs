use std::collections::BTreeSet;

use super::{baseline, measure};
use crate::domain::plan::{
    Behaviour, Measures, Name, Node, NodeId, NodeKind, Optimised, Order, PlanInput, Size, Weight,
};

fn names(texts: &[&str]) -> BTreeSet<Name> {
    texts.iter().map(|t| Name::from(*t)).collect()
}

fn node(id: &str, size: u32, provides: &[&str], needs: &[&str]) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours: vec![Behaviour {
            text: "a behaviour".to_owned(),
            size: Size::Weight(Weight(size)),
            writes: BTreeSet::new(),
            provides: names(provides),
            needs: names(needs),
        }],
    }
}

fn order(from: &str, to: &str) -> Order {
    Order {
        from: NodeId::from(from),
        to: NodeId::from(to),
        needs: Name::from("n"),
    }
}

fn plan(nodes: Vec<Node>, orders: &[(&str, &str)]) -> Optimised {
    Optimised {
        nodes,
        orders: orders.iter().map(|(a, b)| order(a, b)).collect(),
        kept: BTreeSet::new(),
    }
}

fn path(texts: &[&str]) -> Vec<NodeId> {
    texts.iter().map(|t| NodeId::from(*t)).collect()
}

#[test]
fn empty_plan_measures_nothing() {
    assert_eq!(measure(&Optimised::default(), 4), Measures::default());
}

#[test]
fn layers_chain_width_and_narrow_layers() {
    let p = plan(
        vec![
            node("A", 1, &[], &[]),
            node("B", 1, &[], &[]),
            node("C", 1, &[], &[]),
            node("D", 1, &[], &[]),
            node("E", 1, &[], &[]),
        ],
        &[("A", "D"), ("B", "D"), ("D", "E"), ("C", "E")],
    );
    let m = measure(&p, 3);
    assert_eq!(m.nodes, 5);
    assert_eq!(m.chain, 3);
    assert_eq!(m.width, [3, 1, 1]);
    assert_eq!(m.narrow_layers, [1, 2]);
    assert_eq!(m.work, 5);
    assert_eq!(m.critical, 3);
    assert_eq!(measure(&p, 1).narrow_layers, Vec::<usize>::new());
}

#[test]
fn critical_weighs_by_size_not_by_layers() {
    // A heavy node on a short chain beats a long chain of small ones.
    let p = plan(
        vec![
            node("A", 1, &[], &[]),
            node("B", 1, &[], &[]),
            node("C", 1, &[], &[]),
            node("H", 5, &[], &[]),
        ],
        &[("A", "B"), ("B", "C")],
    );
    let m = measure(&p, 4);
    assert_eq!(m.chain, 3);
    assert_eq!(m.critical, 5);
    assert_eq!(m.critical_path, path(&["H"]));
}

#[test]
fn interface_nodes_are_counted_and_weigh_a_signature_each() {
    let mut interface = node("I", 1, &[], &[]);
    interface.kind = NodeKind::Interface;
    interface.behaviours.push(interface.behaviours[0].clone());
    let p = plan(vec![interface, node("A", 2, &[], &[])], &[("I", "A")]);
    let m = measure(&p, 4);
    assert_eq!(m.interfaces, 1);
    assert_eq!(m.work, 4);
    assert_eq!(m.critical, 4);
    assert_eq!(m.critical_path, path(&["I", "A"]));
}

#[test]
fn critical_path_ties_go_to_the_lower_id_at_the_end_and_at_each_step_back() {
    // A2 and A10 tie before C; E and G tie as ends (fixture 20's shape); ids read naturally.
    let p = plan(
        vec![
            node("A2", 2, &[], &[]),
            node("A10", 2, &[], &[]),
            node("C", 1, &[], &[]),
            node("E", 1, &[], &[]),
            node("G", 1, &[], &[]),
        ],
        &[("A2", "C"), ("A10", "C"), ("C", "E"), ("C", "G")],
    );
    let m = measure(&p, 4);
    assert_eq!(m.critical, 4);
    assert_eq!(m.critical_path, path(&["A2", "C", "E"]));
}

#[test]
fn a_wait_on_a_node_off_the_chain_is_not_followed() {
    let p = plan(
        vec![
            node("A", 3, &[], &[]),
            node("B", 1, &[], &[]),
            node("C", 1, &[], &[]),
        ],
        &[("A", "C"), ("B", "C")],
    );
    assert_eq!(measure(&p, 4).critical_path, path(&["A", "C"]));
}

#[test]
fn baseline_derives_the_waits_from_the_names() {
    let input = PlanInput::of(vec![
        node("B", 1, &[], &["a"]),
        node("A", 2, &["a"], &[]),
        node("C", 1, &[], &[]),
    ]);
    let m = baseline(&input).expect("the plan measures");
    assert_eq!(m.chain, 2);
    assert_eq!(m.width, [2, 1]);
    assert_eq!(m.critical, 3);
    assert_eq!(m.critical_path, path(&["A", "B"]));
}

#[test]
fn baseline_refuses_a_cycle() {
    let input = PlanInput::of(vec![
        node("A", 1, &["a"], &["b"]),
        node("B", 1, &["b"], &["a"]),
    ]);
    let refusal = baseline(&input).expect_err("the waits go round");
    assert_eq!(refusal.to_string(), "cycle: A B");
}
