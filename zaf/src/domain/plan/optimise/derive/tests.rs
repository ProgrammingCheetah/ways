use std::collections::BTreeSet;
use std::path::Path;

use super::derive;
use crate::domain::plan::names::normalise;
use crate::domain::plan::optimise::Working;
use crate::domain::plan::{
    Area, Behaviour, Filed, Name, Node, NodeId, NodeKind, Options, Order, PlanInput, Turn,
};

fn names(texts: &[&str]) -> BTreeSet<Name> {
    texts.iter().map(|t| Name::from(*t)).collect()
}

fn node(id: &str, provides: &[&str], needs: &[&str]) -> Node {
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
            provides: names(provides),
            needs: names(needs),
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

fn derived(plan: PlanInput) -> Vec<Order> {
    let mut working = Working {
        plan,
        options: Options::default(),
        orders: BTreeSet::new(),
    };
    derive(&mut working);
    working.orders.into_iter().collect()
}

/// The plan of a fixture, normalised as the optimiser reads it.
fn fixture(file: &str) -> PlanInput {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.teams/releases/0.6.0/plan-graph/fixtures")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text).expect("a fixture is JSON");
    let plan: PlanInput = serde_json::from_value(value["plan"].clone()).expect("a fixture plan");
    normalise(&plan).expect("a fixture plan normalises")
}

#[test]
fn fixture_01_derives_a_to_b_to_c_on_their_names() {
    let orders = derived(fixture("01-chain-becomes-wide.json"));
    assert_eq!(orders, [order("A", "B", "a.api"), order("B", "C", "b.api")]);
}

#[test]
fn fixture_06_existing_need_makes_no_wait() {
    assert_eq!(derived(fixture("06-existing-need-makes-no-edge.json")), []);
}

#[test]
fn a_need_nobody_provides_makes_no_wait() {
    // Another epic's task (`outside`) or existing code: no node of the plan provides the name.
    let mut plan = PlanInput::of(vec![node("A", &[], &["elsewhere"])]);
    plan.outside
        .insert(Name::from("elsewhere"), NodeId::from("#9"));
    assert_eq!(derived(plan), []);
}

#[test]
fn a_node_never_waits_on_itself() {
    let plan = PlanInput::of(vec![node("A", &["x"], &["x"])]);
    assert_eq!(derived(plan), []);
}

#[test]
fn a_name_needed_by_two_nodes_gives_each_its_wait() {
    let plan = PlanInput::of(vec![
        node("C", &[], &["x"]),
        node("B", &[], &["x"]),
        node("A", &["x"], &[]),
    ]);
    assert_eq!(derived(plan), [order("A", "B", "x"), order("A", "C", "x")]);
}

#[test]
fn the_waits_sort_by_natural_from_to_and_name() {
    let plan = PlanInput::of(vec![
        node("A10", &["y"], &[]),
        node("A2", &["x", "z"], &[]),
        node("B", &[], &["z", "x", "y"]),
    ]);
    assert_eq!(
        derived(plan),
        [
            order("A2", "B", "x"),
            order("A2", "B", "z"),
            order("A10", "B", "y"),
        ]
    );
}

#[test]
fn the_listing_order_does_not_matter() {
    let nodes = vec![
        node("A", &["x"], &[]),
        node("B", &["y"], &["x"]),
        node("C", &[], &["x", "y"]),
    ];
    let mut reversed = nodes.clone();
    reversed.reverse();
    assert_eq!(
        derived(PlanInput::of(nodes)),
        derived(PlanInput::of(reversed))
    );
}

#[test]
fn a_filed_nodes_turns_each_derive_a_turn_wait() {
    let mut filed = node("#5", &[], &[]);
    filed.filed = Some(Filed::Open);
    filed.turns = vec![
        Turn(NodeId::from("A"), Area::from("app/update")),
        Turn(NodeId::from("B"), Area::from("cli")),
    ];
    let plan = PlanInput::of(vec![filed, node("A", &[], &[]), node("B", &[], &[])]);
    assert_eq!(
        derived(plan),
        [
            order("A", "#5", "turn:app/update"),
            order("B", "#5", "turn:cli"),
        ]
    );
}

#[test]
fn a_turn_after_a_done_ticket_drops() {
    let mut done = node("#4", &["x"], &[]);
    done.filed = Some(Filed::Done);
    let mut open = node("#5", &[], &["x"]);
    open.filed = Some(Filed::Open);
    open.turns = vec![
        Turn(NodeId::from("#4"), Area::from("cli")),
        Turn(NodeId::from("#6"), Area::from("cli")),
    ];
    let mut following = node("#6", &[], &[]);
    following.filed = Some(Filed::Open);
    // The done ticket leaves the plan when it is normalised, and what it provided is existing.
    let plan = normalise(&PlanInput::of(vec![done, open, following])).expect("normalises");
    assert_eq!(derived(plan), [order("#6", "#5", "turn:cli")]);
}
