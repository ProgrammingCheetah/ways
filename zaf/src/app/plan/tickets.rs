//! The node-to-ticket step (I2; D3, D9, D11, D13; M10): each node of an optimised plan becomes
//! the ticket to file for it, its title, group, type and planned body.
//!
//! The step is pure over the optimised plan, the plan as the planner wrote it and the shapes of
//! the names that have a signature. Nothing is filed here, and the body's text form is
//! [`PlannedTicket`]'s.
//!
//! - A node from one section keeps that section's title, group, type and prose fields.
//! - A merged node `A+B` has one section per behaviour, each opened by `Behaviour: <part id>`; its
//!   title is its parts' titles joined with ` + ` in natural id order, and its group and type are
//!   those of the lowest part (the Engineer's ruling on #1213).
//! - A split node `A.a` takes the section its behaviour came from: the section written for `A`,
//!   with the behaviour's own text as its goal and its own weight as its size, and a title that
//!   ends `(a)` so no two parts share a title (the board files a title once).
//! - The interface node is one `Kind: interface` ticket: `Signatures:` holds each name's shape,
//!   and its section is written from D11.
//! - A wait `turn:<area>` becomes a `Turn:` line and a kept wait a `Waits on:` line, as the plan
//!   meant it: `epic #<n>` for a name another epic's interface ticket `I@<n>.<k>` holds, `#<n>`
//!   for a task. Every other wait is read from the sections' `Needs:` and `Provides:`.
//! - A behaviour that provided a name the lift moved into an interface now `Implements:` it.
//!
//! An open filed ticket (D29) is already filed and makes no ticket. A body that does not parse
//! back to what was made, or whose sections weigh too much, is an error: nothing is repaired.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::plan::text::{ParsedPlan, PlanSection};
use crate::domain::plan::{
    Area, Behaviour, Name, Node, NodeId, NodeKey, NodeKind, Optimised, PlannedTicket, Section,
    Signature, Turn, Wait, WaitOn, Weight,
};
use crate::domain::{SecurityTrigger, TaskField, TaskModel, TaskSize, TaskTemplate, TicketId};

/// What a wait on a turn is written as: `turn:<area>` in an order.
const TURN_PREFIX: &str = "turn:";
/// An interface ticket's one acceptance line (D11).
const INTERFACE_ACCEPTANCE: &str = "the gate passes with every stand-in";

/// A ticket to file for one node of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketToFile {
    /// The node it is.
    pub node: NodeId,
    /// Its title.
    pub title: String,
    /// Its group, when the plan gave one.
    pub group: Option<String>,
    /// Its issue type, when the plan gave one: `Task` when none.
    pub kind: Option<String>,
    /// Its body.
    pub body: PlannedTicket,
    /// Its size, derived from its sections (D3).
    pub size: TaskSize,
    /// Its model, derived from its sections (D3).
    pub model: TaskModel,
}

/// Why a node makes no ticket. Each message names the node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TicketError {
    /// The plan as written has no section for a node, or for the node a part was split from.
    #[error("node {node}: the plan as written has no section for {part}")]
    NoSection {
        /// The node.
        node: NodeId,
        /// The part with no section.
        part: NodeId,
    },
    /// A node of several behaviours whose id does not give one part to each.
    #[error("node {node}: its id gives {parts} part(s) to {behaviours} behaviours")]
    BadParts {
        /// The node.
        node: NodeId,
        /// How many parts its id names.
        parts: usize,
        /// How many behaviours it holds.
        behaviours: usize,
    },
    /// An interface name that neither the vocabulary nor the plan's new names give a shape.
    #[error("node {node}: the name {name} has no shape in the vocabulary or the new names")]
    NoShape {
        /// The interface node.
        node: NodeId,
        /// The name.
        name: Name,
    },
    /// A kept wait on a node that is neither an epic's interface ticket nor a task by number.
    #[error(
        "node {node}: the wait on {name} comes from {from}, which is neither I@<epic>.<k> nor #<n>"
    )]
    BadWait {
        /// The node that waits.
        node: NodeId,
        /// The node `outside` gives the name.
        from: NodeId,
        /// The name.
        name: Name,
    },
    /// A section the template refuses, or a ticket too heavy to file.
    #[error("node {node}: {detail}")]
    Template {
        /// The node.
        node: NodeId,
        /// The template's message.
        detail: String,
    },
    /// A body that does not parse back to the ticket made.
    #[error("node {node}: its body does not parse back: {detail}")]
    NotParsed {
        /// The node.
        node: NodeId,
        /// What the parser said, or that the ticket read back differently.
        detail: String,
    },
}

/// What every node's ticket reads from.
struct Context<'a> {
    parsed: &'a ParsedPlan,
    optimised: &'a Optimised,
    /// The shape of each name with a signature: the vocabulary's and the plan's.
    shapes: BTreeMap<Name, String>,
    /// The names an interface node of the plan provides.
    interface_names: BTreeSet<Name>,
}

/// The tickets of an optimised plan, one for each node that is not an open filed ticket, by
/// natural id; or every reason a node makes none.
///
/// `shapes` holds the shape of each name the vocabulary gives a signature; the plan's own
/// `New names:` are in `parsed.signatures`.
///
/// # Errors
///
/// Each [`TicketError`], in node order: a node with no section, a name with no shape, a wait on
/// a node that names no task, a section the template refuses, a ticket past the limit, a body
/// that does not parse back.
pub fn tickets(
    parsed: &ParsedPlan,
    optimised: &Optimised,
    shapes: &BTreeMap<Name, String>,
) -> Result<Vec<TicketToFile>, Vec<TicketError>> {
    let mut all = shapes.clone();
    for (name, shape) in &parsed.signatures {
        all.entry(name.clone()).or_insert_with(|| shape.clone());
    }
    let interface_names = optimised
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Interface)
        .flat_map(|node| &node.behaviours)
        .flat_map(|behaviour| behaviour.provides.iter().cloned())
        .collect();
    let context = Context {
        parsed,
        optimised,
        shapes: all,
        interface_names,
    };
    let mut errors = Vec::new();
    let mut made = Vec::new();
    let open = |node: &&Node| node.filed.is_none();
    for node in optimised
        .nodes
        .iter()
        .filter(open)
        .filter(|n| n.kind == NodeKind::Body)
    {
        match body_ticket(&context, node) {
            Ok(ticket) => made.push(ticket),
            Err(mut found) => errors.append(&mut found),
        }
    }
    for node in optimised
        .nodes
        .iter()
        .filter(open)
        .filter(|n| n.kind == NodeKind::Interface)
    {
        match interface_ticket(&context, node, &made) {
            Ok(ticket) => made.push(ticket),
            Err(mut found) => errors.append(&mut found),
        }
    }
    made.sort_by(|a, b| a.node.cmp(&b.node));
    if errors.is_empty() {
        Ok(made)
    } else {
        Err(errors)
    }
}

/// One behaviour of a node with the section written for it.
struct Part<'a> {
    id: NodeId,
    behaviour: &'a Behaviour,
    section: &'a PlanSection,
    /// The behaviour as written, when the written node can be told.
    written: Option<&'a Behaviour>,
    /// The suffix of a split part (`a` of `A.a`): the section was written for the node before it.
    split: Option<String>,
}

/// The ids of the parts a node's behaviours belong to: the node's own id for one behaviour, a
/// merged id's parts for several.
fn part_ids(node: &Node) -> Result<Vec<NodeId>, TicketError> {
    let count = node.behaviours.len();
    if count == 1 {
        return Ok(vec![node.id.clone()]);
    }
    let parts: Vec<&str> = node.id.as_str().split('+').collect();
    if count > 1 && parts.len() == count {
        let mut ids: Vec<NodeId> = parts.into_iter().map(NodeId::from).collect();
        ids.sort();
        return Ok(ids);
    }
    Err(TicketError::BadParts {
        node: node.id.clone(),
        parts: parts.len(),
        behaviours: count,
    })
}

/// The index a split suffix names: `a` to `z` are 0 to 25, a number is itself.
fn suffix_index(suffix: &str) -> Option<usize> {
    let mut letters = suffix.chars();
    match (letters.next(), letters.next()) {
        (Some(letter @ 'a'..='z'), None) => Some(usize::from(letter as u8 - b'a')),
        _ => suffix.parse().ok(),
    }
}

/// The section written for a part and the behaviour written in it. A part with a section of its
/// own id uses it; `A.a` falls back to the section of `A` and the behaviour `a` names.
fn resolve<'a>(
    context: &Context<'a>,
    node: &NodeId,
    id: &NodeId,
    behaviour: &'a Behaviour,
) -> Result<Part<'a>, TicketError> {
    let section_of = |wanted: &str| {
        context
            .parsed
            .sections
            .iter()
            .find(|section| section.node.as_str() == wanted)
    };
    let written_node = |wanted: &str| {
        context
            .parsed
            .input
            .nodes
            .iter()
            .find(|candidate| candidate.id.as_str() == wanted)
    };
    if let Some(section) = section_of(id.as_str()) {
        let written = written_node(id.as_str())
            .filter(|written| written.behaviours.len() == 1)
            .and_then(|written| written.behaviours.first());
        return Ok(Part {
            id: id.clone(),
            behaviour,
            section,
            written,
            split: None,
        });
    }
    let no_section = || TicketError::NoSection {
        node: node.clone(),
        part: id.clone(),
    };
    let (base, suffix) = id.as_str().rsplit_once('.').ok_or_else(no_section)?;
    let section = section_of(base).ok_or_else(no_section)?;
    let written = written_node(base)
        .zip(suffix_index(suffix))
        .and_then(|(written, index)| written.behaviours.get(index));
    Ok(Part {
        id: id.clone(),
        behaviour,
        section,
        written,
        split: Some(suffix.to_owned()),
    })
}

/// A behaviour's weight as the template's size: its own size for a whole section, else the size
/// its weight gives.
fn size_of(weight: Option<Weight>, written: TaskSize) -> TaskSize {
    match weight {
        Some(Weight(2)) => TaskSize::M,
        _ if written == TaskSize::XS => TaskSize::XS,
        _ => TaskSize::S,
    }
}

/// The section of one part (D3).
fn section_of(context: &Context<'_>, part: &Part<'_>, keyed: bool) -> Result<Section, TicketError> {
    let mut fields = part.section.fields.clone();
    if part.split.is_some() {
        fields.insert(TaskField::Goal, part.behaviour.text.clone());
        let written = fields
            .get(&TaskField::Size)
            .and_then(|size| size.parse::<TaskSize>().ok())
            .unwrap_or(TaskSize::S);
        fields.insert(
            TaskField::Size,
            size_of(part.behaviour.size.weight(), written).to_string(),
        );
    }
    let template = TaskTemplate::from_plan_fields(fields, part.section.acceptance.clone())
        .map_err(|error| TicketError::Template {
            node: part.id.clone(),
            detail: error.to_string(),
        })?;
    let provided = part.written.map(|written| &written.provides);
    let implements: BTreeSet<Name> = part
        .behaviour
        .needs
        .iter()
        .filter(|name| context.interface_names.contains(*name))
        .filter(|name| provided.is_some_and(|provided| provided.contains(*name)))
        .cloned()
        .collect();
    let needs = part
        .behaviour
        .needs
        .difference(&implements)
        .cloned()
        .collect();
    Ok(Section {
        key: keyed.then(|| NodeKey::from(part.id.as_str())),
        template,
        writes: part.behaviour.writes.clone(),
        provides: part.behaviour.provides.clone(),
        implements,
        needs,
    })
}

/// The ticket lines only zaf writes for a node: its turns and its kept waits.
fn lines_of(
    context: &Context<'_>,
    node: &NodeId,
) -> Result<(Vec<Turn>, Vec<Wait>), Vec<TicketError>> {
    let turns = context
        .optimised
        .orders
        .iter()
        .filter(|order| order.to == *node)
        .filter_map(|order| {
            let area = order.needs.as_str().strip_prefix(TURN_PREFIX)?;
            Some(Turn(order.from.clone(), Area::from(area)))
        })
        .collect();
    let mut waits = Vec::new();
    let mut errors = Vec::new();
    for kept in context
        .optimised
        .kept
        .iter()
        .filter(|kept| kept.to == *node)
    {
        match wait_on(&kept.from) {
            Some(on) => waits.push(Wait {
                on,
                name: kept.needs.clone(),
            }),
            None => errors.push(TicketError::BadWait {
                node: node.clone(),
                from: kept.from.clone(),
                name: kept.needs.clone(),
            }),
        }
    }
    if errors.is_empty() {
        Ok((turns, waits))
    } else {
        Err(errors)
    }
}

/// What a kept wait's provider stands for, as the plan meant it: another epic's interface ticket
/// `I@<n>.<k>` is `epic #<n>`, and `#<n>` is a task.
fn wait_on(from: &NodeId) -> Option<WaitOn> {
    let text = from.as_str();
    if let Some(rest) = text.strip_prefix("I@") {
        let epic = rest.split('.').next()?;
        return epic.parse::<TicketId>().ok().map(WaitOn::Epic);
    }
    text.starts_with('#')
        .then(|| text.parse::<TicketId>().ok())
        .flatten()
        .map(WaitOn::Task)
}

/// The ticket of a body node.
fn body_ticket(context: &Context<'_>, node: &Node) -> Result<TicketToFile, Vec<TicketError>> {
    let ids = part_ids(node).map_err(|error| vec![error])?;
    // The behaviours come in the order of the id's parts, which are in natural order already.
    let mut behaviours: Vec<&Behaviour> = node.behaviours.iter().collect();
    if ids.len() > 1 {
        let listed: Vec<NodeId> = node.id.as_str().split('+').map(NodeId::from).collect();
        let mut paired: Vec<(NodeId, &Behaviour)> = listed.into_iter().zip(behaviours).collect();
        paired.sort_by(|a, b| a.0.cmp(&b.0));
        behaviours = paired.into_iter().map(|(_, behaviour)| behaviour).collect();
    }
    let mut parts = Vec::new();
    let mut errors = Vec::new();
    for (id, behaviour) in ids.iter().zip(behaviours) {
        match resolve(context, &node.id, id, behaviour) {
            Ok(part) => parts.push(part),
            Err(error) => errors.push(error),
        }
    }
    let keyed = parts.len() > 1;
    let mut sections = Vec::new();
    for part in &parts {
        match section_of(context, part, keyed) {
            Ok(section) => sections.push(section),
            Err(error) => errors.push(error),
        }
    }
    let (turns, waits) = lines_of(context, &node.id).unwrap_or_else(|mut found| {
        errors.append(&mut found);
        (Vec::new(), Vec::new())
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    let lowest = parts.first().expect("a body node holds at least one part");
    let title = parts
        .iter()
        .map(|part| title_of(part))
        .collect::<Vec<_>>()
        .join(" + ");
    let body = PlannedTicket {
        node: node.id.clone(),
        kind: NodeKind::Body,
        signatures: Vec::new(),
        turns,
        waits,
        sections,
    };
    finish(
        node,
        title,
        lowest.section.group.clone(),
        lowest.section.kind.clone(),
        body,
    )
}

/// A part's title: its section's, and for a split part the suffix that tells it from the others.
fn title_of(part: &Part<'_>) -> String {
    match &part.split {
        Some(suffix) => format!("{} ({suffix})", part.section.title),
        None => part.section.title.clone(),
    }
}

/// The ticket of an interface node (D11), written from the shapes and from the tickets that
/// implement or need its names.
fn interface_ticket(
    context: &Context<'_>,
    node: &Node,
    bodies: &[TicketToFile],
) -> Result<TicketToFile, Vec<TicketError>> {
    let names: BTreeSet<Name> = node
        .behaviours
        .iter()
        .flat_map(|behaviour| behaviour.provides.iter().cloned())
        .collect();
    let mut errors = Vec::new();
    let mut signatures = Vec::new();
    for name in &names {
        match context.shapes.get(name) {
            Some(shape) => signatures.push(Signature {
                name: name.clone(),
                shape: shape.clone(),
            }),
            None => errors.push(TicketError::NoShape {
                node: node.id.clone(),
                name: name.clone(),
            }),
        }
    }
    let (turns, waits) = lines_of(context, &node.id).unwrap_or_else(|mut found| {
        errors.append(&mut found);
        (Vec::new(), Vec::new())
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    let listed: Vec<&str> = names.iter().map(Name::as_str).collect();
    let listed = listed.join(", ");
    let users: Vec<&Section> = bodies
        .iter()
        .flat_map(|ticket| &ticket.body.sections)
        .filter(|section| {
            names
                .iter()
                .any(|name| section.implements.contains(name) || section.needs.contains(name))
        })
        .collect();
    let pages: BTreeSet<&str> = users
        .iter()
        .flat_map(|section| section.template.design_pages.split(','))
        .map(str::trim)
        .filter(|page| !page.is_empty() && *page != "none")
        .collect();
    let triggers: BTreeSet<&str> = bodies
        .iter()
        .flat_map(|ticket| &ticket.body.sections)
        .filter(|section| names.iter().any(|name| section.implements.contains(name)))
        .filter_map(|section| match &section.template.security_trigger {
            SecurityTrigger::Yes(which) => Some(which.as_str()),
            SecurityTrigger::No => None,
        })
        .collect();
    let template = TaskTemplate {
        goal: format!("the signatures of {listed}: stand-ins that build, with no behaviour"),
        owned_paths: format!("the stand-ins for {listed}"),
        scoped: false,
        out_of_scope: None,
        interfaces: format!("defines {listed}; the tasks that implement or need them consume"),
        design_pages: if pages.is_empty() {
            "none".to_owned()
        } else {
            pages.into_iter().collect::<Vec<_>>().join(", ")
        },
        architecture: String::new(),
        acceptance: vec![INTERFACE_ACCEPTANCE.to_owned()],
        tests: "none".to_owned(),
        security_trigger: if triggers.is_empty() {
            SecurityTrigger::No
        } else {
            SecurityTrigger::Yes(triggers.into_iter().collect::<Vec<_>>().join("; "))
        },
        model: TaskModel::Sonnet,
        size: if signatures.len() >= 3 {
            TaskSize::M
        } else {
            TaskSize::S
        },
        blocked_by: Vec::new(),
        style: None,
    };
    let section = Section {
        key: None,
        template,
        writes: node
            .behaviours
            .iter()
            .flat_map(|behaviour| behaviour.writes.iter().cloned())
            .collect(),
        provides: names,
        implements: BTreeSet::new(),
        needs: BTreeSet::new(),
    };
    let body = PlannedTicket {
        node: node.id.clone(),
        kind: NodeKind::Interface,
        signatures,
        turns,
        waits,
        sections: vec![section],
    };
    finish(
        node,
        format!("The signatures of {listed}"),
        None,
        None,
        body,
    )
}

/// Derives the size and model, and checks the body reads back as the ticket made.
fn finish(
    node: &Node,
    title: String,
    group: Option<String>,
    kind: Option<String>,
    body: PlannedTicket,
) -> Result<TicketToFile, Vec<TicketError>> {
    let size = body.size().map_err(|error| {
        vec![TicketError::Template {
            node: node.id.clone(),
            detail: error.to_string(),
        }]
    })?;
    let not_parsed = |detail: String| {
        vec![TicketError::NotParsed {
            node: node.id.clone(),
            detail,
        }]
    };
    match body.to_string().parse::<PlannedTicket>() {
        Ok(read) if read == body => {}
        Ok(_) => return Err(not_parsed("it reads back as another ticket".to_owned())),
        Err(error) => return Err(not_parsed(error.to_string())),
    }
    let model = body.model();
    Ok(TicketToFile {
        node: node.id.clone(),
        title,
        group,
        kind,
        body,
        size,
        model,
    })
}
