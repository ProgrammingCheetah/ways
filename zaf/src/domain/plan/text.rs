//! The plan as a planner writes it (I1), read into the plan's input.
//!
//! A plan is the task template the planner already writes, one section per task, each opened by
//! `Task: <title>` (D2). It may open with two lists:
//!
//! ```text
//! New names:
//! - area app/update
//! - signature mcp.catalog: fn catalog() -> Vec<Verb>
//! - no signature prompt.fix
//! Drop: #41, #42
//!
//! Task: Update takes --yes
//! Node: A
//! Group: update
//! Type: Task
//! Goal: update takes --yes
//! Scope: the update verb
//! Out of scope: the summary line (#2)
//! Interfaces: none
//! Design pages: D2
//! Acceptance:
//! - `update --yes` skips the prompt
//! Tests: update_yes
//! Security trigger: no
//! Model: sonnet
//! Size: S
//! Writes: app/update
//! Provides: update.yes
//! Needs: cli
//! ```
//!
//! A section is one node of one behaviour: its `Goal` is the behaviour's text, its `Size` its
//! weight (`XS` and `S` are 1, `M` is 2), and `Writes`, `Provides` and `Needs` its lists, each
//! comma-separated or one `- item` a line. Nobody writes an edge, so `Blocked by` is refused.
//! `Needs` may also hold `#<n>`, a filed task of the release by its number (D4, D13).
//!
//! The plan is untrusted text from a model. [`parse_plan`] bounds what it reads (the limits are
//! the constants below), never indexes into text and never panics, and a plan that is wrong is
//! refused with every fault at once, each naming its section and its line. It checks the shape of
//! a name, not whether the name is known: that is the vocabulary's (#1183).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::{Area, Behaviour, NewNames, Node, NodeId, NodeKind, PlanInput, Size, Weight};
use crate::domain::TicketId;
use crate::domain::task::{SecurityTrigger, TaskField, TaskModel, TaskSize};

/// The most bytes of plan read; a longer plan is refused unread.
pub const MAX_PLAN_BYTES: usize = 256 * 1024;
/// The most lines read.
pub const MAX_LINES: usize = 5_000;
/// The longest line, in bytes; a longer one is refused and skipped.
pub const MAX_LINE_BYTES: usize = 2_000;
/// The most sections (tasks) in a plan.
pub const MAX_SECTIONS: usize = 200;
/// The most items in one list.
pub const MAX_ITEMS: usize = 200;
/// The most faults listed; the rest are counted by one last fault.
pub const MAX_FAULTS: usize = 50;
/// The longest node key, name, area, group or type.
const MAX_NAME_BYTES: usize = 128;
/// The longest title.
const MAX_TITLE_CHARS: usize = 200;
/// How much of a written value a fault repeats.
const SHOWN_CHARS: usize = 60;

/// What a plan holds once it is read: the input the optimiser takes, and what filing needs that
/// the input has no place for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPlan {
    /// The nodes, one per section in the order written, and the names the plan adds
    /// (`new_names`, set when the plan has a `New names:` list). Every other key is its default.
    pub input: PlanInput,
    /// Each section as written, in the order of `input.nodes`.
    pub sections: Vec<PlanSection>,
    /// The shape of each name the plan gives a signature.
    pub signatures: BTreeMap<super::Name, String>,
    /// The filed tickets a re-plan drops (D29).
    pub drop: Vec<TicketId>,
}

/// One section, kept for filing: what the node's title, group, type and prose fields say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSection {
    /// The node this section is.
    pub node: NodeId,
    /// The line after `Task:`: the ticket's title.
    pub title: String,
    /// The `Group` line, if any.
    pub group: Option<String>,
    /// The `Type` line, if any: the ticket's issue type.
    pub kind: Option<String>,
    /// The template's prose fields as written (every field but `Acceptance`, `Blocked by` and
    /// the plan's own), a continuation joined by a line break.
    pub fields: BTreeMap<TaskField, String>,
    /// The `Acceptance` items.
    pub acceptance: Vec<String>,
}

/// The section a fault is in: its number from 1 and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRef {
    /// Its place among the sections, from 1.
    pub number: usize,
    /// Its title; empty when the `Task:` line gave none.
    pub title: String,
}

impl fmt::Display for SectionRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.title.is_empty() {
            write!(formatter, "section {}", self.number)
        } else {
            write!(formatter, "section {} \"{}\"", self.number, self.title)
        }
    }
}

/// What is wrong at a place in a plan.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FaultKind {
    /// The bytes are not UTF-8.
    #[error("the plan is not UTF-8 text")]
    NotText,
    /// The plan is over the byte limit.
    #[error("the plan is {bytes} bytes; at most {limit} are read")]
    TooLarge {
        /// The size given.
        bytes: usize,
        /// The limit.
        limit: usize,
    },
    /// The plan is over the line limit.
    #[error("the plan has more than {limit} lines")]
    TooManyLines {
        /// The limit.
        limit: usize,
    },
    /// A line is over the length limit.
    #[error("the line is over {limit} bytes")]
    LineTooLong {
        /// The limit.
        limit: usize,
    },
    /// The plan is over the section limit.
    #[error("more than {limit} sections")]
    TooManySections {
        /// The limit.
        limit: usize,
    },
    /// A list is over the item limit.
    #[error("`{field}` has more than {limit} items")]
    TooManyItems {
        /// The list.
        field: String,
        /// The limit.
        limit: usize,
    },
    /// The plan has no section.
    #[error("the plan has no `Task:` section")]
    NoSection,
    /// A line that is no field, continuation or item where it stands.
    #[error("`{text}` is not a field, a continuation or an item here")]
    Misplaced {
        /// The line, shortened.
        text: String,
    },
    /// A `- item` under a field that has no list.
    #[error("a list item outside a list")]
    StrayItem,
    /// A `Label:` that is neither the template's nor the plan's.
    #[error("unknown field `{label}`")]
    UnknownField {
        /// The label as written.
        label: String,
    },
    /// A `Blocked by` line: a plan names no edge.
    #[error(
        "`Blocked by` has no place in a plan: nobody writes an edge, zaf derives the waits from `Provides` and `Needs`"
    )]
    BlockedBy,
    /// A list or field that belongs before the first task.
    #[error("`{label}` belongs before the first `Task:`")]
    OutOfPlace {
        /// The label.
        label: String,
    },
    /// A field given twice.
    #[error("`{field}` appears twice")]
    DuplicateField {
        /// The label.
        field: String,
    },
    /// A required field left out.
    #[error("`{field}` is missing")]
    Missing {
        /// The label, or the pair `Scope`/`Owned paths`.
        field: String,
    },
    /// A required field with nothing in it.
    #[error("`{field}` is empty")]
    Empty {
        /// The label.
        field: String,
    },
    /// A node key that is not letters, digits, `-` and `_`.
    #[error("`{value}` is not a node key: use letters, digits, `-` and `_`")]
    BadNode {
        /// The value as written.
        value: String,
    },
    /// A name that is not lowercase letters, digits, `.`, `-`, `/` and `_`.
    #[error(
        "`{value}` is not a name in `{field}`: use lowercase letters, digits, `.`, `-`, `/` and `_`"
    )]
    BadName {
        /// The list.
        field: String,
        /// The value as written.
        value: String,
    },
    /// An area of the wrong shape.
    #[error(
        "`{value}` is not an area in `{field}`: use lowercase letters, digits, `.`, `-`, `/` and `_`"
    )]
    BadArea {
        /// The list.
        field: String,
        /// The value as written.
        value: String,
    },
    /// A field whose value is outside what it accepts.
    #[error("`{field}` is `{value}`; it accepts {accepts}")]
    BadValue {
        /// The label.
        field: String,
        /// The value as written.
        value: String,
        /// The forms it accepts.
        accepts: String,
    },
    /// A node id two sections give.
    #[error("`Node: {id}` is also the node of the section at line {first}")]
    DuplicateNode {
        /// The id.
        id: String,
        /// The line of the first.
        first: usize,
    },
    /// An item of `New names:` that is none of the three forms.
    #[error("`{value}` is not `area <name>`, `signature <name>: <shape>` or `no signature <name>`")]
    BadNewName {
        /// The item as written.
        value: String,
    },
    /// A name the plan declares twice.
    #[error("`{name}` is declared twice in `New names:`")]
    DeclaredTwice {
        /// The name.
        name: String,
    },
    /// An item of `Drop:` that is not an issue number.
    #[error("`{value}` is not an issue number such as `#12`")]
    BadDrop {
        /// The item as written.
        value: String,
    },
    /// More faults than are listed.
    #[error("{more} more faults are not listed")]
    Truncated {
        /// How many.
        more: usize,
    },
}

/// One fault, at a line, in a section when it is in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFault {
    /// The section, or `None` for a fault outside any.
    pub section: Option<SectionRef>,
    /// The 1-based line; 0 when the fault is of the whole plan.
    pub line: usize,
    /// What is wrong.
    pub kind: FaultKind,
}

impl fmt::Display for PlanFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.section, self.line) {
            (Some(section), 0) => write!(formatter, "{section}: {}", self.kind),
            (Some(section), line) => write!(formatter, "{section}, line {line}: {}", self.kind),
            (None, 0) => write!(formatter, "{}", self.kind),
            (None, line) => write!(formatter, "line {line}: {}", self.kind),
        }
    }
}

/// A plan refused: every fault found, in line order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFaults(pub Vec<PlanFault>);

impl fmt::Display for PlanFaults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, fault) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "{fault}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlanFaults {}

/// Reads a plan from bytes: text that is not UTF-8 is refused, the rest as [`parse_plan`].
pub fn parse_plan_bytes(bytes: &[u8]) -> Result<ParsedPlan, PlanFaults> {
    if bytes.len() > MAX_PLAN_BYTES {
        return Err(whole(FaultKind::TooLarge {
            bytes: bytes.len(),
            limit: MAX_PLAN_BYTES,
        }));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => parse_plan(text),
        Err(_) => Err(whole(FaultKind::NotText)),
    }
}

fn whole(kind: FaultKind) -> PlanFaults {
    PlanFaults(vec![PlanFault {
        section: None,
        line: 0,
        kind,
    }])
}

/// Reads a plan as a planner writes it into the plan's input, or refuses it with every fault.
pub fn parse_plan(text: &str) -> Result<ParsedPlan, PlanFaults> {
    if text.len() > MAX_PLAN_BYTES {
        return Err(whole(FaultKind::TooLarge {
            bytes: text.len(),
            limit: MAX_PLAN_BYTES,
        }));
    }
    let mut scan = Scan::default();
    for (index, raw) in text.lines().enumerate() {
        if index >= MAX_LINES {
            scan.faults.push(
                None,
                index + 1,
                FaultKind::TooManyLines { limit: MAX_LINES },
            );
            return Err(scan.faults.finish());
        }
        scan.line(index + 1, raw);
        if scan.stopped {
            return Err(scan.faults.finish());
        }
    }
    scan.close();
    scan.finish()
}

/// A line of a section that holds a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Node,
    Group,
    Type,
    Writes,
    Provides,
    Needs,
    Template(TaskField),
}

impl Slot {
    fn of(label: &str) -> Option<Self> {
        Some(match label {
            "Node" => Self::Node,
            "Group" => Self::Group,
            "Type" => Self::Type,
            "Writes" => Self::Writes,
            "Provides" => Self::Provides,
            "Needs" => Self::Needs,
            other => Self::Template(other.parse::<TaskField>().ok()?),
        })
    }

    fn label(self) -> String {
        match self {
            Self::Node => "Node".to_owned(),
            Self::Group => "Group".to_owned(),
            Self::Type => "Type".to_owned(),
            Self::Writes => "Writes".to_owned(),
            Self::Provides => "Provides".to_owned(),
            Self::Needs => "Needs".to_owned(),
            Self::Template(field) => field.to_string(),
        }
    }

    fn is_list(self) -> bool {
        matches!(self, Self::Writes | Self::Provides | Self::Needs)
    }

    /// Whether a continuation line may follow it.
    fn continues(self) -> bool {
        matches!(self, Self::Template(_)) || self.is_list()
    }
}

/// Where the line scan stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    Nothing,
    NewNames,
    Drop,
    Field(Slot),
    /// In a field already faulted: its lines are not faulted again.
    Reported,
}

#[derive(Default)]
struct Faults {
    list: Vec<PlanFault>,
    more: usize,
}

impl Faults {
    fn push(&mut self, section: Option<&SectionRef>, line: usize, kind: FaultKind) {
        if self.list.len() >= MAX_FAULTS {
            self.more += 1;
            return;
        }
        self.list.push(PlanFault {
            section: section.cloned(),
            line,
            kind,
        });
    }

    fn finish(mut self) -> PlanFaults {
        // A stable sort: faults of one line keep the order they were found in.
        self.list.sort_by_key(|fault| fault.line);
        if self.more > 0 {
            self.list.push(PlanFault {
                section: None,
                line: 0,
                kind: FaultKind::Truncated { more: self.more },
            });
        }
        PlanFaults(self.list)
    }
}

/// A section being read.
struct Draft {
    at: SectionRef,
    line: usize,
    values: BTreeMap<Slot, (usize, String)>,
    writes: BTreeSet<Area>,
    provides: BTreeSet<super::Name>,
    needs: BTreeSet<super::Name>,
    acceptance: Vec<String>,
    over: BTreeSet<Slot>,
}

#[derive(Default)]
struct Scan {
    faults: Faults,
    reading: Option<Reading>,
    draft: Option<Draft>,
    started: bool,
    stopped: bool,
    new_names: Option<NewNames>,
    declared: BTreeSet<String>,
    signatures: BTreeMap<super::Name, String>,
    seen_new_names: bool,
    new_count: usize,
    drop_over: bool,
    seen_drop: bool,
    drop: Vec<TicketId>,
    sections: Vec<PlanSection>,
    nodes: Vec<Node>,
    node_lines: BTreeMap<String, usize>,
    number: usize,
}

impl Scan {
    fn reading(&self) -> Reading {
        self.reading.unwrap_or(Reading::Nothing)
    }

    fn fault(&mut self, line: usize, kind: FaultKind) {
        let at = self.draft.as_ref().map(|draft| draft.at.clone());
        self.faults.push(at.as_ref(), line, kind);
    }

    fn line(&mut self, line: usize, raw: &str) {
        let content = raw.trim();
        if content.is_empty() {
            return;
        }
        if raw.len() > MAX_LINE_BYTES {
            self.fault(
                line,
                FaultKind::LineTooLong {
                    limit: MAX_LINE_BYTES,
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        }
        if let Some(item) = content.strip_prefix("- ") {
            self.item(line, item.trim());
        } else if raw.starts_with([' ', '\t']) {
            self.continuation(line, content);
        } else if let Some((label, value)) = content.split_once(':') {
            self.labelled(line, label.trim(), value.trim(), content);
        } else {
            self.fault(
                line,
                FaultKind::Misplaced {
                    text: shorten(content),
                },
            );
        }
    }

    fn item(&mut self, line: usize, item: &str) {
        match self.reading() {
            Reading::NewNames => self.new_name(line, item),
            Reading::Drop => self.drop_items(line, item),
            Reading::Field(Slot::Template(TaskField::Acceptance)) => {
                if let Some(draft) = &mut self.draft {
                    if draft.acceptance.len() < MAX_ITEMS {
                        draft.acceptance.push(item.to_owned());
                    } else if draft.over.insert(Slot::Template(TaskField::Acceptance)) {
                        self.fault(
                            line,
                            FaultKind::TooManyItems {
                                field: "Acceptance".to_owned(),
                                limit: MAX_ITEMS,
                            },
                        );
                    }
                }
            }
            Reading::Field(slot) if slot.is_list() => self.names(slot, line, item),
            Reading::Reported => {}
            _ => self.fault(line, FaultKind::StrayItem),
        }
    }

    fn continuation(&mut self, line: usize, content: &str) {
        match self.reading() {
            Reading::Field(slot) if slot.is_list() => self.names(slot, line, content),
            Reading::Field(slot) if slot.continues() => {
                if let Some(value) = self
                    .draft
                    .as_mut()
                    .and_then(|draft| draft.values.get_mut(&slot))
                {
                    if !value.1.is_empty() {
                        value.1.push('\n');
                    }
                    value.1.push_str(content);
                }
            }
            Reading::Reported => {}
            _ => self.fault(
                line,
                FaultKind::Misplaced {
                    text: shorten(content),
                },
            ),
        }
    }

    fn labelled(&mut self, line: usize, label: &str, value: &str, content: &str) {
        match label {
            "Task" => self.open(line, value),
            "New names" | "Drop" => self.header_list(line, label, value),
            "Blocked by" => {
                self.fault(line, FaultKind::BlockedBy);
                self.reading = Some(Reading::Reported);
            }
            _ => self.field(line, label, value, content),
        }
    }

    fn header_list(&mut self, line: usize, label: &str, value: &str) {
        if self.started {
            self.fault(
                line,
                FaultKind::OutOfPlace {
                    label: label.to_owned(),
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        }
        let drop = label == "Drop";
        let seen = if drop {
            &mut self.seen_drop
        } else {
            &mut self.seen_new_names
        };
        if std::mem::replace(seen, true) {
            self.fault(
                line,
                FaultKind::DuplicateField {
                    field: label.to_owned(),
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        }
        if drop {
            self.reading = Some(Reading::Drop);
            if !is_none(value) {
                self.drop_items(line, value);
            }
        } else {
            self.reading = Some(Reading::NewNames);
            self.new_names.get_or_insert_with(NewNames::default);
            if !value.is_empty() {
                self.fault(
                    line,
                    FaultKind::Misplaced {
                        text: shorten(value),
                    },
                );
            }
        }
    }

    fn new_name(&mut self, line: usize, item: &str) {
        let bad = || FaultKind::BadNewName {
            value: shorten(item),
        };
        if self.new_count >= 4 * MAX_ITEMS {
            if self.new_count == 4 * MAX_ITEMS {
                self.new_count += 1;
                self.fault(
                    line,
                    FaultKind::TooManyItems {
                        field: "New names".to_owned(),
                        limit: 4 * MAX_ITEMS,
                    },
                );
            }
            return;
        }
        self.new_count += 1;
        let names = self.new_names.get_or_insert_with(NewNames::default);
        if let Some(rest) = item.strip_prefix("area ") {
            let rest = rest.trim();
            if is_name(rest) {
                names.areas.insert(Area::from(rest));
            } else {
                self.fault(
                    line,
                    FaultKind::BadArea {
                        field: "New names".to_owned(),
                        value: shorten(rest),
                    },
                );
            }
        } else if let Some(rest) = item.strip_prefix("no signature ") {
            let rest = rest.trim();
            if !is_name(rest) {
                self.fault(
                    line,
                    FaultKind::BadName {
                        field: "New names".to_owned(),
                        value: shorten(rest),
                    },
                );
            } else if self.declared.insert(rest.to_owned()) {
                if let Some(names) = &mut self.new_names {
                    names.no_signature.insert(rest.into());
                }
            } else {
                self.fault(
                    line,
                    FaultKind::DeclaredTwice {
                        name: rest.to_owned(),
                    },
                );
            }
        } else if let Some(rest) = item.strip_prefix("signature ") {
            let Some((name, shape)) = rest.split_once(':') else {
                self.fault(line, bad());
                return;
            };
            let (name, shape) = (name.trim(), shape.trim());
            if !is_name(name) {
                self.fault(
                    line,
                    FaultKind::BadName {
                        field: "New names".to_owned(),
                        value: shorten(name),
                    },
                );
            } else if shape.is_empty() {
                self.fault(line, bad());
            } else if self.declared.insert(name.to_owned()) {
                if let Some(names) = &mut self.new_names {
                    names.signature.insert(name.into());
                }
                self.signatures.insert(name.into(), shape.to_owned());
            } else {
                self.fault(
                    line,
                    FaultKind::DeclaredTwice {
                        name: name.to_owned(),
                    },
                );
            }
        } else {
            self.fault(line, bad());
        }
    }

    fn drop_items(&mut self, line: usize, text: &str) {
        for piece in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let ticket = piece
                .strip_prefix('#')
                .and_then(|digits| TicketId::from_str(digits).ok());
            match ticket {
                Some(ticket) if self.drop.len() < MAX_ITEMS => self.drop.push(ticket),
                Some(_) => {
                    if !std::mem::replace(&mut self.drop_over, true) {
                        self.fault(
                            line,
                            FaultKind::TooManyItems {
                                field: "Drop".to_owned(),
                                limit: MAX_ITEMS,
                            },
                        );
                    }
                }
                None => self.fault(
                    line,
                    FaultKind::BadDrop {
                        value: shorten(piece),
                    },
                ),
            }
        }
    }

    fn open(&mut self, line: usize, title: &str) {
        self.close();
        self.started = true;
        self.number += 1;
        if self.number > MAX_SECTIONS {
            self.faults.push(
                None,
                line,
                FaultKind::TooManySections {
                    limit: MAX_SECTIONS,
                },
            );
            self.stopped = true;
            return;
        }
        let at = SectionRef {
            number: self.number,
            title: shorten_to(title, MAX_TITLE_CHARS),
        };
        if title.is_empty() {
            self.faults.push(
                Some(&at),
                line,
                FaultKind::Empty {
                    field: "Task".to_owned(),
                },
            );
        }
        self.draft = Some(Draft {
            at,
            line,
            values: BTreeMap::new(),
            writes: BTreeSet::new(),
            provides: BTreeSet::new(),
            needs: BTreeSet::new(),
            acceptance: Vec::new(),
            over: BTreeSet::new(),
        });
        self.reading = Some(Reading::Nothing);
    }

    fn field(&mut self, line: usize, label: &str, value: &str, content: &str) {
        if self.draft.is_none() {
            self.fault(
                line,
                FaultKind::Misplaced {
                    text: shorten(content),
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        }
        let Some(slot) = Slot::of(label) else {
            self.fault(
                line,
                FaultKind::UnknownField {
                    label: shorten(label),
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        };
        let duplicate = self.draft.as_ref().is_some_and(|draft| {
            draft.values.contains_key(&slot)
                || (slot == Slot::Template(TaskField::Acceptance) && !draft.acceptance.is_empty())
        });
        if duplicate {
            self.fault(
                line,
                FaultKind::DuplicateField {
                    field: slot.label(),
                },
            );
            self.reading = Some(Reading::Reported);
            return;
        }
        if let Some(draft) = &mut self.draft {
            let kept = if slot.is_list() { "" } else { value };
            draft.values.insert(slot, (line, kept.to_owned()));
        }
        self.reading = Some(Reading::Field(slot));
        if slot.is_list() && !is_none(value) {
            self.names(slot, line, value);
        }
    }

    /// Adds the comma-separated names of a list line.
    fn names(&mut self, slot: Slot, line: usize, text: &str) {
        let field = slot.label();
        for piece in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let Some(draft) = &mut self.draft else { return };
            let size = match slot {
                Slot::Writes => draft.writes.len(),
                Slot::Provides => draft.provides.len(),
                _ => draft.needs.len(),
            };
            if size >= MAX_ITEMS {
                if draft.over.insert(slot) {
                    self.fault(
                        line,
                        FaultKind::TooManyItems {
                            field,
                            limit: MAX_ITEMS,
                        },
                    );
                }
                return;
            }
            let item = match slot {
                Slot::Needs => Need::parse(piece).map(Item::Need),
                _ => is_name(piece).then_some(Item::Name),
            };
            let Some(item) = item else {
                let value = shorten(piece);
                let kind = if slot == Slot::Writes {
                    FaultKind::BadArea {
                        field: field.clone(),
                        value,
                    }
                } else {
                    FaultKind::BadName {
                        field: field.clone(),
                        value,
                    }
                };
                self.fault(line, kind);
                continue;
            };
            match (slot, item) {
                (_, Item::Need(need)) => draft.needs.insert(need.into_name()),
                (Slot::Writes, Item::Name) => draft.writes.insert(Area::from(piece)),
                (_, Item::Name) => draft.provides.insert(piece.into()),
            };
        }
    }

    /// Ends the section being read: checks its fields and keeps it.
    fn close(&mut self) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let at = draft.at.clone();
        let mut faults = Vec::new();
        let mut fault = |line: usize, kind: FaultKind| faults.push((line, kind));
        let text = |slot: Slot| {
            draft
                .values
                .get(&slot)
                .map(|(line, value)| (*line, value.as_str()))
        };
        let template = |field: TaskField| text(Slot::Template(field));

        let node = match text(Slot::Node) {
            None => {
                fault(
                    draft.line,
                    FaultKind::Missing {
                        field: "Node".to_owned(),
                    },
                );
                None
            }
            Some((line, key)) if !is_key(key) => {
                fault(
                    line,
                    FaultKind::BadNode {
                        value: shorten(key),
                    },
                );
                None
            }
            Some((line, key)) => {
                if let Some(first) = self.node_lines.get(key) {
                    fault(
                        line,
                        FaultKind::DuplicateNode {
                            id: key.to_owned(),
                            first: *first,
                        },
                    );
                    None
                } else {
                    self.node_lines.insert(key.to_owned(), line);
                    Some(NodeId::from(key))
                }
            }
        };
        let mut word = |slot: Slot| match text(slot) {
            Some((_, "")) => {
                fault(
                    draft.values[&slot].0,
                    FaultKind::Empty {
                        field: slot.label(),
                    },
                );
                None
            }
            Some((line, value)) if !is_word(value) => {
                fault(
                    line,
                    FaultKind::BadValue {
                        field: slot.label(),
                        value: shorten(value),
                        accepts: format!("one word of up to {MAX_NAME_BYTES} bytes"),
                    },
                );
                None
            }
            Some((_, value)) => Some(value.to_owned()),
            None => None,
        };
        let group = word(Slot::Group);
        let kind = word(Slot::Type);

        for field in [
            TaskField::Goal,
            TaskField::Interfaces,
            TaskField::DesignPages,
            TaskField::Tests,
        ] {
            match template(field) {
                None => fault(
                    draft.line,
                    FaultKind::Missing {
                        field: field.to_string(),
                    },
                ),
                Some((line, "")) => fault(
                    line,
                    FaultKind::Empty {
                        field: field.to_string(),
                    },
                ),
                Some(_) => {}
            }
        }
        match (
            template(TaskField::Scope),
            template(TaskField::OwnedPaths),
            template(TaskField::OutOfScope),
        ) {
            (None, None, _) => fault(
                draft.line,
                FaultKind::Missing {
                    field: "Scope or Owned paths".to_owned(),
                },
            ),
            (Some(_), _, None) => fault(
                draft.line,
                FaultKind::Missing {
                    field: "Out of scope".to_owned(),
                },
            ),
            _ => {}
        }
        match template(TaskField::Acceptance) {
            None => fault(
                draft.line,
                FaultKind::Missing {
                    field: "Acceptance".to_owned(),
                },
            ),
            Some((line, inline)) if !inline.is_empty() => fault(
                line,
                FaultKind::BadValue {
                    field: "Acceptance".to_owned(),
                    value: shorten(inline),
                    accepts: "- item, one per line below".to_owned(),
                },
            ),
            Some((line, _)) if draft.acceptance.is_empty() => fault(
                line,
                FaultKind::Empty {
                    field: "Acceptance".to_owned(),
                },
            ),
            Some(_) => {}
        }
        let mut typed = |field: TaskField, accepts: &str, parses: &dyn Fn(&str) -> bool| {
            match template(field) {
                None => fault(
                    draft.line,
                    FaultKind::Missing {
                        field: field.to_string(),
                    },
                ),
                Some((line, value)) if !parses(value) => fault(
                    line,
                    FaultKind::BadValue {
                        field: field.to_string(),
                        value: shorten(value),
                        accepts: accepts.to_owned(),
                    },
                ),
                Some(_) => {}
            }
        };
        typed(TaskField::SecurityTrigger, "no | yes <which>", &|value| {
            value.parse::<SecurityTrigger>().is_ok()
        });
        typed(TaskField::Model, "sonnet | opus", &|value| {
            value.parse::<TaskModel>().is_ok()
        });
        typed(TaskField::Size, "XS | S | M", &|value| {
            weight_of(value).is_some()
        });

        let bad = !faults.is_empty();
        for (line, kind) in faults {
            self.faults.push(Some(&at), line, kind);
        }
        if bad {
            return;
        }
        let Some(node) = node else { return };
        let size = template(TaskField::Size)
            .and_then(|(_, value)| weight_of(value))
            .unwrap_or(Weight(1));
        let goal = template(TaskField::Goal)
            .map(|(_, value)| value.to_owned())
            .unwrap_or_default();
        self.nodes.push(Node {
            id: node.clone(),
            kind: NodeKind::Body,
            filed: None,
            turns: Vec::new(),
            blocked_by: Vec::new(),
            epic: None,
            behaviours: vec![Behaviour {
                text: goal,
                size: Size::Weight(size),
                writes: draft.writes.clone(),
                provides: draft.provides.clone(),
                needs: draft.needs.clone(),
            }],
        });
        let fields = draft
            .values
            .iter()
            .filter_map(|(slot, (_, value))| match slot {
                Slot::Template(TaskField::Acceptance) => None,
                Slot::Template(field) => Some((*field, value.clone())),
                _ => None,
            })
            .collect();
        self.sections.push(PlanSection {
            node,
            title: at.title,
            group,
            kind,
            fields,
            acceptance: draft.acceptance.clone(),
        });
    }

    fn finish(mut self) -> Result<ParsedPlan, PlanFaults> {
        if self.number == 0 {
            self.faults.push(None, 0, FaultKind::NoSection);
        }
        if !self.faults.list.is_empty() || self.faults.more > 0 {
            return Err(self.faults.finish());
        }
        let mut input = PlanInput::of(self.nodes);
        input.new_names = self.new_names;
        Ok(ParsedPlan {
            input,
            sections: self.sections,
            signatures: self.signatures,
            drop: self.drop,
        })
    }
}

/// `Size: XS | S | M` as a weight: `XS` and `S` are 1, `M` is 2.
fn weight_of(value: &str) -> Option<Weight> {
    match value.parse::<TaskSize>().ok()? {
        TaskSize::XS | TaskSize::S => Some(Weight(1)),
        TaskSize::M => Some(Weight(2)),
        _ => None,
    }
}

/// A list written as nothing: `—`, `-` or `none`, or no text.
fn is_none(value: &str) -> bool {
    matches!(value.trim(), "" | "—" | "-" | "none")
}

/// A node key: letters, digits, `-` and `_`.
fn is_key(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NAME_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// A name or an area: lowercase letters, digits, `.`, `-`, `/` and `_`.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NAME_BYTES
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'-' | b'/' | b'_')
        })
}

/// One item of a `Needs` list: a name, or a filed task by its number (D4, D13).
enum Need {
    /// A name of the vocabulary or of the plan.
    Name(super::Name),
    /// A filed task, written `#<n>`.
    Task(TicketId),
}

impl Need {
    /// Reads an item: `#<n>` is a task and nothing else, anything without a `#` must be a name.
    fn parse(piece: &str) -> Option<Self> {
        if piece.starts_with('#') {
            return (piece.len() <= MAX_NAME_BYTES)
                .then(|| piece.parse::<TicketId>().ok())
                .flatten()
                .map(Self::Task);
        }
        is_name(piece).then(|| Self::Name(piece.into()))
    }

    /// The name the plan's input holds the need under: a task's is its number's (D4).
    fn into_name(self) -> super::Name {
        match self {
            Self::Name(name) => name,
            Self::Task(task) => super::Name::of_task(task),
        }
    }
}

/// What a list item was read as.
enum Item {
    /// A name or an area of the right shape, still the text of the item.
    Name,
    /// An item of `Needs`.
    Need(Need),
}

/// A group or a type: one line, no control character, not over the limit.
fn is_word(text: &str) -> bool {
    text.len() <= MAX_NAME_BYTES && !text.chars().any(char::is_control)
}

fn shorten(text: &str) -> String {
    shorten_to(text, SHOWN_CHARS)
}

/// The first `limit` characters of the text, with `…` when it was longer.
fn shorten_to(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_cuts_on_a_character() {
        assert_eq!(shorten_to("ééé", 2), "éé…");
        assert_eq!(shorten_to("ab", 2), "ab");
    }
}
