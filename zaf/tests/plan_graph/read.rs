//! The graph read back from the tickets (I6): a fixture's output nodes are filed on the fake
//! board as the planned tickets they render as, and `PlanGraph::read` gives the nodes and waits
//! they came from.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use zaf::domain::plan::read::{KeptRead, PlanGraph, PlanNode, WaitTarget};
use zaf::domain::plan::record::{FilingMark, GraphHash, PlanRecord};
use zaf::domain::plan::{Name, Node, NodeId, NodeKind, Order, WaitOn};
use zaf::domain::{
    BodyHash, Epic, NewEpic, NewTask, SecurityTrigger, Standing, TaskModel, TaskSize, TaskTemplate,
    TicketBody, TicketId, TicketState, TicketType,
};
use zaf::fakes::FakeBoard;
use zaf::ports::Board;

use super::body::ticket_of;
use super::{ExpectedPlan, Outcome, fixtures};

/// The output of the fixture whose file starts with `prefix`.
pub(super) fn output(prefix: &str) -> ExpectedPlan {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(prefix))
        .unwrap_or_else(|| panic!("no fixture {prefix}"));
    let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
        panic!("fixture {prefix} is refused");
    };
    plan.clone()
}

pub(super) fn board() -> (FakeBoard, TicketId) {
    let board = FakeBoard::default();
    let epic = board
        .file_epic(&NewEpic {
            title: "tickets".into(),
            body: "Release: 0.6.0\n\nGoal.".into(),
        })
        .expect("the epic files");
    (board, epic)
}

/// Files `nodes` as rendered tickets under `epic`, giving each node's ticket.
pub(super) fn file(
    board: &FakeBoard,
    epic: TicketId,
    plan: &ExpectedPlan,
) -> Vec<(NodeId, TicketId)> {
    plan.nodes
        .iter()
        .map(|node: &Node| {
            let body = ticket_of(node, &plan.kept).to_string();
            let ticket = board
                .file_task(&NewTask {
                    epic,
                    title: format!("node {}", node.id).into(),
                    body: TicketBody::from(body),
                    state: TicketState::Ready,
                    group: None,
                    kind: TicketType::from("Task"),
                })
                .expect("the ticket files");
            (node.id.clone(), ticket)
        })
        .collect()
}

fn epic(board: &FakeBoard, id: TicketId) -> Epic {
    board.epic(id).expect("the epic reads")
}

pub(super) fn read(board: &FakeBoard, id: TicketId) -> PlanGraph {
    PlanGraph::read(&epic(board, id), &[])
}

fn kinds(plan: &ExpectedPlan) -> Vec<(NodeId, NodeKind)> {
    let mut nodes: Vec<_> = plan.nodes.iter().map(|n| (n.id.clone(), n.kind)).collect();
    nodes.sort();
    nodes
}

fn read_kinds(graph: &PlanGraph) -> Vec<(NodeId, NodeKind)> {
    graph.nodes.iter().map(|n| (n.id.clone(), n.kind)).collect()
}

fn expected_orders(plan: &ExpectedPlan) -> Vec<Order> {
    let mut orders = plan.orders.clone();
    orders.sort();
    orders
}

#[test]
fn read_fixture_12_output_from_fake_board() {
    let plan = output("12-");
    let (board, epic) = board();
    let filed = file(&board, epic, &plan);
    let graph = read(&board, epic);

    assert_eq!(read_kinds(&graph), kinds(&plan));
    assert_eq!(
        graph.orders.iter().cloned().collect::<Vec<_>>(),
        expected_orders(&plan)
    );
    assert!(graph.kept.is_empty());
    assert!(!graph.half_filed);
    for node in &graph.nodes {
        let ticket = filed.iter().find(|(id, _)| *id == node.id).map(|(_, t)| *t);
        let filed = node
            .filed
            .as_ref()
            .expect("a node read from a ticket is filed");
        assert_eq!(Some(filed.ticket), ticket, "{} keeps its ticket", node.id);
        assert_eq!(filed.standing, Standing::Open);
    }
}

#[test]
fn read_fixture_13_keeps_greatest_label() {
    let plan = output("13-");
    let (board, epic) = board();
    file(&board, epic, &plan);
    let graph = read(&board, epic);

    assert_eq!(read_kinds(&graph), kinds(&plan));
    assert_eq!(
        graph.orders.iter().cloned().collect::<Vec<_>>(),
        expected_orders(&plan)
    );
    // B needs a.api and b.api, both from the interface node: one wait stays, on the greater.
    let between: Vec<&Order> = graph
        .orders
        .iter()
        .filter(|o| o.from.as_str() == "I" && o.to.as_str() == "B")
        .collect();
    assert_eq!(between.len(), 1);
    assert_eq!(between[0].needs.as_str(), "b.api");
}

#[test]
fn read_keeps_closed_tickets() {
    let plan = output("13-");
    let (board, epic) = board();
    let filed = file(&board, epic, &plan);
    let ticket = |id: &str| {
        filed
            .iter()
            .find(|(node, _)| node.as_str() == id)
            .map(|(_, ticket)| *ticket)
            .expect("a filed node")
    };
    let open = read(&board, epic);
    board.close(ticket("A"));
    board.close_not_planned(ticket("C"));
    let graph = read(&board, epic);

    assert_eq!(read_kinds(&graph), kinds(&plan));
    assert_eq!(graph.orders, open.orders);
    let standing = |id: &str| -> Standing {
        let node: &PlanNode = graph
            .nodes
            .iter()
            .find(|n| n.id.as_str() == id)
            .expect("a node");
        node.filed.as_ref().expect("a filed node").standing
    };
    assert_eq!(standing("A"), Standing::Done);
    assert_eq!(standing("B"), Standing::Open);
    assert_eq!(standing("C"), Standing::ClosedNotPlanned);
}

fn record(k: u64) -> PlanRecord {
    PlanRecord {
        k,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: Vec::new(),
        plan: format!("plan {k}\n"),
    }
}

fn mark(k: u64) -> FilingMark {
    FilingMark {
        k,
        graph: "ab".repeat(32).parse::<GraphHash>().expect("a digest"),
    }
}

#[test]
fn read_half_filed_when_last_record_has_no_mark() {
    let (board, epic) = board();
    assert!(
        !read(&board, epic).half_filed,
        "no record, nothing half filed"
    );

    board
        .comment(epic, &record(1).to_string())
        .expect("comment");
    assert!(read(&board, epic).half_filed, "a record with no mark");

    board.comment(epic, &mark(1).to_string()).expect("comment");
    assert!(!read(&board, epic).half_filed, "a record with its mark");

    board
        .comment(epic, &record(2).to_string())
        .expect("comment");
    assert!(read(&board, epic).half_filed, "the last record has none");
}

#[test]
fn read_is_independent_of_listing_order() {
    let plan = output("13-");
    let (board, epic) = board();
    file(&board, epic, &plan);
    let listed = self::epic(&board, epic);
    let mut reversed = listed.clone();
    reversed.tasks.reverse();
    let mut rotated = listed.clone();
    rotated.tasks.rotate_left(1);

    let graph = PlanGraph::read(&listed, &[]);
    assert_eq!(PlanGraph::read(&reversed, &[]), graph);
    assert_eq!(PlanGraph::read(&rotated, &[]), graph);
}

/// An older ticket's body (no `Node:` line) whose `Blocked by` line names `blocked_by`.
fn older_body(blocked_by: &[TicketId]) -> TicketBody {
    let template = TaskTemplate {
        goal: "a goal".to_owned(),
        owned_paths: "a path".to_owned(),
        scoped: false,
        out_of_scope: None,
        interfaces: "none".to_owned(),
        design_pages: "none".to_owned(),
        architecture: String::new(),
        acceptance: vec!["a line".to_owned()],
        tests: "a test".to_owned(),
        security_trigger: SecurityTrigger::No,
        model: TaskModel::Sonnet,
        size: TaskSize::S,
        blocked_by: blocked_by.to_vec(),
        style: None,
    };
    TicketBody::from(template.to_string())
}

pub(super) fn file_older(board: &FakeBoard, epic: TicketId, blocked_by: &[TicketId]) -> TicketId {
    board
        .file_task(&NewTask {
            epic,
            title: "an older ticket".into(),
            body: older_body(blocked_by),
            state: TicketState::Ready,
            group: None,
            kind: TicketType::from("Task"),
        })
        .expect("the ticket files")
}

fn file_epic(board: &FakeBoard, title: &str) -> TicketId {
    board
        .file_epic(&NewEpic {
            title: title.into(),
            body: "Release: 0.6.0\n\nGoal.".into(),
        })
        .expect("the epic files")
}

fn node_id(ticket: TicketId) -> NodeId {
    NodeId::from(ticket.get().to_string().as_str())
}

fn order(from: TicketId, to: TicketId) -> Order {
    Order {
        from: node_id(from),
        to: node_id(to),
        needs: Name::from(from.to_string()),
    }
}

/// The kept wait of an older ticket `to` on the ticket `from` of another epic.
fn older_wait(from: TicketId, to: TicketId) -> KeptRead {
    KeptRead {
        to: node_id(to),
        name: Name::from(from.to_string()),
        line: WaitOn::Task(from),
        on: WaitTarget::Ticket {
            ticket: from,
            node: None,
            interface: false,
        },
    }
}

/// The kept wait of an older ticket `to` on `epic`, which has no task yet.
fn epic_wait(epic: TicketId, to: TicketId) -> KeptRead {
    KeptRead {
        to: node_id(to),
        name: Name::from(epic.to_string()),
        line: WaitOn::Epic(epic),
        on: WaitTarget::Epic(epic),
    }
}

#[test]
fn read_fixture_21_blocked_by_as_needs() {
    let plan = output("21-");
    let (board, epic) = board();
    let e3 = file_epic(&board, "E3");
    // The fixture's numbers are the tickets filed here: E3's two tasks, then 20, 21 and 22.
    let mut ticket_of: BTreeMap<&str, TicketId> = BTreeMap::new();
    ticket_of.insert("10", file_older(&board, e3, &[]));
    ticket_of.insert("11", file_older(&board, e3, &[]));
    ticket_of.insert("20", file_older(&board, epic, &[]));
    ticket_of.insert("21", file_older(&board, epic, &[ticket_of["20"]]));
    ticket_of.insert(
        "22",
        file_older(&board, epic, &[ticket_of["20"], ticket_of["21"], e3]),
    );
    let graph = PlanGraph::read(&self::epic(&board, epic), &[self::epic(&board, e3)]);

    let number = |text: &str| ticket_of[text.trim_start_matches('#')];
    let mut orders: Vec<Order> = plan
        .orders
        .iter()
        .map(|o| order(number(o.from.as_str()), number(o.to.as_str())))
        .collect();
    orders.sort();
    assert_eq!(graph.orders.iter().cloned().collect::<Vec<_>>(), orders);
    let waits: BTreeSet<KeptRead> = plan
        .kept
        .iter()
        .map(|k| older_wait(number(k.from.as_str()), number(k.to.as_str())))
        .collect();
    assert_eq!(graph.kept, waits);
    let mut ids: Vec<NodeId> = plan
        .nodes
        .iter()
        .map(|n| node_id(number(n.id.as_str())))
        .collect();
    ids.sort();
    assert_eq!(
        graph.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
        ids
    );
    assert!(graph.nodes.iter().all(|n| n.kind == NodeKind::Body));
}

#[test]
fn read_epic_blocker_needs_each_task() {
    let (board, epic) = board();
    let other = file_epic(&board, "other");
    let first = file_older(&board, other, &[]);
    let second = file_older(&board, other, &[]);
    let waiting = file_older(&board, epic, &[other]);
    let graph = PlanGraph::read(&self::epic(&board, epic), &[self::epic(&board, other)]);

    assert!(graph.orders.is_empty(), "the tasks are of another epic");
    assert_eq!(
        graph.kept,
        BTreeSet::from([older_wait(first, waiting), older_wait(second, waiting)])
    );
}

#[test]
fn read_epic_blocker_thins_waits_the_tasks_imply() {
    let (board, epic) = board();
    let first = file_older(&board, epic, &[]);
    let second = file_older(&board, epic, &[first]);
    let waiting = file_older(&board, epic, &[epic]);
    let graph = PlanGraph::read(&self::epic(&board, epic), &[self::epic(&board, epic)]);

    // The epic names its own tasks: `waiting` needs `first` and `second`, and `first` is implied.
    assert_eq!(
        graph.orders,
        BTreeSet::from([order(first, second), order(second, waiting)])
    );
    assert!(graph.kept.is_empty());
}

#[test]
fn read_epic_blocker_ignores_tasks_closed_as_not_planned() {
    let (board, epic) = board();
    let other = file_epic(&board, "other");
    let done = file_older(&board, other, &[]);
    let dropped = file_older(&board, other, &[]);
    board.close(done);
    board.close_not_planned(dropped);
    let waiting = file_older(&board, epic, &[other]);
    let graph = PlanGraph::read(&self::epic(&board, epic), &[self::epic(&board, other)]);

    assert_eq!(graph.kept, BTreeSet::from([older_wait(done, waiting)]));
}

#[test]
fn read_epic_blocker_without_tasks_waits_on_epic() {
    let (board, epic) = board();
    let empty = file_epic(&board, "empty");
    let waiting = file_older(&board, epic, &[empty]);
    let graph = PlanGraph::read(&self::epic(&board, epic), &[self::epic(&board, empty)]);

    assert!(graph.orders.is_empty());
    assert_eq!(graph.kept, BTreeSet::from([epic_wait(empty, waiting)]));
}

#[test]
fn read_older_and_planned_tickets_share_an_epic() {
    let plan = output("13-");
    let (board, epic) = board();
    file(&board, epic, &plan);
    let planned_only = read(&board, epic);
    let older = file_older(&board, epic, &[]);
    let after = file_older(&board, epic, &[older]);
    let graph = read(&board, epic);

    assert_eq!(graph.nodes.len(), planned_only.nodes.len() + 2);
    assert!(planned_only.orders.is_subset(&graph.orders));
    assert!(graph.orders.contains(&order(older, after)));
}

// Kept waits (D13, D18): a `Waits on:` line, as written and as resolved.

/// The epic the fixture-22 plan waits on.
const PROVIDER_EPIC: u64 = 1004;

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// A board with epic #1004 on it, then the planned epic, which files the next number.
fn board_with_provider() -> (FakeBoard, TicketId) {
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(PROVIDER_EPIC), "provider");
    let epic = board
        .file_epic(&NewEpic {
            title: "tickets".into(),
            body: "Release: 0.6.0\n\nGoal.".into(),
        })
        .expect("the epic files");
    (board, epic)
}

/// Files epic #1004's interface ticket `I@1004.1`, which holds `mcp.catalog`.
fn file_interface(board: &FakeBoard) -> TicketId {
    let node: Node = serde_json::from_value(serde_json::json!({
        "id": "I@1004.1",
        "kind": "interface",
        "behaviours": [{
            "text": "the signature of mcp.catalog",
            "size": 1,
            "writes": ["sig:mcp.catalog"],
            "provides": ["mcp.catalog"],
            "needs": []
        }]
    }))
    .expect("a node");
    board
        .file_task(&NewTask {
            epic: TicketId::from(PROVIDER_EPIC),
            title: "interface".into(),
            body: TicketBody::from(ticket_of(&node, &[]).to_string()),
            state: TicketState::Ready,
            group: None,
            kind: TicketType::from("Task"),
        })
        .expect("the ticket files")
}

fn release(board: &FakeBoard) -> Vec<Epic> {
    board.epics().expect("the epics read")
}

fn catalog_waits(graph: &PlanGraph) -> Vec<&KeptRead> {
    graph
        .kept
        .iter()
        .filter(|wait| wait.name.as_str() == "mcp.catalog")
        .collect()
}

#[test]
fn read_kept_wait_resolves_to_interface_ticket() {
    let plan = output("22-");
    let (board, epic) = board_with_provider();
    file(&board, epic, &plan);
    let interface = file_interface(&board);
    let graph = PlanGraph::read(&self::epic(&board, epic), &release(&board));

    assert_eq!(
        graph.kept.iter().cloned().collect::<Vec<_>>(),
        vec![KeptRead {
            to: NodeId::from("D"),
            name: "mcp.catalog".into(),
            line: WaitOn::Epic(TicketId::from(PROVIDER_EPIC)),
            on: WaitTarget::Ticket {
                ticket: interface,
                node: Some(NodeId::from("I@1004.1")),
                interface: true,
            },
        }]
    );
}

#[test]
fn read_kept_wait_on_unplanned_epic_stays_on_epic() {
    let plan = output("22-");
    let (board, epic) = board_with_provider();
    file(&board, epic, &plan);
    let graph = PlanGraph::read(&self::epic(&board, epic), &release(&board));

    let waits = catalog_waits(&graph);
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].on, WaitTarget::Epic(TicketId::from(PROVIDER_EPIC)));
    assert_eq!(waits[0].line, WaitOn::Epic(TicketId::from(PROVIDER_EPIC)));
}

/// The plan of fixture 22 with D waiting on ticket `provider` by number, and its plan record
/// posted at 100.
fn wait_on_ticket(board: &FakeBoard, epic: TicketId, provider: TicketId) -> Epic {
    let mut plan = output("22-");
    for wait in &mut plan.kept {
        wait.from = NodeId::from(provider.get().to_string().as_str());
    }
    file(board, epic, &plan);
    board
        .comment(epic, &record(1).to_string())
        .expect("comment");
    let mut read = self::epic(board, epic);
    read.records[0].posted_at = Some(at(100));
    read
}

#[test]
fn read_drops_wait_done_before_record() {
    let (board, epic) = board_with_provider();
    let provider = file_interface(&board);
    board.close_at(provider, at(50));
    let waiting = wait_on_ticket(&board, epic, provider);
    let graph = PlanGraph::read(&waiting, &release(&board));

    assert!(graph.kept.is_empty(), "the name was code by then");
}

#[test]
fn read_keeps_wait_done_after_record() {
    let (board, epic) = board_with_provider();
    let provider = file_interface(&board);
    board.close_at(provider, at(150));
    let waiting = wait_on_ticket(&board, epic, provider);
    let graph = PlanGraph::read(&waiting, &release(&board));

    let waits = catalog_waits(&graph);
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].line, WaitOn::Task(provider));
    assert_eq!(
        waits[0].on,
        WaitTarget::Ticket {
            ticket: provider,
            node: Some(NodeId::from("I@1004.1")),
            interface: true,
        }
    );
}

#[test]
fn read_keeps_wait_on_not_planned_provider() {
    let (board, epic) = board_with_provider();
    let provider = file_interface(&board);
    board.close_at(provider, at(50));
    board.close_not_planned(provider);
    let waiting = wait_on_ticket(&board, epic, provider);
    let graph = PlanGraph::read(&waiting, &release(&board));

    assert_eq!(catalog_waits(&graph).len(), 1, "a dead wait stays a wait");
}

// The hash (I6): over the tickets' bodies, the waits and the written lines, and over no state.

#[test]
fn hash_is_stable_across_reads() {
    let plan = output("12-");
    let (board, epic) = board();
    file(&board, epic, &plan);
    let first = read(&board, epic).hash();

    assert_eq!(read(&board, epic).hash(), first);
    let listed = self::epic(&board, epic);
    let mut reversed = listed.clone();
    reversed.tasks.reverse();
    assert_eq!(PlanGraph::read(&reversed, &[]).hash(), first);
}

#[test]
fn hash_moves_on_a_body_edit() {
    let plan = output("12-");
    let (board, epic) = board();
    let filed = file(&board, epic, &plan);
    let before = read(&board, epic);
    let ticket = filed[0].1;
    let body = self::epic(&board, epic)
        .tasks
        .iter()
        .find(|task| task.number == ticket)
        .expect("the ticket")
        .body
        .to_string();
    let edited = body.replace("Goal: a goal", "Goal: another goal");
    assert_ne!(edited, body, "the edit lands");
    board.set_body(ticket, &edited);
    let after = read(&board, epic);

    assert_eq!(read_kinds(&after), read_kinds(&before), "it still reads");
    assert_ne!(after.hash(), before.hash());
}

#[test]
fn hash_ignores_a_closed_ticket() {
    let plan = output("13-");
    let (board, epic) = board();
    let filed = file(&board, epic, &plan);
    let before = read(&board, epic).hash();
    board.close(filed[0].1);
    assert_eq!(read(&board, epic).hash(), before, "closed as completed");
    board.close_not_planned(filed[1].1);
    assert_eq!(read(&board, epic).hash(), before, "closed as not planned");
}

#[test]
fn hash_ignores_a_closed_ticket_a_kept_wait_names() {
    let (board, epic) = board_with_provider();
    let provider = file_interface(&board);
    let waiting = wait_on_ticket(&board, epic, provider);
    let before = PlanGraph::read(&waiting, &release(&board));
    assert_eq!(catalog_waits(&before).len(), 1);
    board.close_at(provider, at(50));
    let after = PlanGraph::read(&waiting, &release(&board));

    assert!(after.kept.is_empty(), "the wait is dropped as code by then");
    assert_eq!(after.hash(), before.hash());
}

#[test]
fn hash_ignores_another_epics_planning() {
    let plan = output("22-");
    let (board, epic) = board_with_provider();
    file(&board, epic, &plan);
    let before = PlanGraph::read(&self::epic(&board, epic), &release(&board));
    file_interface(&board);
    let after = PlanGraph::read(&self::epic(&board, epic), &release(&board));

    assert_ne!(after.kept, before.kept, "the wait now resolves to a ticket");
    assert_eq!(after.hash(), before.hash());
}
