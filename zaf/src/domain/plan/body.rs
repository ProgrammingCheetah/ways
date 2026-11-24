//! The body of a planned ticket (I2): what zaf writes on a ticket it files from a plan, and what
//! it reads back.
//!
//! Ticket lines come first: `Node: <id>`, `Kind: interface` with `Signatures:` and one item
//! `<name>: <shape>` each, `Turn: after <node> on <area>`, `Waits on: #<n> for <name>` and
//! `Waits on: epic #<n> for <name>`. Then one section for each behaviour, opened by
//! `Behaviour: <key>` only when there is more than one. A section holds the template's fields
//! without `Blocked by`, then `Writes`, `Provides`, `Implements` and `Needs` as lists of names.
//!
//! A body with no `Node:` line is not a planned body: the older template reads it
//! ([`PlannedTicket::is_planned`] tells which). A malformed planned body is refused with a
//! [`TaskTemplateError`] that names the line or the field.
//!
//! A ticket's size and model are derived from its sections ([`PlannedTicket::size`] and
//! [`PlannedTicket::model`]); a section holds the template's `Size` and `Model` as written.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use crate::domain::task::Blockers;
use crate::domain::{
    TaskField, TaskModel, TaskProblem, TaskSize, TaskTemplate, TaskTemplateError, TicketId,
};

use super::{Area, Name, NodeId, NodeKey, NodeKind, Turn};

const NODE: &str = "Node";
const KIND: &str = "Kind";
const SIGNATURES: &str = "Signatures";
const TURN: &str = "Turn";
const WAITS_ON: &str = "Waits on";
const BEHAVIOUR: &str = "Behaviour";
const WRITES: &str = "Writes";
const PROVIDES: &str = "Provides";
const IMPLEMENTS: &str = "Implements";
const NEEDS: &str = "Needs";
const BLOCKED_BY: &str = "Blocked by";

/// The most the sections of a ticket zaf files may weigh together (D10).
pub const BODY_WEIGHT_LIMIT: u32 = 2;
/// The interface ticket is `M` from this many signatures (D11).
const INTERFACE_M_SIGNATURES: usize = 3;

/// What a node id accepts: its characters are letters, digits and `-_.+@:`.
const NODE_ID_ACCEPTS: &str = "an id of letters, digits and - _ . + @ : (no #, no spaces)";
/// What a name accepts.
const NAME_ACCEPTS: &str = "a name with no spaces and no comma";
/// What an area accepts.
const AREA_ACCEPTS: &str = "an area on one line with no comma";
/// What a behaviour key accepts.
const KEY_ACCEPTS: &str = "a key of letters, digits and - _ . @ :";

/// A name an interface ticket gives a signature to, with the shape it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The name.
    pub name: Name,
    /// The shape, on one line.
    pub shape: String,
}

/// The task a `Waits on:` line names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaitOn {
    /// A task named by its number: `#<n>`.
    Task(TicketId),
    /// Another epic, named by its number: `epic #<n>`.
    Epic(TicketId),
}

impl fmt::Display for WaitOn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Task(id) => write!(formatter, "{id}"),
            Self::Epic(id) => write!(formatter, "epic {id}"),
        }
    }
}

/// A wait on another epic's task or on a filed one, kept as written (D13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wait {
    /// What it waits on.
    pub on: WaitOn,
    /// The name the wait is on.
    pub name: Name,
}

/// One behaviour of a planned ticket: the template's fields without `Blocked by`, and the names
/// it writes, provides, implements and needs.
///
/// `template.blocked_by` is always empty: the graph holds the waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The `Behaviour:` key; set when the ticket has more than one section and only then.
    pub key: Option<NodeKey>,
    /// The template's fields.
    pub template: TaskTemplate,
    /// The areas it writes.
    pub writes: BTreeSet<Area>,
    /// The names it provides.
    pub provides: BTreeSet<Name>,
    /// The names the interface task gave a signature to, which this behaviour builds the body
    /// of; the graph reads each as a need.
    pub implements: BTreeSet<Name>,
    /// The names it waits for.
    pub needs: BTreeSet<Name>,
}

/// The body of a ticket filed from a plan (I2).
///
/// The sections' keys are set when there are several sections and only then; a ticket that
/// breaks that does not render as text that parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTicket {
    /// The node: the planner's key, `A+B` for a merge, `A.a` for a part, or `I@<epic>.<k>`.
    pub node: NodeId,
    /// `Kind: interface` for the interface ticket, a body otherwise.
    pub kind: NodeKind,
    /// The interface ticket's names with their shapes; empty on any other.
    pub signatures: Vec<Signature>,
    /// The waits a filed ticket kept: `Turn: after <node> on <area>`.
    pub turns: Vec<Turn>,
    /// The waits on another epic's task or on a filed one.
    pub waits: Vec<Wait>,
    /// One section for each behaviour; at least one.
    pub sections: Vec<Section>,
}

impl PlannedTicket {
    /// Whether a body is a planned one: it has a `Node:` line. A body without one is read by the
    /// older template.
    pub fn is_planned(text: &str) -> bool {
        text.lines().any(|line| line.starts_with("Node:"))
    }

    /// The ticket's size, derived from its sections (D3, D10, D11).
    ///
    /// The interface ticket is `S`, or `M` from three signatures. Any other ticket weighs its
    /// sections together, XS and S 1 and M 2: weight 1 is `S` (`XS` when its one behaviour is
    /// `XS`) and weight 2 is `M`.
    ///
    /// # Errors
    ///
    /// [`TaskTemplateError::TooLarge`] when the sections weigh more than 2 (or one is sized
    /// `L`, `XL` or `E`): zaf files no such ticket, so no size is guessed.
    pub fn size(&self) -> Result<TaskSize, TaskTemplateError> {
        if self.kind == NodeKind::Interface {
            return Ok(if self.signatures.len() >= INTERFACE_M_SIGNATURES {
                TaskSize::M
            } else {
                TaskSize::S
            });
        }
        let weight = self.weight();
        match (weight, self.sections.as_slice()) {
            (..=1, [only]) if only.template.size == TaskSize::XS => Ok(TaskSize::XS),
            (..=1, _) => Ok(TaskSize::S),
            (2, _) => Ok(TaskSize::M),
            _ => Err(TaskTemplateError::TooLarge {
                weight,
                limit: BODY_WEIGHT_LIMIT,
            }),
        }
    }

    /// The ticket's model: `opus` if any section says so, else `sonnet` (D3).
    pub fn model(&self) -> TaskModel {
        if self
            .sections
            .iter()
            .any(|section| section.template.model == TaskModel::Opus)
        {
            TaskModel::Opus
        } else {
            TaskModel::Sonnet
        }
    }

    /// What the ticket weighs as a node of the graph, as [`measure`](super::measure::measure)
    /// weighs it: an interface ticket 1 a signature, any other the sum of its sections.
    pub fn node_weight(&self) -> u32 {
        match self.kind {
            NodeKind::Interface => u32::try_from(self.signatures.len()).unwrap_or(u32::MAX),
            NodeKind::Body => self.weight(),
        }
    }

    /// What the sections weigh together: XS and S 1, M 2, and a size past M the limit plus 1, so
    /// that it is never within it.
    pub fn weight(&self) -> u32 {
        self.sections
            .iter()
            .map(|section| match section.template.size {
                TaskSize::XS | TaskSize::S => 1,
                TaskSize::M => 2,
                TaskSize::L | TaskSize::XL | TaskSize::E => BODY_WEIGHT_LIMIT + 1,
            })
            .sum()
    }
}

impl fmt::Display for Section {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(key) = &self.key {
            writeln!(formatter, "{BEHAVIOUR}: {key}")?;
        }
        self.template.write_fields(formatter, false)?;
        write_list(formatter, WRITES, &self.writes)?;
        write_list(formatter, PROVIDES, &self.provides)?;
        write_list(formatter, IMPLEMENTS, &self.implements)?;
        write_list(formatter, NEEDS, &self.needs)
    }
}

fn write_list<T: fmt::Display>(
    formatter: &mut fmt::Formatter<'_>,
    label: &str,
    items: &BTreeSet<T>,
) -> fmt::Result {
    if items.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = items.iter().map(ToString::to_string).collect();
    writeln!(formatter, "{label}: {}", names.join(", "))
}

impl fmt::Display for PlannedTicket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{NODE}: {}", self.node)?;
        if self.kind == NodeKind::Interface {
            writeln!(formatter, "{KIND}: interface")?;
        }
        if !self.signatures.is_empty() {
            writeln!(formatter, "{SIGNATURES}:")?;
            for signature in &self.signatures {
                writeln!(formatter, "  - {}: {}", signature.name, signature.shape)?;
            }
        }
        for Turn(after, area) in &self.turns {
            writeln!(formatter, "{TURN}: after {after} on {area}")?;
        }
        for wait in &self.waits {
            writeln!(formatter, "{WAITS_ON}: {} for {}", wait.on, wait.name)?;
        }
        for section in &self.sections {
            writeln!(formatter)?;
            write!(formatter, "{section}")?;
        }
        Ok(())
    }
}

/// A line of a planned body whose value is outside what its label accepts.
fn bad(line: usize, label: &str, value: &str, accepts: &str) -> TaskTemplateError {
    TaskTemplateError::BadPlannedLine {
        line,
        label: label.to_owned(),
        value: value.to_owned(),
        accepts: accepts.to_owned(),
    }
}

fn is_word(text: &str, extra: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

fn node_id(line: usize, label: &str, text: &str) -> Result<NodeId, TaskTemplateError> {
    if is_word(text, "-_.+@:") {
        Ok(NodeId::from(text))
    } else {
        Err(bad(line, label, text, NODE_ID_ACCEPTS))
    }
}

fn name(line: usize, label: &str, text: &str) -> Result<Name, TaskTemplateError> {
    if text.is_empty() || text.contains(',') || text.contains(char::is_whitespace) {
        Err(bad(line, label, text, NAME_ACCEPTS))
    } else {
        Ok(Name::from(text))
    }
}

fn area(line: usize, label: &str, text: &str) -> Result<Area, TaskTemplateError> {
    if text.is_empty() || text.contains(',') || text.contains(char::is_control) {
        Err(bad(line, label, text, AREA_ACCEPTS))
    } else {
        Ok(Area::from(text))
    }
}

fn names(line: usize, label: &str, value: &str) -> Result<BTreeSet<Name>, TaskTemplateError> {
    split_list(value)
        .map(|item| name(line, label, item))
        .collect()
}

fn areas(line: usize, label: &str, value: &str) -> Result<BTreeSet<Area>, TaskTemplateError> {
    split_list(value)
        .map(|item| area(line, label, item))
        .collect()
}

/// The items of a comma list; an empty value holds none.
fn split_list(value: &str) -> impl Iterator<Item = &str> {
    let items: Vec<&str> = if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(str::trim).collect()
    };
    items.into_iter()
}

fn turn(line: usize, value: &str) -> Result<Turn, TaskTemplateError> {
    let accepts = "after <node> on <area>";
    let (after, on) = value
        .strip_prefix("after ")
        .and_then(|rest| rest.split_once(" on "))
        .ok_or_else(|| bad(line, TURN, value, accepts))?;
    Ok(Turn(
        node_id(line, TURN, after.trim())?,
        area(line, TURN, on.trim())?,
    ))
}

fn wait(line: usize, value: &str) -> Result<Wait, TaskTemplateError> {
    let accepts = "#<n> for <name> | epic #<n> for <name>";
    let (on, name_text) = value
        .split_once(" for ")
        .ok_or_else(|| bad(line, WAITS_ON, value, accepts))?;
    let (epic, number) = match on.strip_prefix("epic ") {
        Some(number) => (true, number.trim()),
        None => (false, on.trim()),
    };
    let id = number
        .starts_with('#')
        .then(|| number.parse::<TicketId>().ok())
        .flatten()
        .ok_or_else(|| bad(line, WAITS_ON, value, accepts))?;
    let on = if epic {
        WaitOn::Epic(id)
    } else {
        WaitOn::Task(id)
    };
    Ok(Wait {
        on,
        name: name(line, WAITS_ON, name_text.trim())?,
    })
}

fn signature(line: usize, item: &str) -> Result<Signature, TaskTemplateError> {
    let accepts = "<name>: <shape>";
    let (name_text, shape) = item
        .split_once(": ")
        .ok_or_else(|| bad(line, SIGNATURES, item, accepts))?;
    let shape = shape.trim();
    if shape.is_empty() {
        return Err(bad(line, SIGNATURES, item, accepts));
    }
    Ok(Signature {
        name: name(line, SIGNATURES, name_text.trim())?,
        shape: shape.to_owned(),
    })
}

/// Where the line scan stands: what a continuation line or a list item belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Current {
    Nothing,
    Field(TaskField),
    Signatures,
    /// Inside a line already reported (only [`PlannedTicket::problems`] gets here): the lines
    /// under it are not reported again.
    Reported,
}

/// One section while it is read.
#[derive(Default)]
struct SectionScan {
    line: usize,
    key: Option<NodeKey>,
    values: BTreeMap<TaskField, String>,
    acceptance: Vec<String>,
    writes: Option<BTreeSet<Area>>,
    provides: Option<BTreeSet<Name>>,
    implements: Option<BTreeSet<Name>>,
    needs: Option<BTreeSet<Name>>,
}

impl SectionScan {
    fn finish(self) -> Result<Section, TaskTemplateError> {
        let template = TaskTemplate::from_fields(self.values, self.acceptance, Blockers::Refused)?;
        Ok(Section {
            key: self.key,
            template,
            writes: self.writes.unwrap_or_default(),
            provides: self.provides.unwrap_or_default(),
            implements: self.implements.unwrap_or_default(),
            needs: self.needs.unwrap_or_default(),
        })
    }
}

/// The scan of a planned body.
struct Scan {
    node: Option<NodeId>,
    kind: Option<NodeKind>,
    signatures: Option<(usize, Vec<Signature>)>,
    turns: Vec<Turn>,
    waits: Vec<Wait>,
    sections: Vec<SectionScan>,
    current: Current,
}

impl Scan {
    fn new() -> Self {
        Self {
            node: None,
            kind: None,
            signatures: None,
            turns: Vec::new(),
            waits: Vec::new(),
            sections: Vec::new(),
            current: Current::Nothing,
        }
    }

    fn line(&mut self, line: usize, raw: &str) -> Result<(), TaskTemplateError> {
        let content = raw.trim();
        if content.is_empty() {
            return Ok(());
        }
        if let Some(item) = content.strip_prefix("- ") {
            return self.item(line, item.trim());
        }
        let unrecognized = || TaskTemplateError::UnrecognizedLine {
            line,
            text: content.to_owned(),
        };
        if raw.starts_with([' ', '\t']) {
            if self.current == Current::Reported {
                return Ok(());
            }
            let Current::Field(field) = self.current else {
                return Err(unrecognized());
            };
            let continued = self
                .sections
                .last_mut()
                .and_then(|section| section.values.get_mut(&field))
                .ok_or_else(unrecognized)?;
            if !continued.is_empty() {
                continued.push('\n');
            }
            continued.push_str(content);
            return Ok(());
        }
        let (label, value) = content.split_once(':').ok_or_else(unrecognized)?;
        self.labelled(line, label.trim(), value.trim())
    }

    fn item(&mut self, line: usize, item: &str) -> Result<(), TaskTemplateError> {
        match self.current {
            Current::Field(TaskField::Acceptance) => {
                if let Some(section) = self.sections.last_mut() {
                    section.acceptance.push(item.to_owned());
                }
                Ok(())
            }
            Current::Signatures => {
                let parsed = signature(line, item)?;
                if let Some((_, signatures)) = self.signatures.as_mut() {
                    signatures.push(parsed);
                }
                Ok(())
            }
            Current::Reported => Ok(()),
            _ => Err(TaskTemplateError::StrayListItem { line }),
        }
    }

    fn labelled(&mut self, line: usize, label: &str, value: &str) -> Result<(), TaskTemplateError> {
        match label {
            NODE | KIND | SIGNATURES | TURN | WAITS_ON => self.ticket_line(line, label, value),
            BEHAVIOUR => self.behaviour(line, value),
            WRITES | PROVIDES | IMPLEMENTS | NEEDS => self.list(line, label, value),
            BLOCKED_BY => Err(TaskTemplateError::BlockedByInPlannedBody { line }),
            _ => self.field(line, label, value),
        }
    }

    fn ticket_line(
        &mut self,
        line: usize,
        label: &str,
        value: &str,
    ) -> Result<(), TaskTemplateError> {
        if !self.sections.is_empty() {
            return Err(bad(
                line,
                label,
                value,
                "ticket lines before the first section",
            ));
        }
        self.current = Current::Nothing;
        let once = |taken: bool| {
            if taken {
                Err(bad(line, label, value, "the line once"))
            } else {
                Ok(())
            }
        };
        match label {
            NODE => {
                once(self.node.is_some())?;
                self.node = Some(node_id(line, label, value)?);
            }
            KIND => {
                once(self.kind.is_some())?;
                if value != "interface" {
                    return Err(bad(line, label, value, "interface"));
                }
                self.kind = Some(NodeKind::Interface);
            }
            SIGNATURES => {
                once(self.signatures.is_some())?;
                if !value.is_empty() {
                    return Err(bad(
                        line,
                        label,
                        value,
                        "nothing: the items follow, one a line",
                    ));
                }
                self.signatures = Some((line, Vec::new()));
                self.current = Current::Signatures;
            }
            TURN => self.turns.push(turn(line, value)?),
            _ => self.waits.push(wait(line, value)?),
        }
        Ok(())
    }

    fn behaviour(&mut self, line: usize, value: &str) -> Result<(), TaskTemplateError> {
        if !is_word(value, "-_.@:") {
            return Err(bad(line, BEHAVIOUR, value, KEY_ACCEPTS));
        }
        if self.sections.last().is_some_and(|last| last.key.is_none()) {
            return Err(TaskTemplateError::NoBehaviourLine { line });
        }
        let key = NodeKey::from(value);
        if self.sections.iter().any(|s| s.key.as_ref() == Some(&key)) {
            return Err(bad(line, BEHAVIOUR, value, "a key once"));
        }
        self.sections.push(SectionScan {
            line,
            key: Some(key),
            ..SectionScan::default()
        });
        self.current = Current::Nothing;
        Ok(())
    }

    /// The section a field or a list belongs to: the last one, or a first one with no key.
    fn section(&mut self, line: usize) -> &mut SectionScan {
        if self.sections.is_empty() {
            self.sections.push(SectionScan {
                line,
                ..SectionScan::default()
            });
        }
        let last = self.sections.len() - 1;
        &mut self.sections[last]
    }

    fn list(&mut self, line: usize, label: &str, value: &str) -> Result<(), TaskTemplateError> {
        self.current = Current::Nothing;
        let section = self.section(line);
        let taken = match label {
            WRITES => section.writes.is_some(),
            PROVIDES => section.provides.is_some(),
            IMPLEMENTS => section.implements.is_some(),
            _ => section.needs.is_some(),
        };
        if taken {
            return Err(if section.key.is_none() {
                TaskTemplateError::NoBehaviourLine { line }
            } else {
                bad(line, label, value, "the line once in a section")
            });
        }
        match label {
            WRITES => section.writes = Some(areas(line, label, value)?),
            PROVIDES => section.provides = Some(names(line, label, value)?),
            IMPLEMENTS => section.implements = Some(names(line, label, value)?),
            _ => section.needs = Some(names(line, label, value)?),
        }
        Ok(())
    }

    fn field(&mut self, line: usize, label: &str, value: &str) -> Result<(), TaskTemplateError> {
        let field = label
            .parse::<TaskField>()
            .map_err(|_| TaskTemplateError::UnknownField {
                line,
                label: label.to_owned(),
            })?;
        let section = self.section(line);
        if section.values.contains_key(&field) {
            return Err(if section.key.is_none() {
                TaskTemplateError::NoBehaviourLine { line }
            } else {
                TaskTemplateError::DuplicateField(field)
            });
        }
        section.values.insert(field, value.to_owned());
        self.current = Current::Field(field);
        Ok(())
    }

    fn finish(self) -> Result<PlannedTicket, TaskTemplateError> {
        let node = self.node.ok_or(TaskTemplateError::NotPlanned)?;
        let kind = self.kind.unwrap_or_default();
        let (signatures_line, signatures) = self.signatures.unwrap_or_default();
        if kind != NodeKind::Interface && !signatures.is_empty() {
            return Err(bad(
                signatures_line,
                SIGNATURES,
                "",
                "items only on a ticket with `Kind: interface`",
            ));
        }
        match self.sections.as_slice() {
            [] => return Err(TaskTemplateError::NoSection),
            [only] => {
                if let Some(key) = &only.key {
                    return Err(bad(
                        only.line,
                        BEHAVIOUR,
                        key.as_str(),
                        "no line on a ticket of one behaviour",
                    ));
                }
            }
            _ => {}
        }
        let sections = self
            .sections
            .into_iter()
            .map(SectionScan::finish)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PlannedTicket {
            node,
            kind,
            signatures,
            turns: self.turns,
            waits: self.waits,
            sections,
        })
    }
}

/// What a line's refusal comes to as a problem; `content` is the line as written.
fn problem_of(error: TaskTemplateError, content: &str) -> TaskProblem {
    let line_of = |line: usize, label: &str, accepts: &str| TaskProblem::BadLine {
        line,
        label: label.to_owned(),
        value: String::new(),
        accepts: accepts.to_owned(),
    };
    match error {
        TaskTemplateError::BadPlannedLine {
            line,
            label,
            value,
            accepts,
        } => TaskProblem::BadLine {
            line,
            label,
            value,
            accepts,
        },
        TaskTemplateError::UnknownField { line, label } => TaskProblem::Unknown { line, label },
        TaskTemplateError::DuplicateField(field) => TaskProblem::Bad {
            field,
            value: String::new(),
            accepts: "one line".to_owned(),
        },
        TaskTemplateError::BlockedByInPlannedBody { line } => {
            line_of(line, BLOCKED_BY, "no line: the graph holds the waits")
        }
        TaskTemplateError::NoBehaviourLine { line } => line_of(
            line,
            BEHAVIOUR,
            "a Behaviour line opening each section of a ticket of several",
        ),
        TaskTemplateError::UnrecognizedLine { line, text } => TaskProblem::Misplaced { line, text },
        TaskTemplateError::StrayListItem { line } => TaskProblem::Misplaced {
            line,
            text: content.to_owned(),
        },
        other => TaskProblem::Misplaced {
            line: 0,
            text: other.to_string(),
        },
    }
}

impl PlannedTicket {
    /// Every problem a planned body has, in line order and then in template order for each
    /// section; none when it parses as a [`PlannedTicket`].
    ///
    /// A section of a ticket of several names its `Behaviour` key on each of its problems.
    pub fn problems(text: &str) -> Vec<TaskProblem> {
        let mut scan = Scan::new();
        let mut problems = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            if let Err(error) = scan.line(index + 1, raw) {
                problems.push(problem_of(error, raw.trim()));
                scan.current = Current::Reported;
            }
        }
        if let Some((line, signatures)) = &scan.signatures
            && scan.kind != Some(NodeKind::Interface)
            && !signatures.is_empty()
        {
            problems.push(TaskProblem::BadLine {
                line: *line,
                label: SIGNATURES.to_owned(),
                value: String::new(),
                accepts: "items only on a ticket with `Kind: interface`".to_owned(),
            });
        }
        if let [only] = scan.sections.as_slice()
            && let Some(key) = &only.key
        {
            problems.push(TaskProblem::BadLine {
                line: only.line,
                label: BEHAVIOUR.to_owned(),
                value: key.as_str().to_owned(),
                accepts: "no line on a ticket of one behaviour".to_owned(),
            });
        }
        if scan.sections.is_empty() {
            problems.extend(TaskTemplate::field_problems(&BTreeMap::new(), 0, false));
        }
        for section in &scan.sections {
            let found =
                TaskTemplate::field_problems(&section.values, section.acceptance.len(), false);
            problems.extend(found.into_iter().map(|problem| match &section.key {
                Some(key) if scan.sections.len() > 1 => TaskProblem::InSection {
                    key: key.as_str().to_owned(),
                    problem: Box::new(problem),
                },
                _ => problem,
            }));
        }
        problems
    }
}

impl FromStr for PlannedTicket {
    type Err = TaskTemplateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut scan = Scan::new();
        for (index, raw) in text.lines().enumerate() {
            scan.line(index + 1, raw)?;
        }
        scan.finish()
    }
}

#[cfg(test)]
mod tests;
