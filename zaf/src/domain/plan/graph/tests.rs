use std::collections::{BTreeMap, BTreeSet};

use super::{Dag, NodeId};

fn id(text: &str) -> NodeId {
    NodeId::from(text)
}

fn ids(texts: &[&str]) -> Vec<NodeId> {
    texts.iter().map(|t| id(t)).collect()
}

fn edges(pairs: &[(&str, &str)]) -> Vec<(NodeId, NodeId)> {
    pairs.iter().map(|(a, b)| (id(a), id(b))).collect()
}

/// A diamond with a shortcut: A before B and C, both before D, and A before D as well; E alone;
/// F after E. Ids that read as numbers, so the natural order is tried.
fn sample() -> (Vec<NodeId>, Vec<(NodeId, NodeId)>) {
    (
        ids(&["A10", "A2", "A3", "A4", "A9", "A1"]),
        edges(&[
            ("A2", "A3"),
            ("A2", "A4"),
            ("A3", "A10"),
            ("A4", "A10"),
            ("A2", "A10"),
            ("A9", "A1"),
        ]),
    )
}

fn reversed<T: Clone>(items: &[T]) -> Vec<T> {
    items.iter().rev().cloned().collect()
}

#[test]
fn the_five_answers_are_the_same_for_two_orders() {
    let (nodes, waits) = sample();
    let one = Dag::new(&nodes, &waits);
    let two = Dag::new(&reversed(&nodes), &reversed(&waits));
    assert_eq!(one, two);
    assert_eq!(one.cycle(), two.cycle());
    assert_eq!(one.reduction(), two.reduction());
    assert_eq!(one.closure(), two.closure());
    assert_eq!(one.order(), two.order());
    assert_eq!(one.layers(), two.layers());
}

#[test]
fn the_five_answers_are_the_same_for_two_orders_of_a_graph_with_a_cycle() {
    let nodes = ids(&["B", "A", "C", "D"]);
    let waits = edges(&[("A", "B"), ("B", "C"), ("C", "A"), ("C", "D")]);
    let one = Dag::new(&nodes, &waits);
    let two = Dag::new(&reversed(&nodes), &reversed(&waits));
    assert_eq!(one, two);
    assert_eq!(one.cycle(), two.cycle());
}

#[test]
fn a_cycle_is_named_sorted() {
    let nodes = ids(&["D", "A10", "A9", "A2"]);
    let waits = edges(&[("A9", "A10"), ("A10", "A2"), ("A2", "A9"), ("A2", "D")]);
    let dag = Dag::new(&nodes, &waits);
    assert_eq!(dag.cycle(), Some(ids(&["A2", "A9", "A10"])));
}

#[test]
fn a_node_that_waits_on_itself_is_a_cycle() {
    let dag = Dag::new(&ids(&["A", "B"]), &edges(&[("B", "B")]));
    assert_eq!(dag.cycle(), Some(ids(&["B"])));
}

#[test]
fn a_dag_has_no_cycle() {
    let (nodes, waits) = sample();
    assert_eq!(Dag::new(&nodes, &waits).cycle(), None);
    assert_eq!(Dag::default().cycle(), None);
}

#[test]
fn the_reduction_leaves_out_a_wait_another_path_gives() {
    let (nodes, waits) = sample();
    let reduction = Dag::new(&nodes, &waits).reduction();
    let expected: BTreeSet<(NodeId, NodeId)> = edges(&[
        ("A2", "A3"),
        ("A2", "A4"),
        ("A3", "A10"),
        ("A4", "A10"),
        ("A9", "A1"),
    ])
    .into_iter()
    .collect();
    assert_eq!(reduction, expected);
}

#[test]
fn the_closure_holds_every_node_and_all_that_follows_it() {
    let (nodes, waits) = sample();
    let closure = Dag::new(&nodes, &waits).closure();
    let of = |texts: &[&str]| -> BTreeSet<NodeId> { ids(texts).into_iter().collect() };
    let expected: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::from([
        (id("A1"), of(&[])),
        (id("A2"), of(&["A3", "A4", "A10"])),
        (id("A3"), of(&["A10"])),
        (id("A4"), of(&["A10"])),
        (id("A9"), of(&["A1"])),
        (id("A10"), of(&[])),
    ]);
    assert_eq!(closure, expected);
}

#[test]
fn the_order_takes_the_lowest_natural_id_that_is_free() {
    let (nodes, waits) = sample();
    assert_eq!(
        Dag::new(&nodes, &waits).order(),
        ids(&["A2", "A3", "A4", "A9", "A1", "A10"])
    );
}

#[test]
#[should_panic(expected = "go round")]
fn the_order_of_a_graph_with_a_cycle_is_refused() {
    let dag = Dag::new(&ids(&["A", "B", "C"]), &edges(&[("B", "C"), ("C", "B")]));
    dag.order();
}

#[test]
#[should_panic(expected = "go round")]
fn the_layers_of_a_graph_with_a_cycle_are_refused() {
    let dag = Dag::new(&ids(&["A", "B", "C"]), &edges(&[("B", "C"), ("C", "B")]));
    dag.layers();
}

#[test]
#[should_panic(expected = "go round")]
fn the_reduction_of_a_graph_with_a_cycle_is_refused() {
    let dag = Dag::new(&ids(&["A", "B"]), &edges(&[("A", "B"), ("B", "A")]));
    dag.reduction();
}

#[test]
fn the_layers_hold_the_sources_first_and_each_is_sorted() {
    let (nodes, waits) = sample();
    assert_eq!(
        Dag::new(&nodes, &waits).layers(),
        vec![ids(&["A2", "A9"]), ids(&["A1", "A3", "A4"]), ids(&["A10"])]
    );
}

#[test]
fn a_node_given_twice_is_kept_once_and_an_edge_to_a_stranger_is_left_out() {
    let nodes = ids(&["A", "B", "A"]);
    let waits = edges(&[("A", "B"), ("A", "B"), ("B", "Z")]);
    let dag = Dag::new(&nodes, &waits);
    assert_eq!(dag.order(), ids(&["A", "B"]));
    assert_eq!(dag.reduction(), edges(&[("A", "B")]).into_iter().collect());
}

#[test]
fn an_empty_graph_answers_empty() {
    let dag = Dag::default();
    assert_eq!(dag.order(), Vec::<NodeId>::new());
    assert_eq!(dag.layers(), Vec::<Vec<NodeId>>::new());
    assert!(dag.reduction().is_empty());
    assert!(dag.closure().is_empty());
}
