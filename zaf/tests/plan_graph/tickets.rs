//! The node-to-ticket step (I2): each node of an optimised plan becomes a ticket to file. A merged
//! node has a section a behaviour, the interface ticket holds the shapes, a kept wait is written
//! as the plan meant it, a turn order is a `Turn:` line, and every ticket parses back.

use std::collections::{BTreeMap, BTreeSet};

use zaf::app::plan::{TicketError, TicketToFile, tickets};
use zaf::domain::plan::optimise::optimise;
use zaf::domain::plan::text::{ParsedPlan, PlanSection};
use zaf::domain::plan::{
    Area, Name, Node, NodeId, NodeKind, Optimised, PlanInput, PlannedTicket, Signature, Turn, Wait,
    WaitOn, Weight,
};
use zaf::domain::{SecurityTrigger, TaskField, TaskModel, TaskSize, TicketId};

use super::{Fixture, Outcome, fixtures};

/// The fixture whose file name starts with `number-`.
fn fixture(number: &str) -> Fixture {
    fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(&format!("{number}-")))
        .unwrap_or_else(|| panic!("fixture {number} is in the folder"))
        .1
}

/// The prose fields of a section about `text`.
fn fields(text: &str, weight: Option<Weight>) -> BTreeMap<TaskField, String> {
    let size = if weight == Some(Weight(2)) { "M" } else { "S" };
    [
        (TaskField::Goal, text.to_owned()),
        (TaskField::OwnedPaths, "a path".to_owned()),
        (TaskField::Interfaces, "none".to_owned()),
        (TaskField::DesignPages, "D3".to_owned()),
        (TaskField::Tests, "a_test".to_owned()),
        (TaskField::SecurityTrigger, "no".to_owned()),
        (TaskField::Model, "sonnet".to_owned()),
        (TaskField::Size, size.to_owned()),
    ]
    .into_iter()
    .collect()
}

/// A section as a planner would have written it for `id`, titled `Do <id>`.
fn section(id: &str, text: &str, weight: Option<Weight>) -> PlanSection {
    PlanSection {
        node: NodeId::from(id),
        title: format!("Do {id}"),
        group: Some(format!("group-{id}")),
        kind: Some(format!("Type-{id}")),
        fields: fields(text, weight),
        acceptance: vec!["it works".to_owned()],
    }
}

/// The plan as written for a fixture's nodes: a section for each node, and for each part of a
/// merged id (a planner cannot write a merged node, only its parts).
fn written(plan: &PlanInput) -> ParsedPlan {
    let mut sections = Vec::new();
    for node in &plan.nodes {
        let parts: Vec<&str> = node.id.as_str().split('+').collect();
        if parts.len() == node.behaviours.len() && parts.len() > 1 {
            for (part, behaviour) in parts.iter().zip(&node.behaviours) {
                sections.push(section(part, &behaviour.text, behaviour.size.weight()));
            }
        } else if let Some(behaviour) = node.behaviours.first() {
            sections.push(section(
                node.id.as_str(),
                &behaviour.text,
                behaviour.size.weight(),
            ));
        }
    }
    ParsedPlan {
        input: plan.clone(),
        sections,
        signatures: BTreeMap::new(),
        drop: Vec::new(),
    }
}

/// A shape for every name a plan lifts.
fn shapes_of(optimised: &Optimised) -> BTreeMap<Name, String> {
    optimised
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Interface)
        .flat_map(|node| &node.behaviours)
        .flat_map(|behaviour| &behaviour.provides)
        .map(|name| (name.clone(), format!("fn {name}()")))
        .collect()
}

/// The tickets of a fixture's plan, which must make them.
fn made(number: &str) -> Vec<TicketToFile> {
    let fixture = fixture(number);
    let optimised = optimise(&fixture.plan, fixture.options)
        .unwrap_or_else(|refusals| panic!("fixture {number} is refused: {refusals:?}"));
    tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised))
        .unwrap_or_else(|errors| panic!("fixture {number} makes no tickets: {errors:?}"))
}

fn ticket<'a>(all: &'a [TicketToFile], id: &str) -> &'a TicketToFile {
    all.iter()
        .find(|ticket| ticket.node.as_str() == id)
        .unwrap_or_else(|| panic!("a ticket for {id}"))
}

#[test]
fn tickets_merged_node_has_a_section_per_behaviour() {
    let all = made("12");
    let merged = ticket(&all, "A+B");
    let goals: Vec<&str> = merged
        .body
        .sections
        .iter()
        .map(|section| section.template.goal.as_str())
        .collect();
    assert_eq!(
        goals,
        ["update takes --yes", "update prints one summary line"]
    );
    let keys: Vec<Option<&str>> = merged
        .body
        .sections
        .iter()
        .map(|section| section.key.as_ref().map(|key| key.as_str()))
        .collect();
    assert_eq!(keys, [Some("A"), Some("B")]);
    // The ruling on #1213: the titles joined in id order, the group and type of the lowest id.
    assert_eq!(merged.title, "Do A + Do B");
    assert_eq!(merged.group.as_deref(), Some("group-A"));
    assert_eq!(merged.kind.as_deref(), Some("Type-A"));
    assert_eq!(merged.size, TaskSize::M);
    // A ticket of one behaviour has no key and keeps its section's title, group and type.
    let single = ticket(&all, "D");
    assert_eq!(single.body.sections.len(), 1);
    assert_eq!(single.body.sections[0].key, None);
    assert_eq!(single.title, "Do D");
    assert_eq!(single.group.as_deref(), Some("group-D"));
    assert_eq!(single.kind.as_deref(), Some("Type-D"));
    assert_eq!(single.model, TaskModel::Sonnet);
}

#[test]
fn tickets_interface_holds_signatures() {
    let fixture = fixture("12");
    let optimised = optimise(&fixture.plan, fixture.options).expect("fixture 12 optimises");
    let mut parsed = written(&fixture.plan);
    // C says so for its security trigger, and design pages come from the tickets that use a name.
    for section in &mut parsed.sections {
        if section.node.as_str() == "C" {
            section
                .fields
                .insert(TaskField::SecurityTrigger, "yes the catalog".to_owned());
        }
    }
    let shapes = BTreeMap::from([(
        Name::from("mcp.catalog"),
        "fn catalog() -> Vec<Verb>".to_owned(),
    )]);
    let all = tickets(&parsed, &optimised, &shapes).expect("tickets");
    let interface = ticket(&all, "I:mcp.catalog");
    assert_eq!(interface.body.kind, NodeKind::Interface);
    assert_eq!(
        interface.body.signatures,
        [Signature {
            name: Name::from("mcp.catalog"),
            shape: "fn catalog() -> Vec<Verb>".to_owned(),
        }]
    );
    assert_eq!(interface.title, "The signatures of mcp.catalog");
    assert_eq!(interface.size, TaskSize::S);
    assert_eq!(interface.model, TaskModel::Sonnet);
    let section = &interface.body.sections[0];
    assert_eq!(
        section.template.acceptance,
        ["the gate passes with every stand-in"]
    );
    assert_eq!(
        section.template.security_trigger,
        SecurityTrigger::Yes("the catalog".to_owned())
    );
    assert_eq!(section.template.design_pages, "D3");
    assert_eq!(
        section.provides,
        BTreeSet::from([Name::from("mcp.catalog")])
    );
    // C provided the name before the lift: it implements the stand-in now, and D needs it.
    let c = &ticket(&all, "C").body.sections[0];
    assert_eq!(c.implements, BTreeSet::from([Name::from("mcp.catalog")]));
    assert!(c.provides.is_empty());
    assert!(!c.needs.contains(&Name::from("mcp.catalog")));
    let d = &ticket(&all, "D").body.sections[0];
    assert!(d.implements.is_empty());
    assert_eq!(d.needs, BTreeSet::from([Name::from("mcp.catalog")]));
    // A shape nobody gave is an error, never a guess.
    let errors = tickets(&parsed, &optimised, &BTreeMap::new()).expect_err("no shape");
    assert!(
        matches!(errors.as_slice(), [TicketError::NoShape { name, .. }] if name.as_str() == "mcp.catalog")
    );
    // The plan's own new names give a shape too.
    parsed
        .signatures
        .insert(Name::from("mcp.catalog"), "fn own()".to_owned());
    let all = tickets(&parsed, &optimised, &BTreeMap::new()).expect("tickets");
    assert_eq!(
        ticket(&all, "I:mcp.catalog").body.signatures[0].shape,
        "fn own()"
    );
}

#[test]
fn tickets_waits_on_as_written() {
    let all = made("22");
    let d = ticket(&all, "D");
    assert_eq!(
        d.body.waits,
        [Wait {
            on: WaitOn::Epic(TicketId::from(1004)),
            name: Name::from("mcp.catalog"),
        }]
    );
    assert!(
        d.body
            .to_string()
            .contains("Waits on: epic #1004 for mcp.catalog\n")
    );
    assert!(!d.body.to_string().contains("I@1004.1"));
    // A wait by number stays a wait by number.
    let fixture = fixture("22");
    let mut optimised = optimise(&fixture.plan, fixture.options).expect("optimises");
    optimised.kept = optimised
        .kept
        .iter()
        .cloned()
        .map(|mut kept| {
            kept.from = NodeId::from("#41");
            kept
        })
        .collect();
    let all =
        tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised)).expect("tickets");
    assert_eq!(
        ticket(&all, "D").body.waits,
        [Wait {
            on: WaitOn::Task(TicketId::from(41)),
            name: Name::from("mcp.catalog"),
        }]
    );
    // A provider that names no epic and no task cannot be written.
    optimised.kept = optimised
        .kept
        .iter()
        .cloned()
        .map(|mut kept| {
            kept.from = NodeId::from("T9");
            kept
        })
        .collect();
    let errors = tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised))
        .expect_err("a wait with no target");
    assert!(matches!(errors.as_slice(), [TicketError::BadWait { .. }]));
}

#[test]
fn tickets_turn_lines_from_orders() {
    let fixture = fixture("03");
    let optimised = optimise(&fixture.plan, fixture.options).expect("fixture 03 optimises");
    let turns: Vec<_> = optimised
        .orders
        .iter()
        .filter(|order| order.needs.as_str().starts_with("turn:"))
        .collect();
    assert_eq!(turns.len(), 1, "fixture 03 orders one conflict");
    let all = made("03");
    let later = ticket(&all, turns[0].to.as_str());
    let area = turns[0]
        .needs
        .as_str()
        .strip_prefix("turn:")
        .expect("a turn");
    assert_eq!(
        later.body.turns,
        [Turn(turns[0].from.clone(), Area::from(area))]
    );
    assert!(
        later
            .body
            .to_string()
            .contains(&format!("Turn: after {} on {area}\n", turns[0].from))
    );
    // The node that goes first has no turn, and no ticket holds a wait that a name makes.
    assert!(ticket(&all, turns[0].from.as_str()).body.turns.is_empty());
}

#[test]
fn tickets_parse_back() {
    let mut checked = 0;
    for (file, fixture) in fixtures() {
        if !matches!(fixture.expect.outcome(), Outcome::Plan { .. }) {
            continue;
        }
        let optimised = optimise(&fixture.plan, fixture.options).expect("optimises");
        let all = match tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised)) {
            Ok(all) => all,
            // A fixture whose kept waits come from a node that is no epic's and no task's, as a
            // filed ticket by key, cannot be written as a `Waits on:` line.
            Err(errors) => {
                assert!(
                    errors
                        .iter()
                        .all(|error| matches!(error, TicketError::BadWait { .. })),
                    "{file}: {errors:?}"
                );
                continue;
            }
        };
        let open = optimised
            .nodes
            .iter()
            .filter(|node| node.filed.is_none())
            .count();
        assert_eq!(
            all.len(),
            open,
            "{file}: a ticket for each node still to file"
        );
        for made in &all {
            let text = made.body.to_string();
            let read: PlannedTicket = text.parse().unwrap_or_else(|error| {
                panic!("{file}: {} does not parse: {error}\n{text}", made.node)
            });
            assert_eq!(read, made.body, "{file}: {}", made.node);
            assert!(PlannedTicket::is_planned(&text), "{file}: {}", made.node);
            assert!(
                made.body
                    .sections
                    .iter()
                    .all(|s| s.template.blocked_by.is_empty())
            );
        }
        checked += 1;
    }
    assert!(checked >= 19, "the fixtures are checked ({checked})");
}

#[test]
fn tickets_split_parts_take_their_behaviours_section() {
    let fixture = fixture("04");
    let optimised = optimise(&fixture.plan, fixture.options).expect("optimises");
    let node: &Node = fixture
        .plan
        .nodes
        .iter()
        .find(|node| node.behaviours.len() > 1)
        .expect("fixture 04 has a node of two behaviours");
    // The section of a written node of several behaviours is the node's; the parts differ by
    // their behaviour and by a title suffix.
    let all =
        tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised)).expect("tickets");
    let parts: Vec<&TicketToFile> = all
        .iter()
        .filter(|ticket| ticket.node.as_str().starts_with(&format!("{}.", node.id)))
        .collect();
    assert_eq!(parts.len(), node.behaviours.len());
    let titles: BTreeSet<&str> = parts.iter().map(|ticket| ticket.title.as_str()).collect();
    assert_eq!(titles.len(), parts.len(), "no two parts share a title");
    for (part, behaviour) in parts.iter().zip(&node.behaviours) {
        assert_eq!(part.body.sections[0].template.goal, behaviour.text);
    }
}

#[test]
fn tickets_open_filed_nodes_make_no_ticket() {
    let fixture = fixture("27");
    let optimised = optimise(&fixture.plan, fixture.options).expect("optimises");
    let filed: Vec<&NodeId> = optimised
        .nodes
        .iter()
        .filter(|node| node.filed.is_some())
        .map(|node| &node.id)
        .collect();
    assert!(!filed.is_empty(), "fixture 27 holds a filed ticket");
    if let Ok(all) = tickets(&written(&fixture.plan), &optimised, &shapes_of(&optimised)) {
        assert!(all.iter().all(|ticket| !filed.contains(&&ticket.node)));
    }
}
