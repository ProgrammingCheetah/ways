use std::collections::BTreeSet;

use super::*;
use crate::domain::plan::{Area, Behaviour, EpicKey, Options, PlanInput};

fn behaviour(text: &str, area: &str) -> Behaviour {
    Behaviour {
        text: text.to_owned(),
        size: Default::default(),
        writes: BTreeSet::from([Area::from(area)]),
        provides: BTreeSet::new(),
        needs: BTreeSet::new(),
    }
}

fn node(id: &str, count: usize) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours: (0..count)
            .map(|i| behaviour(&format!("b{i}"), &format!("area{i}")))
            .collect(),
    }
}

fn run(nodes: Vec<Node>) -> Vec<Node> {
    let mut working = Working {
        plan: PlanInput::of(nodes),
        options: Options::default(),
        orders: BTreeSet::new(),
    };
    split(&mut working);
    working.plan.nodes
}

fn ids(nodes: &[Node]) -> Vec<&str> {
    nodes.iter().map(|n| n.id.as_str()).collect()
}

#[test]
fn a_node_of_two_behaviours_splits_into_a_and_b() {
    let out = run(vec![node("A", 2)]);
    assert_eq!(ids(&out), ["A.a", "A.b"]);
    assert_eq!(out[0].behaviours[0].text, "b0");
    assert_eq!(out[1].behaviours[0].text, "b1");
    assert!(out.iter().all(|n| n.behaviours.len() == 1));
}

#[test]
fn a_node_of_one_behaviour_keeps_its_id() {
    assert_eq!(ids(&run(vec![node("A", 1)])), ["A"]);
}

#[test]
fn a_merged_node_splits_back_into_its_parts_in_order() {
    let out = run(vec![node("A+B", 2)]);
    assert_eq!(ids(&out), ["A", "B"]);
    assert_eq!(out[0].behaviours[0].text, "b0");
    assert_eq!(out[1].behaviours[0].text, "b1");
}

#[test]
fn a_merged_node_of_three_splits_into_three() {
    assert_eq!(ids(&run(vec![node("A+B+C", 3)])), ["A", "B", "C"]);
}

#[test]
fn a_merged_id_that_does_not_match_its_behaviours_gets_letters() {
    assert_eq!(ids(&run(vec![node("A+B", 3)])), ["A+B.a", "A+B.b", "A+B.c"]);
}

#[test]
fn parts_follow_z_with_numbers() {
    let out = run(vec![node("A", 28)]);
    let found = ids(&out);
    assert_eq!(out.len(), 28);
    assert!(found.contains(&"A.z"));
    assert!(found.contains(&"A.26"));
    assert!(found.contains(&"A.27"));
}

#[test]
fn an_interface_node_is_not_split() {
    let mut interface = node("I@1.1", 2);
    interface.kind = NodeKind::Interface;
    assert_eq!(ids(&run(vec![interface])), ["I@1.1"]);
}

#[test]
fn an_open_filed_ticket_is_not_split() {
    let mut open = node("#7", 2);
    open.filed = Some(Filed::Open);
    let out = run(vec![open]);
    assert_eq!(ids(&out), ["#7"]);
    assert_eq!(out[0].behaviours.len(), 2);
}

#[test]
fn each_part_keeps_the_other_fields() {
    let mut whole = node("A", 2);
    whole.epic = Some(EpicKey::from("1004"));
    whole.filed = Some(Filed::Done);
    let out = run(vec![whole]);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|n| n.epic == Some(EpicKey::from("1004"))));
    assert!(out.iter().all(|n| n.filed == Some(Filed::Done)));
}

#[test]
fn a_rerun_keeps_the_ids() {
    let first = run(vec![node("A", 2), node("B", 1)]);
    assert_eq!(ids(&first), ["A.a", "A.b", "B"]);
    assert_eq!(ids(&run(vec![node("A+B", 2)])), ["A", "B"]);
}

#[test]
fn the_nodes_come_out_in_natural_id_order() {
    assert_eq!(
        ids(&run(vec![node("A10", 1), node("A9", 1), node("A2", 2)])),
        ["A2.a", "A2.b", "A9", "A10"]
    );
}
