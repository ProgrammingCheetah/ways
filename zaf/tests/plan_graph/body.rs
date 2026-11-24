//! The planned ticket body (I2): every fixture's output nodes render as planned tickets and parse
//! back to the same nodes, ids, names, turns and kept waits; a merged ticket has a section a
//! behaviour; a malformed body is refused naming the line or the field.

use std::collections::{BTreeMap, BTreeSet};

use zaf::domain::plan::{
    Behaviour, KeptWait, Name, Node, NodeId, NodeKey, NodeKind, PlannedTicket, Section, Signature,
    Size, Wait, WaitOn, Weight,
};
use zaf::domain::{
    SecurityTrigger, TaskModel, TaskSize, TaskTemplate, TaskTemplateError, TicketId,
};

use super::{Outcome, fixtures};

/// A section of neutral prose around the names and the size a behaviour has.
fn section(key: Option<&str>, behaviour: &Behaviour) -> Section {
    let size = match behaviour.size.weight() {
        Some(Weight(2)) => TaskSize::M,
        _ => TaskSize::S,
    };
    Section {
        key: key.map(NodeKey::from),
        template: TaskTemplate {
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
            size,
            blocked_by: Vec::new(),
            style: None,
        },
        writes: behaviour.writes.clone(),
        provides: behaviour.provides.clone(),
        implements: BTreeSet::new(),
        needs: behaviour.needs.clone(),
    }
}

/// The keys of a node's sections: a merged id's parts, else `a`, `b`, and so on.
fn keys(node: &Node) -> Vec<Option<String>> {
    let count = node.behaviours.len();
    if count == 1 {
        return vec![None];
    }
    let parts: Vec<&str> = node.id.as_str().split('+').collect();
    if parts.len() == count {
        return parts.iter().map(|part| Some((*part).to_owned())).collect();
    }
    ('a'..='z')
        .take(count)
        .map(|letter| Some(letter.to_string()))
        .collect()
}

/// The wait a kept wait writes: `#<n>` is a task, an interface of another epic is the epic.
fn wait_of(kept: &KeptWait) -> Wait {
    let from = kept.from.as_str();
    let on = match from.strip_prefix("I@") {
        Some(rest) => {
            let epic = rest.split('.').next().expect("an epic number");
            WaitOn::Epic(TicketId::from(epic.parse::<u64>().expect("a number")))
        }
        None => WaitOn::Task(from.parse::<TicketId>().expect("a task number")),
    };
    Wait {
        on,
        name: kept.needs.clone(),
    }
}

/// The node a planned ticket stands for, with its waits.
pub fn ticket_of(node: &Node, kept: &[KeptWait]) -> PlannedTicket {
    let signatures = if node.kind == NodeKind::Interface {
        node.behaviours
            .iter()
            .flat_map(|b| b.provides.iter())
            .map(|name| Signature {
                name: name.clone(),
                shape: "fn shape()".to_owned(),
            })
            .collect()
    } else {
        Vec::new()
    };
    PlannedTicket {
        node: node.id.clone(),
        kind: node.kind,
        signatures,
        turns: node.turns.clone(),
        waits: kept
            .iter()
            .filter(|k| k.to == node.id)
            .map(wait_of)
            .collect(),
        sections: node
            .behaviours
            .iter()
            .zip(keys(node))
            .map(|(behaviour, key)| section(key.as_deref(), behaviour))
            .collect(),
    }
}

/// What a planned ticket says of its node: id, kind, turns and each behaviour's names and weight.
type Said = (
    NodeId,
    NodeKind,
    Vec<(NodeId, String)>,
    Vec<(BTreeSet<String>, BTreeSet<String>, BTreeSet<String>, u32)>,
);

fn strings<T: ToString>(items: &BTreeSet<T>) -> BTreeSet<String> {
    items.iter().map(ToString::to_string).collect()
}

fn said_by_node(node: &Node) -> Said {
    let turns = node
        .turns
        .iter()
        .map(|t| (t.0.clone(), t.1.to_string()))
        .collect();
    let behaviours = node
        .behaviours
        .iter()
        .map(|b| {
            let weight = b.size.weight().map_or(1, |w| w.0);
            (
                strings(&b.writes),
                strings(&b.provides),
                strings(&b.needs),
                weight.min(2),
            )
        })
        .collect();
    (node.id.clone(), node.kind, turns, behaviours)
}

fn said_by_ticket(ticket: &PlannedTicket) -> Said {
    let turns = ticket
        .turns
        .iter()
        .map(|t| (t.0.clone(), t.1.to_string()))
        .collect();
    let behaviours = ticket
        .sections
        .iter()
        .map(|s| {
            let weight = if s.template.size == TaskSize::M { 2 } else { 1 };
            (
                strings(&s.writes),
                strings(&s.provides),
                strings(&s.needs),
                weight,
            )
        })
        .collect();
    (ticket.node.clone(), ticket.kind, turns, behaviours)
}

#[test]
fn planned_body_round_trips_every_fixture_output() {
    let mut nodes_seen = 0;
    for (file, fixture) in fixtures() {
        let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
            continue;
        };
        let mut kept_back: BTreeMap<NodeId, Vec<Wait>> = BTreeMap::new();
        for node in &plan.nodes {
            let ticket = ticket_of(node, &plan.kept);
            let text = ticket.to_string();
            let parsed: PlannedTicket = text
                .parse()
                .unwrap_or_else(|e| panic!("{file}: {} does not parse: {e}\n{text}", node.id));
            assert_eq!(parsed, ticket, "{file}: {} round-trips\n{text}", node.id);
            assert_eq!(said_by_ticket(&parsed), said_by_node(node), "{file}");
            assert_eq!(
                parsed.to_string(),
                text,
                "{file}: {} renders again",
                node.id
            );
            kept_back.insert(node.id.clone(), parsed.waits);
            nodes_seen += 1;
        }
        let mut written: Vec<(NodeId, Name)> = plan
            .kept
            .iter()
            .map(|k| (k.to.clone(), k.needs.clone()))
            .collect();
        let mut read: Vec<(NodeId, Name)> = kept_back
            .iter()
            .flat_map(|(to, waits)| waits.iter().map(|w| (to.clone(), w.name.clone())))
            .collect();
        written.sort();
        read.sort();
        assert_eq!(read, written, "{file}: the kept waits come back");
    }
    assert!(nodes_seen > 50, "the fixtures give many nodes");
}

fn behaviour(provides: &str) -> Behaviour {
    Behaviour {
        text: "a sentence".to_owned(),
        size: Size::default(),
        writes: BTreeSet::from([zaf::domain::plan::Area::from("hub")]),
        provides: BTreeSet::from([Name::from(provides)]),
        needs: BTreeSet::new(),
    }
}

fn node(id: &str, behaviours: Vec<Behaviour>) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours,
    }
}

#[test]
fn planned_body_merged_ticket_has_a_section_per_behaviour() {
    let merged = node("A+B", vec![behaviour("a.one"), behaviour("b.one")]);
    let text = ticket_of(&merged, &[]).to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Node: A+B");
    let opened: Vec<&&str> = lines
        .iter()
        .filter(|l| l.starts_with("Behaviour:"))
        .collect();
    assert_eq!(opened, [&"Behaviour: A", &"Behaviour: B"]);
    assert_eq!(lines.iter().filter(|l| l.starts_with("Goal:")).count(), 2);
    let parsed: PlannedTicket = text.parse().expect("a merged ticket parses");
    assert_eq!(parsed.sections.len(), 2);
    assert_eq!(parsed.sections[1].key, Some(NodeKey::from("B")));

    let single = node("C", vec![behaviour("c.one")]);
    let text = ticket_of(&single, &[]).to_string();
    assert!(
        !text.contains("Behaviour:"),
        "a single behaviour has no line:\n{text}"
    );
    let parsed: PlannedTicket = text.parse().expect("a one-behaviour ticket parses");
    assert_eq!(parsed.sections[0].key, None);
}

fn one_ticket_text() -> String {
    ticket_of(&node("A", vec![behaviour("a.one")]), &[]).to_string()
}

#[test]
fn planned_body_refuses_a_bad_node_id() {
    let text = one_ticket_text().replace("Node: A", "Node: A#1");
    let error = text.parse::<PlannedTicket>().expect_err("a # is refused");
    assert!(
        matches!(&error, TaskTemplateError::BadPlannedLine { line: 1, label, .. } if label == "Node"),
        "{error:?}"
    );
    assert!(error.to_string().contains("Node"), "{error}");
}

#[test]
fn planned_body_refuses_a_missing_size() {
    let text = one_ticket_text().replace("Size: S\n", "");
    let error = text
        .parse::<PlannedTicket>()
        .expect_err("no Size is refused");
    assert_eq!(
        error,
        TaskTemplateError::MissingField(zaf::domain::TaskField::Size)
    );
    assert!(error.to_string().contains("Size"), "{error}");
}

#[test]
fn planned_body_refuses_blocked_by() {
    let text = format!("{}Blocked by: #12\n", one_ticket_text());
    let error = text
        .parse::<PlannedTicket>()
        .expect_err("Blocked by is refused");
    assert!(matches!(
        error,
        TaskTemplateError::BlockedByInPlannedBody { .. }
    ));
    assert!(error.to_string().contains("Blocked by"), "{error}");
}

#[test]
fn planned_body_refuses_two_sections_without_a_behaviour_line() {
    let one = one_ticket_text();
    let section: String = one
        .split_once("\nGoal:")
        .map(|(_, rest)| format!("Goal:{rest}"))
        .expect("a section");
    let error = format!("{one}{section}")
        .parse::<PlannedTicket>()
        .expect_err("two sections without keys");
    assert!(
        matches!(error, TaskTemplateError::NoBehaviourLine { .. }),
        "{error:?}"
    );
}

#[test]
fn planned_body_with_no_node_line_is_not_planned() {
    let text = one_ticket_text().replace("Node: A\n", "");
    assert!(!PlannedTicket::is_planned(&text));
    assert_eq!(
        text.parse::<PlannedTicket>().expect_err("not planned"),
        TaskTemplateError::NotPlanned
    );
    assert!(PlannedTicket::is_planned(&one_ticket_text()));
}
