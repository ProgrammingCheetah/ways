use std::collections::BTreeSet;
use std::path::Path;

use super::settle;
use crate::domain::plan::names::normalise;
use crate::domain::plan::optimise::Working;
use crate::domain::plan::optimise::derive::derive;
use crate::domain::plan::{
    Area, Behaviour, Filed, Name, Node, NodeId, NodeKind, Options, Order, PlanInput, Size, Weight,
};

fn node(id: &str, size: u32, writes: &[&str]) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours: vec![Behaviour {
            text: format!("{id} builds"),
            size: Size::Weight(Weight(size)),
            writes: writes.iter().map(|w| Area::from(*w)).collect(),
            provides: BTreeSet::new(),
            needs: BTreeSet::new(),
        }],
    }
}

fn filed(mut node: Node) -> Node {
    node.filed = Some(Filed::Open);
    node
}

fn order(from: &str, to: &str, needs: &str) -> Order {
    Order {
        from: NodeId::from(from),
        to: NodeId::from(to),
        needs: Name::from(needs),
    }
}

fn working(nodes: Vec<Node>, orders: &[Order]) -> Working {
    Working {
        plan: PlanInput::of(nodes),
        options: Options::default(),
        orders: orders.iter().cloned().collect(),
    }
}

fn settled(mut working: Working) -> Working {
    settle(&mut working).expect("no merge takes a held id");
    working
}

fn ids(working: &Working) -> Vec<String> {
    working
        .plan
        .nodes
        .iter()
        .map(|node| node.id.to_string())
        .collect()
}

/// A fixture as the optimiser reads it before settling: normalised, split and derived.
fn fixture(file: &str) -> (Working, serde_json::Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.teams/releases/0.6.0/plan-graph/fixtures")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text).expect("a fixture is JSON");
    let plan: PlanInput = serde_json::from_value(value["plan"].clone()).expect("a fixture plan");
    let plan = normalise(&plan).expect("a fixture plan normalises");
    let mut working = Working {
        plan,
        options: Options::default(),
        orders: BTreeSet::new(),
    };
    derive(&mut working);
    let expect = value["expect"]["plan"].clone();
    (working, expect)
}

fn expected_nodes(expect: &serde_json::Value) -> Vec<Node> {
    serde_json::from_value(expect["nodes"].clone()).expect("the expected nodes")
}

fn expected_orders(expect: &serde_json::Value) -> BTreeSet<Order> {
    serde_json::from_value(expect["orders"].clone()).expect("the expected orders")
}

#[test]
fn fixture_02_two_small_writers_merge_into_one_node() {
    let (before, expect) = fixture("02-conflict-merges.json");
    let after = settled(before);
    assert_eq!(after.plan.nodes, expected_nodes(&expect));
    assert_eq!(ids(&after), ["A+B"]);
    assert_eq!(after.orders, expected_orders(&expect));
}

#[test]
fn fixture_03_a_pair_too_big_to_merge_takes_turns() {
    let (before, expect) = fixture("03-conflict-too-big-is-ordered.json");
    let after = settled(before);
    assert_eq!(after.plan.nodes, expected_nodes(&expect));
    assert_eq!(
        after.orders,
        BTreeSet::from([order("A", "B", "turn:app/update")])
    );
}

#[test]
fn fixture_14_the_node_more_nodes_wait_on_goes_first() {
    let (before, expect) = fixture("14-more-waiters-goes-first.json");
    let after = settled(before);
    assert_eq!(after.plan.nodes, expected_nodes(&expect));
    assert_eq!(after.orders, expected_orders(&expect));
    assert!(after.orders.contains(&order("B", "A", "turn:shared")));
}

#[test]
fn fixture_16_a_pair_already_ordered_is_left_alone() {
    let (before, expect) = fixture("16-ordered-conflict-is-settled.json");
    let orders = before.orders.clone();
    let after = settled(before);
    assert_eq!(after.plan.nodes, expected_nodes(&expect));
    assert_eq!(after.orders, orders);
    assert_eq!(after.orders, expected_orders(&expect));
}

/// Fixture 27 as it stands once `lift` has run: `s` and `y` and `t` have their interface
/// nodes and the waits go through them.
fn fixture_27_lifted() -> Working {
    let (mut working, _) = fixture("27-replan-adds-beside-filed.json");
    let mut interface = node("I@1004.2", 1, &["sig:t", "sig:y"]);
    interface.kind = NodeKind::Interface;
    working.plan.nodes.push(interface);
    for node in &mut working.plan.nodes {
        if node.id.as_str() == "C" || node.id.as_str() == "Y2" {
            node.behaviours[0].provides.clear();
        }
    }
    working.orders = BTreeSet::from([
        order("I@1004.1", "A", "s"),
        order("I@1004.1", "C", "s"),
        order("I@1004.2", "C", "t"),
        order("I@1004.2", "E", "t"),
        order("I@1004.2", "X", "y"),
        order("I@1004.2", "Y2", "y"),
    ]);
    working.plan.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    working
}

#[test]
fn fixture_27_a_new_task_takes_its_turn_after_the_filed_ticket() {
    let after = settled(fixture_27_lifted());
    assert!(after.orders.contains(&order("A", "C", "turn:cli")));
    assert!(after.orders.contains(&order("X", "Y2", "turn:hub")));
    // Never merged with a filed ticket.
    for id in ["A", "C", "X", "Y2"] {
        assert!(ids(&after).contains(&id.to_owned()), "{id} stays");
    }
    assert_eq!(after.plan.nodes.len(), 7, "no node was merged away");
}

#[test]
fn a_filed_ticket_and_a_new_node_take_turns_whatever_their_weights() {
    let nodes = vec![node("A", 1, &["x"]), filed(node("B", 1, &["x"]))];
    let after = settled(working(nodes, &[]));
    assert_eq!(ids(&after), ["A", "B"]);
    assert_eq!(after.orders, BTreeSet::from([order("B", "A", "turn:x")]));
}

#[test]
fn two_filed_tickets_are_skipped() {
    let nodes = vec![filed(node("A", 1, &["x"])), filed(node("B", 1, &["x"]))];
    let after = settled(working(nodes, &[]));
    assert_eq!(ids(&after), ["A", "B"]);
    assert_eq!(after.orders, BTreeSet::new());
}

#[test]
fn the_turn_is_on_the_lowest_shared_area() {
    let nodes = vec![
        node("A", 2, &["b", "a", "z"]),
        node("B", 1, &["c", "b", "a"]),
    ];
    let after = settled(working(nodes, &[]));
    assert_eq!(after.orders, BTreeSet::from([order("A", "B", "turn:a")]));
}

#[test]
fn a_tie_goes_to_the_lower_id() {
    let nodes = vec![node("B", 2, &["x"]), node("A", 2, &["x"])];
    let after = settled(working(nodes, &[]));
    assert_eq!(after.orders, BTreeSet::from([order("A", "B", "turn:x")]));
}

#[test]
fn the_lower_id_is_a_natural_one() {
    let nodes = vec![node("A10", 2, &["x"]), node("A9", 2, &["x"])];
    let after = settled(working(nodes, &[]));
    assert_eq!(after.orders, BTreeSet::from([order("A9", "A10", "turn:x")]));
}

#[test]
fn the_count_of_waiters_is_the_nodes_reached_through_any_chain() {
    // B has two nodes behind it by a chain; A has one.
    let nodes = vec![
        node("A", 2, &["x"]),
        node("B", 2, &["x"]),
        node("C", 1, &["c"]),
        node("D", 1, &["d"]),
        node("E", 1, &["e"]),
    ];
    let orders = [
        order("A", "C", "ac"),
        order("B", "D", "bd"),
        order("D", "E", "de"),
    ];
    let after = settled(working(nodes, &orders));
    assert!(after.orders.contains(&order("B", "A", "turn:x")));
}

#[test]
fn a_merge_moves_the_waits_to_the_new_node_and_keeps_no_wait_on_itself() {
    let nodes = vec![
        node("A", 1, &["x"]),
        node("B", 1, &["x"]),
        node("C", 1, &["c"]),
        node("D", 1, &["d"]),
    ];
    let orders = [order("C", "A", "ca"), order("B", "D", "bd")];
    let after = settled(working(nodes, &orders));
    assert_eq!(ids(&after), ["A+B", "C", "D"]);
    assert_eq!(
        after.orders,
        BTreeSet::from([order("C", "A+B", "ca"), order("A+B", "D", "bd")])
    );
}

#[test]
fn a_merge_holds_the_behaviours_in_order() {
    let nodes = vec![node("B", 1, &["x"]), node("A", 1, &["x"])];
    let after = settled(working(nodes, &[]));
    let texts: Vec<&str> = after.plan.nodes[0]
        .behaviours
        .iter()
        .map(|b| b.text.as_str())
        .collect();
    assert_eq!(texts, ["A builds", "B builds"]);
}

#[test]
fn a_weight_at_the_limit_merges_and_one_over_does_not() {
    let fits = settled(working(
        vec![node("A", 1, &["x"]), node("B", 1, &["x"])],
        &[],
    ));
    assert_eq!(ids(&fits), ["A+B"]);
    let over = settled(working(
        vec![node("A", 2, &["x"]), node("B", 1, &["x"])],
        &[],
    ));
    assert_eq!(ids(&over), ["A", "B"]);
}

#[test]
fn a_merge_that_makes_a_new_pair_is_followed_until_none_is_left() {
    // A and B merge, and A+B then conflicts with C on y; the limit stops a second merge.
    let nodes = vec![
        node("A", 1, &["x", "y"]),
        node("B", 1, &["x"]),
        node("C", 1, &["y"]),
    ];
    let after = settled(working(nodes, &[]));
    assert_eq!(ids(&after), ["A+B", "C"]);
    assert_eq!(after.orders, BTreeSet::from([order("A+B", "C", "turn:y")]));
}

#[test]
fn an_interface_node_is_ordered_and_never_merged() {
    let mut interface = node("I", 1, &["x"]);
    interface.kind = NodeKind::Interface;
    let after = settled(working(vec![interface, node("A", 1, &["x"])], &[]));
    assert_eq!(ids(&after), ["A", "I"]);
    assert_eq!(after.orders, BTreeSet::from([order("A", "I", "turn:x")]));
}

#[test]
fn nodes_that_share_no_area_are_left_alone() {
    let after = settled(working(
        vec![node("A", 1, &["x"]), node("B", 1, &["y"])],
        &[],
    ));
    assert_eq!(ids(&after), ["A", "B"]);
    assert_eq!(after.orders, BTreeSet::new());
}

#[test]
fn the_listing_order_does_not_change_the_result() {
    let forward = settled(working(
        vec![
            node("A", 2, &["x"]),
            node("B", 2, &["x"]),
            node("C", 1, &["x"]),
        ],
        &[],
    ));
    let backward = settled(working(
        vec![
            node("C", 1, &["x"]),
            node("B", 2, &["x"]),
            node("A", 2, &["x"]),
        ],
        &[],
    ));
    assert_eq!(forward.plan.nodes, backward.plan.nodes);
    assert_eq!(forward.orders, backward.orders);
}

#[test]
fn waits_that_go_round_are_left_as_they_are() {
    // The refusal pass keeps such a plan out; settle does not panic on one.
    let nodes = vec![node("A", 1, &["x"]), node("B", 1, &["x"])];
    let orders = [order("A", "B", "ab"), order("B", "A", "ba")];
    let after = settled(working(nodes, &orders));
    assert_eq!(ids(&after), ["A", "B"]);
    assert_eq!(after.orders.len(), 2);
}

/// D29: a filed ticket `A+B` stands, and a re-plan's `A` and `B` would merge into its id. The
/// plan is refused; it was two nodes of one id, a wait of the id on itself, and a panic in the
/// measure's layers.
#[test]
fn a_merge_into_a_filed_tickets_id_is_refused() {
    use crate::domain::plan::optimise::optimise;
    use crate::domain::plan::{RefusalCode, measure::measure};

    let mut held = node("A+B", 1, &["cli"]);
    held.behaviours.push(held.behaviours[0].clone());
    let plan = PlanInput::of(vec![
        filed(held),
        node("A", 1, &["cli"]),
        node("B", 1, &["cli"]),
    ]);
    let refusals = optimise(&plan, Options::default()).expect_err("refused");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, RefusalCode::DuplicateId);
    assert!(
        refusals[0].detail.contains("A+B") && refusals[0].detail.contains("other keys"),
        "{}",
        refusals[0].detail
    );

    // With other keys the same re-plan is taken, and it measures.
    let plan = PlanInput::of(vec![
        filed({
            let mut held = node("A+B", 1, &["cli"]);
            held.behaviours.push(held.behaviours[0].clone());
            held
        }),
        node("A2", 1, &["cli"]),
        node("B2", 1, &["cli"]),
    ]);
    let optimised = optimise(&plan, Options::default()).expect("taken");
    let ids: Vec<&str> = optimised.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["A2+B2", "A+B"]);
    assert_eq!(measure(&optimised, plan.lanes).nodes, 2);
}
