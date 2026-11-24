//! O3's structural check (D24, I13): a graph built to break each of the five rules gives that one
//! failure, naming the rule and the node, and fixture 12's output graph gives none.

use std::collections::BTreeSet;

use zaf::domain::plan::read::{Filed, KeptRead, PlanGraph, PlanNode, WaitTarget};
use zaf::domain::plan::record::{FilingMark, PlanRecord};
use zaf::domain::plan::shape::{Change, ShapeFailure, ShapeRule, Waited, check_shape};
use zaf::domain::plan::{Area, Name, NodeId, NodeKind, Order, Vocabulary, WaitOn, Weight};
use zaf::domain::{BodyHash, EpicRecord, Standing, TicketBody, TicketId};
use zaf::fakes::FakeBoard;
use zaf::ports::Board;

use super::read::{board, file, output, read};

fn filed(ticket: u64, standing: Standing) -> Filed {
    Filed {
        ticket: TicketId::from(ticket),
        standing,
        body: BodyHash::of(&TicketBody::from("a ticket")),
    }
}

fn node(id: &str, kind: NodeKind, weight: u32, writes: &[&str]) -> PlanNode {
    PlanNode {
        id: NodeId::from(id),
        kind,
        filed: Some(filed(100, Standing::Open)),
        weight: Weight(weight),
        writes: writes.iter().map(|area| Area::from(*area)).collect(),
    }
}

fn body(id: &str) -> PlanNode {
    node(id, NodeKind::Body, 1, &[])
}

fn interface(id: &str) -> PlanNode {
    node(id, NodeKind::Interface, 1, &[])
}

fn order(from: &str, to: &str, needs: &str) -> Order {
    Order {
        from: NodeId::from(from),
        to: NodeId::from(to),
        needs: Name::from(needs),
    }
}

fn graph(nodes: Vec<PlanNode>, orders: Vec<Order>, kept: Vec<KeptRead>) -> PlanGraph {
    PlanGraph::of(
        nodes,
        orders.into_iter().collect(),
        kept.into_iter().collect(),
    )
}

fn names(list: &[&str]) -> BTreeSet<Name> {
    list.iter().map(|name| Name::from(*name)).collect()
}

/// A vocabulary where `mcp.catalog` has a signature and `prompt.fix` has none.
fn vocabulary() -> Vocabulary {
    Vocabulary {
        signature: names(&["mcp.catalog"]),
        no_signature: names(&["prompt.fix"]),
        existing: names(&["cli"]),
        ..Vocabulary::default()
    }
}

fn kept(to: &str, name: &str, on: WaitTarget) -> KeptRead {
    KeptRead {
        to: NodeId::from(to),
        name: Name::from(name),
        line: WaitOn::Task(TicketId::from(7)),
        on,
    }
}

fn record(plan: String) -> PlanRecord {
    PlanRecord {
        k: 1,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: Vec::new(),
        plan,
    }
}

/// Plan record 1 with its filing mark, which holds `graph`'s hash: the epic as zaf filed it.
fn filed_as(graph: &PlanGraph, plan: String) -> EpicRecord {
    EpicRecord {
        record: record(plan),
        posted_at: None,
        mark: Some(FilingMark {
            k: 1,
            graph: graph.hash(),
        }),
    }
}

#[test]
fn shape_fails_a_signature_wait_not_through_an_interface() {
    let nodes = vec![body("A"), body("C")];
    let graph = graph(nodes, vec![order("A", "C", "mcp.catalog")], vec![]);

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::SignatureThroughInterface {
            to: NodeId::from("C"),
            on: Waited::Node(NodeId::from("A")),
            name: Name::from("mcp.catalog"),
        }]
    );
    assert_eq!(failures[0].rule(), ShapeRule::SignatureThroughInterface);
    assert_eq!(
        failures[0].to_string(),
        "signature-through-interface: C waits on A for mcp.catalog, a name with a signature, \
         and A is not an interface ticket"
    );
}

#[test]
fn shape_fails_a_kept_wait_on_a_body_for_a_signature() {
    // Another epic's body ticket #7, which is not in this graph.
    let on = WaitTarget::Ticket {
        ticket: TicketId::from(7),
        node: Some(NodeId::from("B")),
        interface: false,
    };
    let graph = graph(vec![body("D")], vec![], vec![kept("D", "mcp.catalog", on)]);

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::SignatureThroughInterface {
            to: NodeId::from("D"),
            on: Waited::Ticket(TicketId::from(7)),
            name: Name::from("mcp.catalog"),
        }]
    );
    assert_eq!(failures[0].nodes(), [&NodeId::from("D")]);
}

/// D4: a filed task's number `#<n>` has no signature, so a kept wait on that task for it (D13)
/// breaks no rule, though no list of the vocabulary holds the name.
#[test]
fn shape_passes_a_kept_wait_on_a_filed_task_by_number() {
    let task = TicketId::from(1012);
    let wait = KeptRead {
        to: NodeId::from("D"),
        name: Name::of_task(task),
        line: WaitOn::Task(task),
        on: WaitTarget::Ticket {
            ticket: task,
            node: None,
            interface: false,
        },
    };
    let graph = graph(vec![body("D")], vec![], vec![wait]);

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(failures, []);
}

#[test]
fn shape_passes_a_kept_wait_on_an_interface_and_on_an_epic_with_no_ticket() {
    let on = WaitTarget::Ticket {
        ticket: TicketId::from(7),
        node: Some(NodeId::from("I@1004.1")),
        interface: true,
    };
    let epic = WaitTarget::Epic(TicketId::from(1004));
    let graph = graph(
        vec![body("D"), body("E")],
        vec![],
        vec![kept("D", "mcp.catalog", on), kept("E", "mcp.catalog", epic)],
    );

    assert_eq!(check_shape(&graph, &vocabulary(), &[]), []);
}

#[test]
fn shape_fails_a_body_wait_on_a_signature_name() {
    // The vocabulary does not know `cat.api`; the plan record adds it with a signature.
    let plan = "New names:\n- signature cat.api: fn catalog()\n\n\
        Task: First\nNode: A\nGoal: g\nScope: s\nOut of scope: o\nInterfaces: none\n\
        Design pages: D2\nAcceptance:\n- a\nTests: t\nSecurity trigger: no\nModel: sonnet\n\
        Size: S\nProvides: cat.api\n";
    let graph = graph(
        vec![body("A"), body("B")],
        vec![order("A", "B", "cat.api")],
        vec![],
    );

    let records = [filed_as(&graph, plan.to_owned())];
    let failures = check_shape(&graph, &vocabulary(), &records);

    assert_eq!(
        failures,
        [ShapeFailure::SignatureThroughInterface {
            to: NodeId::from("B"),
            on: Waited::Node(NodeId::from("A")),
            name: Name::from("cat.api"),
        }]
    );
    assert_eq!(
        check_shape(&graph, &vocabulary(), &[]).len(),
        1,
        "with no record the name is unknown, and the wait still fails"
    );
}

#[test]
fn shape_fails_a_wait_on_a_name_with_no_known_kind() {
    let graph = graph(
        vec![body("A"), body("B")],
        vec![order("A", "B", "mystery")],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].rule(), ShapeRule::UnsignedWait);
    assert_eq!(
        failures[0].to_string(),
        "unsigned-wait: B waits on A for mystery, which is not a turn or a name with no signature"
    );
}

#[test]
fn shape_passes_turns_and_names_with_no_signature() {
    let graph = graph(
        vec![interface("I"), body("A"), body("B"), body("C")],
        vec![
            order("A", "B", "turn:app/x"),
            order("A", "C", "prompt.fix"),
            order("I", "C", "mcp.catalog"),
        ],
        vec![],
    );

    assert_eq!(check_shape(&graph, &vocabulary(), &[]), []);
}

#[test]
fn shape_fails_a_body_over_the_limit() {
    let graph = graph(
        vec![
            node("A", NodeKind::Body, 3, &[]),
            node("I", NodeKind::Interface, 5, &[]),
            node("B", NodeKind::Body, 2, &[]),
        ],
        vec![],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::BodyLimit {
            node: NodeId::from("A"),
            weight: Weight(3),
            limit: Weight(2),
        }]
    );
    assert_eq!(
        failures[0].to_string(),
        "body-limit: A weighs 3, over the limit of 2"
    );
}

#[test]
fn shape_fails_a_cycle() {
    let graph = graph(
        vec![body("A"), body("B"), body("C")],
        vec![
            order("A", "B", "turn:x"),
            order("B", "A", "turn:y"),
            order("B", "C", "turn:z"),
        ],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::Cycle {
            nodes: vec![NodeId::from("A"), NodeId::from("B")],
        }]
    );
    assert_eq!(failures[0].to_string(), "cycle: the waits of A, B go round");
}

#[test]
fn shape_fails_unordered_writers_of_one_area() {
    let graph = graph(
        vec![
            node("A", NodeKind::Body, 1, &["app/x", "app/y"]),
            node("B", NodeKind::Body, 1, &["app/y", "app/x"]),
            node("C", NodeKind::Body, 1, &["app/z"]),
        ],
        vec![],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::UnorderedWriters {
            first: NodeId::from("A"),
            second: NodeId::from("B"),
            area: Area::from("app/x"),
        }]
    );
    assert_eq!(
        failures[0].to_string(),
        "unordered-writers: A and B both write app/x and neither waits for the other"
    );
}

#[test]
fn shape_passes_writers_in_order_and_writers_that_are_closed() {
    let mut done = node("D", NodeKind::Body, 1, &["app/x"]);
    done.filed = Some(filed(101, Standing::Done));
    let mut folded = node("F", NodeKind::Body, 1, &["app/x"]);
    folded.filed = Some(filed(102, Standing::ClosedNotPlanned));
    let graph = graph(
        vec![
            node("A", NodeKind::Body, 1, &["app/x"]),
            // B waits for A through a chain, not directly.
            node("B", NodeKind::Body, 1, &["app/x"]),
            body("M"),
            done,
            folded,
        ],
        vec![order("A", "M", "turn:app/x"), order("M", "B", "turn:app/x")],
        vec![],
    );

    assert_eq!(check_shape(&graph, &vocabulary(), &[]), []);
}

#[test]
fn shape_passes_fixture_12() {
    let plan = output("12-");
    let (board, epic) = board();
    file(&board, epic, &plan);
    let graph = read(&board, epic);
    let vocabulary = Vocabulary {
        signature: names(&["mcp.catalog"]),
        existing: names(&["cli"]),
        ..Vocabulary::default()
    };

    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(check_shape(&graph, &vocabulary, &[]), []);
}

/// A fixture's output filed as tickets and read back.
fn filed_output(prefix: &str) -> PlanGraph {
    let plan = output(prefix);
    let (board, epic) = board();
    file(&board, epic, &plan);
    read(&board, epic)
}

fn catalog_vocabulary() -> Vocabulary {
    Vocabulary {
        signature: names(&["mcp.catalog"]),
        existing: names(&["cli"]),
        ..Vocabulary::default()
    }
}

/// A node no ticket holds: a task of a plan not filed yet.
fn planned(id: &str, weight: u32, writes: &[&str]) -> PlanNode {
    PlanNode {
        filed: None,
        ..node(id, NodeKind::Body, weight, writes)
    }
}

#[test]
fn shape_rerun_passes_fixture_12_and_19() {
    for prefix in ["12-", "19-"] {
        let graph = filed_output(prefix);

        assert_eq!(graph.nodes.len(), 4, "fixture {prefix}");
        assert_eq!(
            check_shape(&graph, &catalog_vocabulary(), &[]),
            [],
            "fixture {prefix}"
        );
    }
}

#[test]
fn shape_rerun_fails_a_merge_the_rerun_makes() {
    let mut graph = filed_output("12-");
    graph.nodes.push(planned("E", 1, &["app/new"]));
    graph.nodes.push(planned("F", 1, &["app/new"]));

    let failures = check_shape(&graph, &catalog_vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::RerunNodes {
            dropped: vec![NodeId::from("E"), NodeId::from("F")],
            added: vec![NodeId::from("E+F")],
        }]
    );
    assert_eq!(failures[0].rule(), ShapeRule::Rerun);
    assert_eq!(
        failures[0].nodes(),
        [&NodeId::from("E"), &NodeId::from("F"), &NodeId::from("E+F")]
    );
    assert_eq!(
        failures[0].to_string(),
        "rerun: the optimiser would replace E and F with E+F"
    );
}

#[test]
fn shape_rerun_fails_a_missing_turn_after_a_filed_writer() {
    // A is an open ticket that writes app/x; G is a task of the plan that writes it too, with no
    // turn after A. Two filed tickets are left as they were filed (D29), so only G is judged.
    let graph = graph(
        vec![
            node("A", NodeKind::Body, 1, &["app/x"]),
            planned("G", 2, &["app/x"]),
        ],
        vec![],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::RerunWait {
            change: Change::Add,
            wait: order("A", "G", "turn:app/x"),
        }]
    );
    assert_eq!(
        failures[0].to_string(),
        "rerun: the optimiser would add the wait of G on A for turn:app/x"
    );
}

#[test]
fn shape_rerun_fails_a_changed_id() {
    // Fixture 12's A+B, not filed and named the other way round: the rerun splits it and merges
    // it back as A+B.
    let mut graph = filed_output("12-");
    let merged = graph
        .nodes
        .iter_mut()
        .find(|node| node.id.as_str() == "A+B")
        .expect("fixture 12 has A+B");
    merged.id = NodeId::from("B+A");
    merged.filed = None;

    let failures = check_shape(&graph, &catalog_vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::RerunNodes {
            dropped: vec![NodeId::from("B+A")],
            added: vec![NodeId::from("A+B")],
        }]
    );
    assert_eq!(
        failures[0].to_string(),
        "rerun: the optimiser would replace B+A with A+B"
    );
}

#[test]
fn shape_rerun_fails_a_wait_another_path_implies() {
    let graph = graph(
        vec![body("A"), body("B"), body("C")],
        vec![
            order("A", "B", "turn:x"),
            order("B", "C", "turn:y"),
            order("A", "C", "turn:z"),
        ],
        vec![],
    );

    let failures = check_shape(&graph, &vocabulary(), &[]);

    assert_eq!(
        failures,
        [ShapeFailure::RerunWait {
            change: Change::Drop,
            wait: order("A", "C", "turn:z"),
        }]
    );
}

#[test]
fn shape_rerun_leaves_closed_tickets_and_their_waits_alone() {
    let mut done = body("D");
    done.filed = Some(filed(101, Standing::Done));
    let graph = graph(
        vec![done, body("A"), body("B")],
        vec![order("D", "A", "prompt.fix"), order("A", "B", "turn:x")],
        vec![],
    );

    assert_eq!(check_shape(&graph, &vocabulary(), &[]), []);
}

#[test]
fn shape_rerun_gives_each_refusal_but_a_cycle() {
    let two_providers = graph(
        vec![body("A"), body("B"), body("C")],
        vec![order("A", "C", "prompt.fix"), order("B", "C", "prompt.fix")],
        vec![],
    );
    let failures = check_shape(&two_providers, &vocabulary(), &[]);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].rule(), ShapeRule::Rerun);
    assert!(
        failures[0]
            .to_string()
            .starts_with("rerun: the optimiser refuses the graph: two-providers:"),
        "{}",
        failures[0]
    );

    let cycle = graph(
        vec![body("A"), body("B")],
        vec![order("A", "B", "turn:x"), order("B", "A", "turn:y")],
        vec![],
    );
    let failures = check_shape(&cycle, &vocabulary(), &[]);
    assert_eq!(failures.len(), 1, "the cycle is named once: {failures:?}");
    assert_eq!(failures[0].rule(), ShapeRule::Cycle);
}

#[test]
fn shape_rerun_keeps_kept_waits_of_an_unchanged_node() {
    let on = WaitTarget::Ticket {
        ticket: TicketId::from(7),
        node: Some(NodeId::from("I@1004.1")),
        interface: true,
    };
    let graph = graph(vec![body("D")], vec![], vec![kept("D", "mcp.catalog", on)]);

    assert_eq!(check_shape(&graph, &vocabulary(), &[]), []);
}

/// Fixture 12's plan filed on a fresh board, its graph read back and the record that marks it.
fn filed_epic() -> (
    FakeBoard,
    TicketId,
    Vec<(NodeId, TicketId)>,
    Vec<EpicRecord>,
) {
    let plan = output("12-");
    let (board, epic) = board();
    let filed = file(&board, epic, &plan);
    let graph = read(&board, epic);
    let records = vec![filed_as(&graph, String::new())];
    (board, epic, filed, records)
}

fn hash_vocabulary() -> Vocabulary {
    Vocabulary {
        signature: names(&["mcp.catalog"]),
        existing: names(&["cli"]),
        ..Vocabulary::default()
    }
}

#[test]
fn shape_hash_passes_as_filed() {
    let (board, epic, _, records) = filed_epic();

    let graph = read(&board, epic);

    assert_eq!(check_shape(&graph, &hash_vocabulary(), &records), []);
}

#[test]
fn shape_hash_ignores_a_state_change() {
    let (board, epic, filed, records) = filed_epic();
    board.close(filed[0].1);

    let graph = read(&board, epic);

    assert_eq!(check_shape(&graph, &hash_vocabulary(), &records), []);
}

#[test]
fn shape_hash_fails_an_edited_body() {
    let (board, epic, filed, records) = filed_epic();
    let ticket = filed[0].1;
    let body = board
        .epic(epic)
        .expect("the epic reads")
        .tasks
        .iter()
        .find(|task| task.number == ticket)
        .expect("the ticket")
        .body
        .to_string();
    let edited = body.replace("Goal: a goal", "Goal: another goal");
    assert_ne!(edited, body, "the edit lands");
    board.set_body(ticket, &edited);

    let graph = read(&board, epic);
    let failures = check_shape(&graph, &hash_vocabulary(), &records);

    let marked = records[0].mark.as_ref().expect("a mark").graph.clone();
    assert_eq!(
        failures,
        [ShapeFailure::GraphHash {
            record: 1,
            marked: marked.clone(),
            graph: graph.hash(),
        }]
    );
    assert_eq!(failures[0].rule(), ShapeRule::GraphHash);
    let line = failures[0].to_string();
    assert!(line.contains(marked.as_str()), "{line}");
    assert!(line.contains(graph.hash().as_str()), "{line}");
    assert!(failures[0].nodes().is_empty());
}

#[test]
fn shape_hash_fails_a_half_filed_plan() {
    let (board, epic, _, mut records) = filed_epic();
    records[0].mark = None;

    let graph = read(&board, epic);
    let failures = check_shape(&graph, &hash_vocabulary(), &records);

    assert_eq!(failures, [ShapeFailure::HalfFiled { record: 1 }]);
    assert_eq!(failures[0].rule(), ShapeRule::HalfFiled);
    assert_eq!(
        failures[0].to_string(),
        "half-filed: plan record 1 has no filing mark, so it is half filed: run `plan file` \
         with empty stdin to finish it"
    );
}
