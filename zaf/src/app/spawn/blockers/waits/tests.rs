use std::collections::BTreeSet;

use super::*;
use crate::domain::plan::body::{PlannedTicket, Section, Signature, WaitOn};
use crate::domain::plan::{Name, NodeId, NodeKind};
use crate::domain::{SecurityTrigger, TaskModel, TaskSize, TaskTemplate, Ticket};
use crate::fakes::FakeBoard;

fn number(value: u64) -> TicketId {
    TicketId::from(value)
}

fn names(values: &[&str]) -> BTreeSet<Name> {
    values.iter().map(|name| Name::from(*name)).collect()
}

fn section(provides: &[&str], implements: &[&str], needs: &[&str]) -> Section {
    Section {
        key: None,
        template: TaskTemplate {
            goal: "a goal".to_owned(),
            owned_paths: "a path".to_owned(),
            scoped: false,
            out_of_scope: None,
            interfaces: "none".to_owned(),
            design_pages: "D1".to_owned(),
            architecture: String::new(),
            acceptance: vec!["a line".to_owned()],
            tests: "a test".to_owned(),
            security_trigger: SecurityTrigger::No,
            model: TaskModel::Sonnet,
            size: TaskSize::S,
            blocked_by: Vec::new(),
            style: None,
        },
        writes: BTreeSet::new(),
        provides: names(provides),
        implements: names(implements),
        needs: names(needs),
    }
}

/// A planned body of one section.
fn planned(node: &str, section: Section) -> PlannedTicket {
    PlannedTicket {
        node: NodeId::from(node),
        kind: NodeKind::Body,
        signatures: Vec::new(),
        turns: Vec::new(),
        waits: Vec::new(),
        sections: vec![section],
    }
}

fn file(board: &FakeBoard, epic: u64, ticket: u64, body: &str) {
    board.add_ticket(Ticket {
        id: number(ticket),
        epic: Some(number(epic)),
        title: format!("Task {ticket}").into(),
        body: body.into(),
        state: None,
        url: "".into(),
        kind: None,
    });
}

/// The older template, as a ticket filed before 0.6.0 reads.
fn older(blocked_by: &str) -> String {
    format!(
        "Goal: a goal\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - a line\nTests: a test\nSecurity trigger: no\nModel: sonnet\n\
         Size: S\nBlocked by: {blocked_by}\n"
    )
}

#[test]
fn waits_from_graph_fixture_12() {
    // Fixture 12 after the optimiser: A+B, C and D, and the interface ticket of `mcp.catalog`,
    // which C implements and D needs.
    let board = FakeBoard::default();
    board.add_epic(number(100), "update and the MCP");
    let mut interface = planned("I:mcp.catalog", section(&[], &[], &[]));
    interface.kind = NodeKind::Interface;
    interface.signatures = vec![Signature {
        name: Name::from("mcp.catalog"),
        shape: "fn catalog() -> Vec<Tool>".to_owned(),
    }];
    file(
        &board,
        100,
        101,
        &planned("A+B", section(&[], &[], &[])).to_string(),
    );
    file(
        &board,
        100,
        102,
        &planned("C", section(&[], &["mcp.catalog"], &["cli"])).to_string(),
    );
    file(
        &board,
        100,
        103,
        &planned("D", section(&[], &[], &["mcp.catalog"])).to_string(),
    );
    file(&board, 100, 104, &interface.to_string());

    let waits = Waits::read(&board, &[number(100)]).expect("the board answers");

    assert_eq!(waits.of(number(102)), [Wait::Ticket(number(104))]);
    assert_eq!(waits.of(number(103)), [Wait::Ticket(number(104))]);
    assert_eq!(waits.of(number(104)), []);
    assert_eq!(waits.of(number(101)), []);
    assert_eq!(
        waits.graph(),
        Graph::from([
            (number(101), vec![]),
            (number(102), vec![number(104)]),
            (number(103), vec![number(104)]),
            (number(104), vec![]),
        ])
    );
}

#[test]
fn waits_from_graph_older_ticket() {
    let board = FakeBoard::default();
    board.add_epic(number(100), "an older epic");
    file(&board, 100, 101, &older("#7, #9"));

    let waits = Waits::read(&board, &[number(100)]).expect("the board answers");

    assert_eq!(
        waits.of(number(101)),
        [Wait::Ticket(number(7)), Wait::Ticket(number(9))]
    );
    assert_eq!(
        waits.graph(),
        Graph::from([(number(101), vec![number(7), number(9)])])
    );
}

#[test]
fn waits_from_graph_unplanned_epic() {
    let board = FakeBoard::default();
    board.add_epic(number(100), "the waiter");
    board.add_epic(number(200), "the provider");
    let mut waiter = planned("W", section(&[], &[], &[]));
    waiter.waits = vec![crate::domain::plan::Wait {
        name: Name::from("store.open"),
        on: WaitOn::Epic(number(200)),
    }];
    file(&board, 100, 101, &waiter.to_string());
    let release = [number(100), number(200)];

    let waits = Waits::read(&board, &release).expect("the board answers");
    let on_epic = Wait::Epic {
        epic: number(200),
        name: Name::from("store.open"),
    };
    assert_eq!(waits.of(number(101)), [on_epic]);
    assert_eq!(waits.graph(), Graph::from([(number(101), vec![])]));

    // The providing epic files its ticket with the name.
    file(
        &board,
        200,
        201,
        &planned("P", section(&["store.open"], &[], &[])).to_string(),
    );
    let waits = Waits::read(&board, &release).expect("the board answers");
    assert_eq!(waits.of(number(101)), [Wait::Ticket(number(201))]);
    assert_eq!(waits.of(number(201)), []);
}

/// D29: X gave `m` and was dropped; the re-plan's X2 gives `m` under a new key. Y, which needs
/// `m`, waits on X2 alone: the dropped ticket is no provider while a live one gives the name.
#[test]
fn waits_from_graph_a_dropped_provider_replaced_under_a_new_key() {
    let board = FakeBoard::default();
    board.add_epic(number(100), "a re-planned epic");
    file(
        &board,
        100,
        101,
        &planned("X", section(&["m"], &[], &[])).to_string(),
    );
    file(
        &board,
        100,
        102,
        &planned("Y", section(&[], &[], &["m"])).to_string(),
    );
    board.close_not_planned(number(101));

    // Nobody gives `m` again yet: the dropped ticket is the dead wait a question is raised on.
    let waits = Waits::read(&board, &[number(100)]).expect("the board answers");
    assert_eq!(waits.of(number(102)), [Wait::Ticket(number(101))]);

    file(
        &board,
        100,
        103,
        &planned("X2", section(&["m"], &[], &[])).to_string(),
    );
    let waits = Waits::read(&board, &[number(100)]).expect("the board answers");
    assert_eq!(waits.of(number(102)), [Wait::Ticket(number(103))]);

    // The replacement done, the name is still its: the dropped ticket does not come back.
    board.close(number(103));
    let waits = Waits::read(&board, &[number(100)]).expect("the board answers");
    assert_eq!(waits.of(number(102)), [Wait::Ticket(number(103))]);
}
