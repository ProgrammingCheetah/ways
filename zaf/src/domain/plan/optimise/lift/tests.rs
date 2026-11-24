use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use crate::domain::plan::names::normalise;
use crate::domain::plan::optimise::{Working, derive, split};
use crate::domain::plan::{EpicKey, Options};

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
            text: format!("{id} works"),
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

/// The plan after split, derive and lift, as the optimiser runs them.
fn lifted(plan: PlanInput, options: Options) -> Working {
    let mut working = Working {
        plan,
        options,
        orders: BTreeSet::new(),
    };
    split::split(&mut working);
    derive::derive(&mut working);
    lift(&mut working);
    working
}

fn one() -> Options {
    Options {
        lift: Lift::All,
        interfaces: Interfaces::One,
    }
}

fn fixture(file: &str) -> (serde_json::Value, PlanInput) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.teams/releases/0.6.0/plan-graph/fixtures")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text).expect("a fixture is JSON");
    let plan: PlanInput = serde_json::from_value(value["plan"].clone()).expect("a fixture plan");
    (value, normalise(&plan).expect("a fixture plan normalises"))
}

fn options_of(value: &serde_json::Value) -> Options {
    serde_json::from_value(value["options"].clone()).expect("fixture options")
}

/// The nodes a fixture expects of the plan, which no later pass changes in these fixtures.
fn expected_nodes(value: &serde_json::Value) -> Vec<Node> {
    let mut nodes: Vec<Node> =
        serde_json::from_value(value["expect"]["plan"]["nodes"].clone()).expect("expected nodes");
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    nodes
}

fn node_of<'a>(working: &'a Working, id: &str) -> &'a Node {
    working
        .plan
        .nodes
        .iter()
        .find(|n| n.id.as_str() == id)
        .unwrap_or_else(|| panic!("no node {id}"))
}

#[test]
fn fixture_01_lifts_the_chain_into_two_interface_nodes() {
    let (value, plan) = fixture("01-chain-becomes-wide.json");
    let working = lifted(plan, options_of(&value));
    assert_eq!(working.plan.nodes, expected_nodes(&value));
    let orders: Vec<Order> = working.orders.into_iter().collect();
    assert_eq!(
        orders,
        [
            order("I:a.api", "A", "a.api"),
            order("I:a.api", "B", "a.api"),
            order("I:b.api", "B", "b.api"),
            order("I:b.api", "C", "b.api"),
        ]
    );
}

#[test]
fn fixture_01_with_one_interface_node_holds_both_signatures() {
    let (_, plan) = fixture("01-chain-becomes-wide.json");
    let working = lifted(plan, one());
    let interface = node_of(&working, "I");
    assert_eq!(interface.kind, NodeKind::Interface);
    let texts: Vec<&str> = interface
        .behaviours
        .iter()
        .map(|b| b.text.as_str())
        .collect();
    assert_eq!(texts, ["the signature of a.api", "the signature of b.api"]);
    assert_eq!(working.plan.nodes.len(), 4);
    assert!(working.orders.iter().all(|o| o.from.as_str() == "I"));
}

#[test]
fn fixture_05_a_name_with_no_signature_stays_a_body_to_body_wait() {
    let (value, plan) = fixture("05-no-signature-stays-an-edge.json");
    let working = lifted(plan, options_of(&value));
    assert_eq!(working.plan.nodes, expected_nodes(&value));
    let orders: Vec<Order> = working.orders.into_iter().collect();
    assert_eq!(orders, [order("A", "B", "prompt.fix")]);
}

#[test]
fn fixture_29_a_shared_name_is_lifted_though_nothing_needs_it() {
    let (value, plan) = fixture("29-a-name-other-epics-need-is-lifted.json");
    let working = lifted(plan, options_of(&value));
    assert_eq!(working.plan.nodes, expected_nodes(&value));
    let orders: Vec<Order> = working.orders.into_iter().collect();
    assert_eq!(orders, [order("I@1004.1", "A", "mcp.catalog")]);
}

#[test]
fn fixture_27_a_replan_lifts_into_the_records_interface_never_a_filed_one() {
    let (value, plan) = fixture("27-replan-adds-beside-filed.json");
    let working = lifted(plan, options_of(&value));
    assert_eq!(working.plan.nodes, expected_nodes(&value));
    // `s` stays with the filed interface ticket; `t` and `y` go into the new one.
    let filed = node_of(&working, "I@1004.1");
    assert_eq!(filed.filed, Some(Filed::Open));
    assert_eq!(filed.behaviours.len(), 1);
    let fresh = node_of(&working, "I@1004.2");
    assert_eq!(fresh.filed, None);
    assert_eq!(fresh.behaviours.len(), 2);
    let orders: BTreeSet<Order> = working.orders;
    for expected in [
        order("I@1004.1", "A", "s"),
        order("I@1004.2", "C", "t"),
        order("I@1004.2", "E", "t"),
        order("I@1004.2", "X", "y"),
    ] {
        assert!(orders.contains(&expected), "missing {expected:?}");
    }
}

#[test]
fn a_name_an_open_filed_ticket_provides_is_never_lifted() {
    let mut filed = node("F", &["f.api"], &[]);
    filed.filed = Some(Filed::Open);
    let mut plan = PlanInput::of(vec![filed, node("U", &[], &["f.api"])]);
    plan.shared = names(&["f.api"]);
    let working = lifted(plan, one());
    assert_eq!(working.plan.nodes.len(), 2);
    assert!(working.plan.nodes.iter().all(|n| n.kind == NodeKind::Body));
    assert_eq!(
        working.orders.into_iter().collect::<Vec<_>>(),
        [order("F", "U", "f.api")]
    );
}

#[test]
fn another_epics_name_is_never_lifted() {
    let mut plan = PlanInput::of(vec![node("A", &[], &["theirs.api"])]);
    plan.outside
        .insert(Name::from("theirs.api"), NodeId::from("#9"));
    plan.shared = names(&["theirs.api"]);
    let working = lifted(plan, one());
    assert_eq!(working.plan.nodes.len(), 1);
    assert!(working.orders.is_empty());
}

#[test]
fn lift_none_does_nothing() {
    let plan = PlanInput::of(vec![node("A", &["a.api"], &[]), node("B", &[], &["a.api"])]);
    let working = lifted(
        plan,
        Options {
            lift: Lift::None,
            interfaces: Interfaces::One,
        },
    );
    assert_eq!(working.plan.nodes.len(), 2);
    assert_eq!(
        working.orders.into_iter().collect::<Vec<_>>(),
        [order("A", "B", "a.api")]
    );
}

#[test]
fn the_provider_stops_providing_and_needs_the_name() {
    let plan = PlanInput::of(vec![node("A", &["a.api"], &[]), node("B", &[], &["a.api"])]);
    let working = lifted(plan, one());
    let provider = &node_of(&working, "A").behaviours[0];
    assert!(provider.provides.is_empty());
    assert_eq!(provider.needs, names(&["a.api"]));
    let signature = &node_of(&working, "I").behaviours[0];
    assert_eq!(signature.text, "the signature of a.api");
    assert_eq!(signature.size, Default::default());
    assert_eq!(signature.writes, BTreeSet::from([Area::from("sig:a.api")]));
    assert_eq!(signature.provides, names(&["a.api"]));
    assert!(signature.needs.is_empty());
}

#[test]
fn the_interface_id_follows_the_owners_epic_and_the_record() {
    let mut a = node("A", &["a.api"], &[]);
    a.epic = Some(EpicKey::from("7"));
    let mut plan = PlanInput::of(vec![a, node("B", &[], &["a.api"])]);
    plan.record = 3;
    let working = lifted(plan, one());
    let interface = node_of(&working, "I@7.3");
    assert_eq!(interface.epic, Some(EpicKey::from("7")));

    assert_eq!(interface_id(None, 1).as_str(), "I");
    assert_eq!(interface_id(None, 2).as_str(), "I.2");
    assert_eq!(interface_id(Some("1004"), 2).as_str(), "I@1004.2");
}

#[test]
fn a_name_on_two_waits_is_lifted_once_and_the_nodes_come_out_in_id_order() {
    let plan = PlanInput::of(vec![
        node("A", &["a.api"], &[]),
        node("B", &[], &["a.api"]),
        node("C", &[], &["a.api"]),
    ]);
    let working = lifted(plan, one());
    let ids: Vec<&str> = working.plan.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["A", "B", "C", "I"]);
    assert_eq!(node_of(&working, "I").behaviours.len(), 1);
    assert_eq!(working.orders.len(), 3);
}
