//! The graph as the planner wrote it (I6): `PlanGraph::before` reads an epic's plan records and
//! gives their nodes and the waits their needs and provides derive, with no ticket on a node.

use zaf::domain::plan::graph::Dag;
use zaf::domain::plan::read::{Dropped, PlanGraph};
use zaf::domain::plan::record::PlanRecord;
use zaf::domain::plan::{Name, NodeId, NodeKind, Order};
use zaf::domain::{BodyHash, TicketBody, TicketId};

use super::{Outcome, fixtures};

/// One section of a plan, as a planner writes it.
fn section(node: &str, goal: &str, writes: &str, provides: &str, needs: &str) -> String {
    format!(
        "Task: {goal}\nNode: {node}\nGoal: {goal}\nScope: {goal}\nOut of scope: nothing\n\
         Interfaces: none\nDesign pages: D2\nAcceptance:\n- `{goal}` works\nTests: t_{node}\n\
         Security trigger: no\nModel: sonnet\nSize: S\nWrites: {writes}\n\
         Provides: {provides}\nNeeds: {needs}\n"
    )
}

/// Fixture 12's input, written as a plan: A and B write one area, C provides the catalog and D
/// needs it.
fn plan_12() -> String {
    [
        section("A", "update takes --yes", "app/update", "", "") + "\n",
        section("B", "update prints one summary line", "app/update", "", "") + "\n",
        section(
            "C",
            "the MCP server lists the verbs as tools",
            "adapters/mcp",
            "mcp.catalog",
            "cli",
        ) + "\n",
        section(
            "D",
            "profiles register the MCP server",
            "app/profile",
            "",
            "mcp.catalog",
        ),
    ]
    .concat()
}

fn record(k: u64, dropped: &[u64], plan: String) -> PlanRecord {
    PlanRecord {
        k,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: dropped.iter().copied().map(TicketId::from).collect(),
        plan,
    }
}

fn ids(graph: &PlanGraph) -> Vec<&str> {
    graph.nodes.iter().map(|node| node.id.as_str()).collect()
}

#[test]
fn plan_before_reads_fixture_12() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with("12-"))
        .expect("fixture 12");
    let Outcome::Plan { before, .. } = fixture.expect.outcome() else {
        panic!("fixture 12 is not refused");
    };
    let graph = PlanGraph::before(&[record(1, &[], plan_12())]).expect("the plan reads");

    let mut input: Vec<(NodeId, NodeKind)> = fixture
        .plan
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.kind))
        .collect();
    input.sort();
    let read: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.kind))
        .collect();
    assert_eq!(read, input, "the input's nodes");
    assert!(graph.nodes.iter().all(|node| node.filed.is_none()));

    let waits: Vec<(NodeId, NodeId)> = graph
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    assert_eq!(
        graph.orders.iter().cloned().collect::<Vec<_>>(),
        [Order {
            from: "C".into(),
            to: "D".into(),
            needs: Name::from("mcp.catalog"),
        }],
        "the one wait: `cli` is the code's"
    );
    let layers = Dag::new(
        &graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        &waits,
    )
    .layers();
    assert_eq!(layers.len(), before.chain, "the chain `before` measures");
    assert_eq!(
        layers.iter().map(Vec::len).collect::<Vec<_>>(),
        before.width,
        "the width `before` measures"
    );
    assert!(graph.kept.is_empty());
    assert!(!graph.half_filed);
    assert_eq!(
        graph.dropped,
        [Dropped {
            record: 1,
            tickets: vec![]
        }]
    );
}

#[test]
fn plan_before_joins_records_with_their_dropped_lists() {
    let first = record(1, &[], plan_12());
    let second = record(
        2,
        &[4],
        [
            section("E", "a later task", "app/later", "later.name", "") + "\n",
            section("F", "its follower", "app/follow", "", "later.name"),
        ]
        .concat(),
    );
    let graph = PlanGraph::before(&[first, second]).expect("both records read");

    assert_eq!(
        ids(&graph),
        ["A", "B", "C", "D", "E", "F"],
        "no node is dropped"
    );
    assert_eq!(graph.orders.len(), 2, "each record's own waits");
    assert!(
        graph
            .orders
            .iter()
            .any(|order| order.from.as_str() == "E" && order.to.as_str() == "F")
    );
    assert_eq!(
        graph.dropped,
        [
            Dropped {
                record: 1,
                tickets: vec![]
            },
            Dropped {
                record: 2,
                tickets: vec![TicketId::from(4)]
            },
        ]
    );
}

#[test]
fn plan_before_names_a_bad_record() {
    let bad = record(7, &[], "this is not a plan\n".to_owned());
    let error = PlanGraph::before(&[record(1, &[], plan_12()), bad]).expect_err("record 7 fails");
    assert_eq!(error.record, 7);
    assert!(error.to_string().contains("record 7"), "{error}");
    assert!(!error.faults.0.is_empty());

    let empty = PlanGraph::before(&[]).expect("no record, no node");
    assert!(empty.nodes.is_empty() && empty.dropped.is_empty());
}
