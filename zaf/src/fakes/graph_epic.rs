//! Fixture 12's epic as the board holds it once planned: four planned tickets whose waits the
//! graph read gives and no `Blocked by:` line does (D19, D23).

use std::collections::BTreeSet;

use super::FakeBoard;
use crate::domain::plan::body::{PlannedTicket, Section, Signature};
use crate::domain::plan::{Name, NodeId, NodeKind};
use crate::domain::{SecurityTrigger, TaskModel, TaskSize, TaskTemplate, Ticket, TicketId};

fn names(values: &[&str]) -> BTreeSet<Name> {
    values.iter().map(|name| Name::from(*name)).collect()
}

fn planned(node: &str, kind: NodeKind, section: Section) -> PlannedTicket {
    PlannedTicket {
        node: NodeId::from(node),
        kind,
        signatures: Vec::new(),
        turns: Vec::new(),
        waits: Vec::new(),
        sections: vec![section],
    }
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

/// Files fixture 12 under `epic`, which the board already holds: A+B as `first`, C as
/// `first + 1`, D as `first + 2` and the interface ticket of `mcp.catalog` as `first + 3`. C
/// implements the name and D needs it, so each waits on the interface ticket. The tickets carry
/// no state, so each is open and shows as not started.
pub fn add_fixture_12(board: &FakeBoard, epic: TicketId, first: u64) {
    let mut interface = planned("I:mcp.catalog", NodeKind::Interface, section(&[], &[], &[]));
    interface.signatures = vec![Signature {
        name: Name::from("mcp.catalog"),
        shape: "fn catalog() -> Vec<Tool>".to_owned(),
    }];
    let bodies = [
        planned("A+B", NodeKind::Body, section(&[], &[], &[])),
        planned(
            "C",
            NodeKind::Body,
            section(&[], &["mcp.catalog"], &["cli"]),
        ),
        planned("D", NodeKind::Body, section(&[], &[], &["mcp.catalog"])),
        interface,
    ];
    for (offset, body) in (0u64..).zip(bodies) {
        let id = TicketId::from(first + offset);
        board.add_ticket(Ticket {
            id,
            epic: Some(epic),
            title: format!("Task {id}").into(),
            body: body.to_string().into(),
            state: None,
            url: "".into(),
            kind: None,
        });
    }
}
