//! The task template: the body of every task ticket, one `Field: value` per line.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use strum::VariantArray;

use super::TicketId;
use super::plan::PlannedTicket;

/// A field of the task template, by the label it carries in the ticket.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::VariantArray,
)]
pub enum TaskField {
    /// `Goal`: one sentence, the visible outcome.
    #[strum(to_string = "Goal")]
    Goal,
    /// `Owned paths`: what the task may change.
    #[strum(to_string = "Owned paths")]
    OwnedPaths,
    /// `Scope`: the capability the task changes, in words; replaces `Owned paths`.
    #[strum(to_string = "Scope")]
    Scope,
    /// `Out of scope`: what the task leaves alone and the ticket that has it.
    #[strum(to_string = "Out of scope")]
    OutOfScope,
    /// `Interfaces`: ports, types and routes defined or consumed.
    #[strum(to_string = "Interfaces")]
    Interfaces,
    /// `Design pages`: the pages the task builds from.
    #[strum(to_string = "Design pages")]
    DesignPages,
    /// `Architecture`: module and interface ids.
    #[strum(to_string = "Architecture")]
    Architecture,
    /// `Acceptance`: a list of checkable statements.
    #[strum(to_string = "Acceptance")]
    Acceptance,
    /// `Tests`: the tests to add, by name.
    #[strum(to_string = "Tests")]
    Tests,
    /// `Security trigger`: `no`, or `yes` and which.
    #[strum(to_string = "Security trigger")]
    SecurityTrigger,
    /// `Model`: `sonnet` or `opus`.
    #[strum(to_string = "Model")]
    Model,
    /// `Size`: `XS`, `S`, `M`, `L`, `XL` or `E`.
    #[strum(to_string = "Size")]
    Size,
    /// `Blocked by`: issue numbers, or `—` for none.
    #[strum(to_string = "Blocked by")]
    BlockedBy,
    /// `Style`: the style rules to follow; optional.
    #[strum(to_string = "Style")]
    Style,
}

/// Which model builds a task.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum TaskModel {
    /// The default builder.
    Sonnet,
    /// For tasks that need the stronger model.
    Opus,
}

/// How big a task is, ordered from `XS` to `E`.
///
/// A planner files `XS`, `S` or `M`; `L`, `XL` and `E` are costs of the size table and of a
/// release launch, not ticket sizes. The name is the key of the size table and the value of
/// `ZAF_SIZE`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::VariantArray,
)]
pub enum TaskSize {
    /// Extra small.
    XS,
    /// Small.
    S,
    /// Medium.
    M,
    /// Large.
    L,
    /// Extra large.
    XL,
    /// The budget minus the reserve.
    E,
}

impl TaskSize {
    /// The size as written in a ticket: `XS`, `S`, `M`, `L`, `XL` or `E`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::XS => "XS",
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
            Self::XL => "XL",
            Self::E => "E",
        }
    }
}

/// Whether a task touches a security trigger, and which one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SecurityTrigger {
    /// No trigger.
    No,
    /// A trigger, named.
    Yes(String),
}

impl fmt::Display for SecurityTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::No => formatter.write_str("no"),
            Self::Yes(which) => write!(formatter, "yes {which}"),
        }
    }
}

impl FromStr for SecurityTrigger {
    type Err = TaskTemplateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid_trigger_error = TaskTemplateError::InvalidValue {
            field: TaskField::SecurityTrigger,
            value: text.to_owned(),
        };
        let trimmed = text.trim();
        if trimmed == "no" {
            return Ok(Self::No);
        }
        let which = trimmed
            .strip_prefix("yes ")
            .map(str::trim)
            .filter(|which| !which.is_empty())
            .ok_or(invalid_trigger_error)?;
        Ok(Self::Yes(which.to_owned()))
    }
}

/// A rejected task body, naming the line or field at fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskTemplateError {
    /// A `Label: value` line whose label is not a template field.
    #[error("line {line}: unknown field `{label}`")]
    UnknownField {
        /// The 1-based line number.
        line: usize,
        /// The label as written.
        label: String,
    },
    /// A line that is neither a field nor an acceptance item.
    #[error("line {line}: expected `Field: value` or an acceptance item, got `{text}`")]
    UnrecognizedLine {
        /// The 1-based line number.
        line: usize,
        /// The line as written.
        text: String,
    },
    /// A `- item` line that does not follow `Acceptance:`.
    #[error("line {line}: a list item outside Acceptance")]
    StrayListItem {
        /// The 1-based line number.
        line: usize,
    },
    /// A field given twice.
    #[error("`{0}` appears twice")]
    DuplicateField(TaskField),
    /// A required field left out.
    #[error("`{0}` is missing")]
    MissingField(TaskField),
    /// Neither `Scope` nor `Owned paths` is given.
    #[error("`Scope` (with `Out of scope`) or `Owned paths` is missing: give one of them")]
    MissingScope,
    /// A field that must carry something left empty.
    #[error("`{0}` is empty")]
    EmptyField(TaskField),
    /// A field whose value is outside its vocabulary.
    #[error("`{field}` has an invalid value `{value}`")]
    InvalidValue {
        /// The field.
        field: TaskField,
        /// The value as written.
        value: String,
    },
    /// A line of a planned body (`Node`, `Kind`, `Behaviour`, a list, a turn, a wait) whose value
    /// is outside what it accepts, or that stands where it may not.
    #[error("line {line}: `{label}` has an invalid value `{value}`: accepts {accepts}")]
    BadPlannedLine {
        /// The 1-based line number.
        line: usize,
        /// The label as written.
        label: String,
        /// The value as written.
        value: String,
        /// The forms it accepts.
        accepts: String,
    },
    /// A `Blocked by` line in a planned body: the graph holds the waits, so the body gives none.
    #[error("line {line}: `Blocked by` is not a field of a planned body")]
    BlockedByInPlannedBody {
        /// The 1-based line number.
        line: usize,
    },
    /// A planned body of two sections where one is not opened by a `Behaviour:` line.
    #[error(
        "line {line}: a section has no `Behaviour:` line, and a ticket of several has one each"
    )]
    NoBehaviourLine {
        /// The 1-based line number.
        line: usize,
    },
    /// A body with no `Node:` line read as a planned body: the older template reads it.
    #[error("no `Node:` line: not a planned body")]
    NotPlanned,
    /// A planned body with no section: it holds ticket lines and no field.
    #[error("a planned body holds no section")]
    NoSection,
    /// A planned ticket whose sections weigh more than a ticket may (D10): zaf files none, so it
    /// has no size to report.
    #[error("the sections weigh {weight}, more than the limit of {limit}: too large to size")]
    TooLarge {
        /// What the sections weigh: XS and S 1, M 2, anything larger counts past the limit.
        weight: u32,
        /// The most a ticket may weigh.
        limit: u32,
    },
    /// A typed field whose value is not one of the allowed values.
    #[error("`{field}` has an invalid value `{value}`: allowed values are {allowed}")]
    NotAllowed {
        /// The field.
        field: TaskField,
        /// The value as written.
        value: String,
        /// The allowed values, as written in a ticket, joined with `, `.
        allowed: String,
    },
}

/// What a line must look like when it is neither a field nor an acceptance item.
const LINE_ACCEPTS: &str = "Field: value | - item under Acceptance";

/// What `Blocked by` accepts: `-` and `none` also read as empty.
const BLOCKERS_ACCEPTS: &str = "— | #<issue>, #<issue>";

/// One thing wrong with a task body, as `zaf board lint` names it.
///
/// [`TaskTemplate::problems`] returns every one of them, where parsing stops at the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskProblem {
    /// A field whose value is outside what it accepts.
    Bad {
        /// The field.
        field: TaskField,
        /// The value as written.
        value: String,
        /// The forms the field accepts.
        accepts: String,
    },
    /// A field that is left out or left empty.
    Missing(TaskField),
    /// Neither `Scope` nor `Owned paths` is given.
    MissingScope,
    /// A `Label: value` line whose label is not a template field.
    Unknown {
        /// The 1-based line number.
        line: usize,
        /// The label as written.
        label: String,
    },
    /// A line that is neither a field, a continuation nor an acceptance item.
    Misplaced {
        /// The 1-based line number.
        line: usize,
        /// The line as written.
        text: String,
    },
    /// A line of a planned body (`Node`, `Kind`, `Behaviour`, a list, a turn, a wait) whose value
    /// is outside what its label accepts, or that stands where it may not.
    BadLine {
        /// The 1-based line number.
        line: usize,
        /// The label as written.
        label: String,
        /// The value as written; empty when the line has none to show.
        value: String,
        /// The forms the line accepts.
        accepts: String,
    },
    /// A problem in one section of a planned body of several, named by its `Behaviour` key.
    InSection {
        /// The section's `Behaviour` key.
        key: String,
        /// What is wrong in it.
        problem: Box<TaskProblem>,
    },
}

impl TaskProblem {
    /// What the problem is about: the field's label, or `line <n>`.
    pub fn name(&self) -> String {
        match self {
            Self::Bad { field, .. } | Self::Missing(field) => field.to_string(),
            Self::MissingScope => format!("{} or {}", TaskField::Scope, TaskField::OwnedPaths),
            Self::Unknown { line, .. }
            | Self::Misplaced { line, .. }
            | Self::BadLine { line, .. } => format!("line {line}"),
            Self::InSection { problem, .. } => problem.name(),
        }
    }

    /// What was written, when something was.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Bad { value, .. } => Some(value),
            Self::Unknown { label, .. } => Some(label),
            Self::Misplaced { text, .. } => Some(text),
            Self::BadLine { value, .. } => Some(value.as_str()).filter(|value| !value.is_empty()),
            Self::InSection { problem, .. } => problem.value(),
            Self::Missing(_) | Self::MissingScope => None,
        }
    }

    /// What would have been accepted, when that is a short list of forms.
    pub fn accepts(&self) -> Option<String> {
        match self {
            Self::Bad { accepts, .. } | Self::BadLine { accepts, .. } => Some(accepts.clone()),
            Self::InSection { problem, .. } => problem.accepts(),
            Self::Misplaced { .. } => Some(LINE_ACCEPTS.to_owned()),
            Self::Unknown { .. } => Some(forms::<TaskField>()),
            Self::Missing(_) | Self::MissingScope => None,
        }
    }
}

impl fmt::Display for TaskProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bad {
                field,
                value,
                accepts,
            } => {
                let written: Vec<&str> = value.lines().collect();
                if written.is_empty() {
                    write!(formatter, "{field} (accepts: {accepts})")
                } else {
                    write!(
                        formatter,
                        "{field}: {} (accepts: {accepts})",
                        written.join(" ")
                    )
                }
            }
            Self::BadLine {
                line,
                label,
                value,
                accepts,
            } if value.is_empty() => write!(formatter, "line {line}: {label} (accepts: {accepts})"),
            Self::BadLine {
                line,
                label,
                value,
                accepts,
            } => write!(
                formatter,
                "line {line}: {label}: {value} (accepts: {accepts})"
            ),
            Self::InSection { key, problem } => write!(formatter, "{problem} [Behaviour: {key}]"),
            Self::Missing(_) | Self::MissingScope => write!(formatter, "{}: missing", self.name()),
            Self::Unknown { line, label } => {
                write!(formatter, "line {line}: unknown field {label}")
            }
            Self::Misplaced { line, text } => {
                write!(formatter, "line {line}: {text} (accepts: {LINE_ACCEPTS})")
            }
        }
    }
}

/// Where the line scan of [`TaskTemplate::problems`] stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// Before any field.
    Nothing,
    /// Inside a field: its continuation lines and, under `Acceptance`, its items belong to it.
    Field(TaskField),
    /// Inside a field already reported: its lines are not reported again.
    Reported,
}

/// The forms a vocabulary field accepts, as written in a ticket.
fn forms<T: VariantArray + ToString + 'static>() -> String {
    let forms: Vec<String> = T::VARIANTS.iter().map(ToString::to_string).collect();
    forms.join(" | ")
}

/// A field given as text that must be one of a few forms, checked by `parses`.
fn check_form(
    problems: &mut Vec<TaskProblem>,
    values: &BTreeMap<TaskField, String>,
    field: TaskField,
    accepts: &str,
    parses: impl Fn(&str) -> bool,
) {
    match values.get(&field) {
        None => problems.push(TaskProblem::Missing(field)),
        Some(value) if !parses(value) => problems.push(TaskProblem::Bad {
            field,
            value: value.clone(),
            accepts: accepts.to_owned(),
        }),
        Some(_) => {}
    }
}

impl TaskTemplate {
    /// Every problem a task body has, in line order and then in template order; none when
    /// it parses as a [`TaskTemplate`].
    pub fn problems(text: &str) -> Vec<TaskProblem> {
        let mut problems = Vec::new();
        let mut values: BTreeMap<TaskField, String> = BTreeMap::new();
        let mut items = 0_usize;
        let mut reading = Reading::Nothing;

        for (index, raw_line) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw_line.trim();
            if content.is_empty() {
                continue;
            }
            let misplaced = TaskProblem::Misplaced {
                line,
                text: content.to_owned(),
            };
            if content.starts_with("- ") {
                match reading {
                    Reading::Field(TaskField::Acceptance) => items += 1,
                    Reading::Reported => {}
                    _ => problems.push(misplaced),
                }
            } else if raw_line.starts_with([' ', '\t']) {
                match reading {
                    Reading::Field(field) => {
                        if let Some(continued) = values.get_mut(&field) {
                            if !continued.is_empty() {
                                continued.push('\n');
                            }
                            continued.push_str(content);
                        }
                    }
                    Reading::Reported => {}
                    Reading::Nothing => problems.push(misplaced),
                }
            } else if let Some((label, value)) = content.split_once(':') {
                let label = label.trim();
                reading = match label.parse::<TaskField>() {
                    Err(_) => {
                        problems.push(TaskProblem::Unknown {
                            line,
                            label: label.to_owned(),
                        });
                        Reading::Reported
                    }
                    Ok(field) if values.contains_key(&field) => {
                        problems.push(TaskProblem::Bad {
                            field,
                            value: value.trim().to_owned(),
                            accepts: "one line".to_owned(),
                        });
                        Reading::Reported
                    }
                    Ok(field) => {
                        values.insert(field, value.trim().to_owned());
                        Reading::Field(field)
                    }
                };
            } else {
                problems.push(misplaced);
            }
        }

        problems.extend(Self::field_problems(&values, items, true));
        problems
    }

    /// The problems the fields a scan read have, in template order: what is left out or left
    /// bad, `items` being the acceptance items counted. `blocked_by` says whether the fields
    /// carry `Blocked by`, as an older body does and a section of a planned one does not.
    pub(super) fn field_problems(
        values: &BTreeMap<TaskField, String>,
        items: usize,
        blocked_by: bool,
    ) -> Vec<TaskProblem> {
        let mut problems = Vec::new();
        match values.get(&TaskField::Goal) {
            Some(goal) if !goal.is_empty() => {}
            _ => problems.push(TaskProblem::Missing(TaskField::Goal)),
        }
        match (
            values.contains_key(&TaskField::Scope),
            values.contains_key(&TaskField::OwnedPaths),
        ) {
            (true, _) if !values.contains_key(&TaskField::OutOfScope) => {
                problems.push(TaskProblem::Missing(TaskField::OutOfScope));
            }
            (false, false) => problems.push(TaskProblem::MissingScope),
            _ => {}
        }
        for field in [
            TaskField::Interfaces,
            TaskField::DesignPages,
            TaskField::Tests,
        ] {
            if !values.contains_key(&field) {
                problems.push(TaskProblem::Missing(field));
            }
        }
        match values.get(&TaskField::Acceptance) {
            None => problems.push(TaskProblem::Missing(TaskField::Acceptance)),
            Some(inline) if !inline.is_empty() => problems.push(TaskProblem::Bad {
                field: TaskField::Acceptance,
                value: inline.clone(),
                accepts: "- item, one per line below".to_owned(),
            }),
            Some(_) if items == 0 => problems.push(TaskProblem::Missing(TaskField::Acceptance)),
            Some(_) => {}
        }
        check_form(
            &mut problems,
            values,
            TaskField::SecurityTrigger,
            "no | yes <which>",
            |value| value.parse::<SecurityTrigger>().is_ok(),
        );
        check_form(
            &mut problems,
            values,
            TaskField::Model,
            &forms::<TaskModel>(),
            |value| value.parse::<TaskModel>().is_ok(),
        );
        check_form(
            &mut problems,
            values,
            TaskField::Size,
            &forms::<TaskSize>(),
            |value| value.parse::<TaskSize>().is_ok(),
        );
        if blocked_by {
            check_form(
                &mut problems,
                values,
                TaskField::BlockedBy,
                BLOCKERS_ACCEPTS,
                |value| parse_blockers(value).is_ok(),
            );
        }
        problems
    }
}

/// The body of a task ticket.
///
/// Free-text fields stay text: their meaning is the prose. Fields with a
/// vocabulary (security trigger, model, size, blockers) are typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTemplate {
    /// One sentence, the user-visible or system-visible outcome.
    pub goal: String,
    /// Where the task works: `Scope` when the ticket gives it, else `Owned paths`.
    pub owned_paths: String,
    /// Whether the ticket states its scope under `Scope` (with `Out of scope`), not `Owned paths`.
    pub scoped: bool,
    /// What the task leaves alone and which ticket has it; read from `Out of scope`.
    pub out_of_scope: Option<String>,
    /// New or changed ports, types and routes; who defines, who consumes.
    pub interfaces: String,
    /// The design pages the task builds from.
    pub design_pages: String,
    /// The module id and the interface ids this task defines or consumes; empty when the
    /// ticket has no `Architecture`.
    pub architecture: String,
    /// Checkable statements, one per line; at least one.
    pub acceptance: Vec<String>,
    /// The tests to add, by name.
    pub tests: String,
    /// Whether a security trigger applies.
    pub security_trigger: SecurityTrigger,
    /// Which model builds it.
    pub model: TaskModel,
    /// How big it is.
    pub size: TaskSize,
    /// The issues that must close first.
    pub blocked_by: Vec<TicketId>,
    /// The style rules to follow, when the ticket names them.
    pub style: Option<String>,
}

/// The body of any task ticket: a planned one (I7) or one of the older template.
///
/// A body with a `Node:` line is planned and one without is the older template. The accessors
/// give the same answer for either shape, so a caller never asks which it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskBody {
    /// A body filed from a plan: one section for each behaviour.
    Planned(PlannedTicket),
    /// A body of the older template: one behaviour.
    Template(TaskTemplate),
}

impl TaskBody {
    /// Reads a task ticket's body as the shape it claims: planned when it has a `Node:` line,
    /// else the older template.
    ///
    /// # Errors
    ///
    /// The error of the shape the body claimed: a malformed body with a `Node:` line is refused
    /// as a planned body, and one without as the older template.
    pub fn parse(text: &str) -> Result<Self, TaskTemplateError> {
        if PlannedTicket::is_planned(text) {
            text.parse().map(Self::Planned)
        } else {
            text.parse().map(Self::Template)
        }
    }

    /// Every problem a task body has, read as the shape it claims: a planned body against I2 and
    /// an older one against the template; none when [`Self::parse`] accepts it.
    pub fn problems(text: &str) -> Vec<TaskProblem> {
        if PlannedTicket::is_planned(text) {
            PlannedTicket::problems(text)
        } else {
            TaskTemplate::problems(text)
        }
    }

    /// The ticket's size: a planned body's is derived from its sections, an older body's is its
    /// `Size`.
    ///
    /// # Errors
    ///
    /// [`TaskTemplateError::TooLarge`] for a planned body whose sections weigh too much.
    pub fn size(&self) -> Result<TaskSize, TaskTemplateError> {
        match self {
            Self::Planned(planned) => planned.size(),
            Self::Template(template) => Ok(template.size),
        }
    }

    /// The ticket's model: a planned body's is derived from its sections, an older body's is its
    /// `Model`.
    pub fn model(&self) -> TaskModel {
        match self {
            Self::Planned(planned) => planned.model(),
            Self::Template(template) => template.model,
        }
    }

    /// The behaviours, in section order; one for an older body.
    pub fn behaviours(&self) -> Vec<&TaskTemplate> {
        match self {
            Self::Planned(planned) => planned
                .sections
                .iter()
                .map(|section| &section.template)
                .collect(),
            Self::Template(template) => vec![template],
        }
    }

    /// Every behaviour's acceptance lines, in section order.
    pub fn acceptance(&self) -> Vec<&str> {
        self.behaviours()
            .into_iter()
            .flat_map(|behaviour| behaviour.acceptance.iter().map(String::as_str))
            .collect()
    }

    /// Every behaviour's `Tests` field, in section order: the test names, as written.
    pub fn tests(&self) -> Vec<&str> {
        self.behaviours()
            .into_iter()
            .map(|behaviour| behaviour.tests.as_str())
            .collect()
    }

    /// Every behaviour's `Design pages` field, in section order, as written.
    pub fn design_pages(&self) -> Vec<&str> {
        self.behaviours()
            .into_iter()
            .map(|behaviour| behaviour.design_pages.as_str())
            .collect()
    }

    /// Every behaviour's owned paths (its `Scope` or `Owned paths`), in section order, as written.
    pub fn owned_paths(&self) -> Vec<&str> {
        self.behaviours()
            .into_iter()
            .map(|behaviour| behaviour.owned_paths.as_str())
            .collect()
    }

    /// Every behaviour's `Interfaces` field, in section order, as written.
    pub fn interfaces(&self) -> Vec<&str> {
        self.behaviours()
            .into_iter()
            .map(|behaviour| behaviour.interfaces.as_str())
            .collect()
    }
}

/// What renders for an empty `Blocked by`; `-` and `none` also read as empty.
const NO_BLOCKERS: &str = "—";

impl FromStr for TaskTemplate {
    type Err = TaskTemplateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut values: BTreeMap<TaskField, String> = BTreeMap::new();
        let mut acceptance: Vec<String> = Vec::new();
        let mut current_field: Option<TaskField> = None;

        for (index, raw_line) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw_line.trim();
            if content.is_empty() {
                continue;
            }
            if let Some(item) = content.strip_prefix("- ") {
                if current_field != Some(TaskField::Acceptance) {
                    return Err(TaskTemplateError::StrayListItem { line });
                }
                acceptance.push(item.trim().to_owned());
                continue;
            }
            if raw_line.starts_with([' ', '\t']) {
                let continued = current_field
                    .and_then(|field| values.get_mut(&field))
                    .ok_or_else(|| TaskTemplateError::UnrecognizedLine {
                        line,
                        text: content.to_owned(),
                    })?;
                if !continued.is_empty() {
                    continued.push('\n');
                }
                continued.push_str(content);
                continue;
            }
            let (label, value) =
                content
                    .split_once(':')
                    .ok_or_else(|| TaskTemplateError::UnrecognizedLine {
                        line,
                        text: content.to_owned(),
                    })?;
            let field =
                label
                    .trim()
                    .parse::<TaskField>()
                    .map_err(|_| TaskTemplateError::UnknownField {
                        line,
                        label: label.trim().to_owned(),
                    })?;
            let previous = values.insert(field, value.trim().to_owned());
            if previous.is_some() {
                return Err(TaskTemplateError::DuplicateField(field));
            }
            current_field = Some(field);
        }

        Self::from_fields(values, acceptance, Blockers::Required)
    }
}

/// Whether the fields of a template give `Blocked by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Blockers {
    /// A task of the older template: `Blocked by` is required.
    Required,
    /// A section of a planned body: the graph holds the waits, and `Blocked by` is not read.
    Refused,
}

impl TaskTemplate {
    /// The template of one section a plan wrote: its prose fields as the plan parser kept them
    /// (so with no `Acceptance` or `Blocked by` among them) and its acceptance items. `Blocked by`
    /// is never read, since the graph holds the waits.
    ///
    /// # Errors
    ///
    /// The [`TaskTemplateError`] of the field that is missing, empty or not allowed.
    pub fn from_plan_fields(
        mut values: BTreeMap<TaskField, String>,
        acceptance: Vec<String>,
    ) -> Result<Self, TaskTemplateError> {
        values.insert(TaskField::Acceptance, String::new());
        Self::from_fields(values, acceptance, Blockers::Refused)
    }

    /// The template from the fields a scan read: each field's text, and the acceptance items.
    pub(super) fn from_fields(
        mut values: BTreeMap<TaskField, String>,
        acceptance: Vec<String>,
        blockers: Blockers,
    ) -> Result<Self, TaskTemplateError> {
        let acceptance_inline = take_required(&mut values, TaskField::Acceptance)?;
        if !acceptance_inline.is_empty() {
            let inline_acceptance_error = TaskTemplateError::InvalidValue {
                field: TaskField::Acceptance,
                value: acceptance_inline,
            };
            return Err(inline_acceptance_error);
        }
        if acceptance.is_empty() {
            return Err(TaskTemplateError::EmptyField(TaskField::Acceptance));
        }

        let goal = take_required(&mut values, TaskField::Goal)?;
        if goal.is_empty() {
            return Err(TaskTemplateError::EmptyField(TaskField::Goal));
        }
        let scope = values.remove(&TaskField::Scope);
        let listed_paths = values.remove(&TaskField::OwnedPaths);
        let out_of_scope = values.remove(&TaskField::OutOfScope);
        let scoped = scope.is_some();
        let owned_paths = match (scope, listed_paths) {
            (Some(scope), _) => {
                if out_of_scope.is_none() {
                    return Err(TaskTemplateError::MissingField(TaskField::OutOfScope));
                }
                scope
            }
            (None, Some(listed)) => listed,
            (None, None) => return Err(TaskTemplateError::MissingScope),
        };
        let interfaces = take_required(&mut values, TaskField::Interfaces)?;
        let design_pages = take_required(&mut values, TaskField::DesignPages)?;
        let architecture = values.remove(&TaskField::Architecture).unwrap_or_default();
        let tests = take_required(&mut values, TaskField::Tests)?;
        let security_trigger =
            take_required(&mut values, TaskField::SecurityTrigger)?.parse::<SecurityTrigger>()?;
        let model = parse_vocabulary::<TaskModel>(&mut values, TaskField::Model)?;
        let size = parse_vocabulary::<TaskSize>(&mut values, TaskField::Size)?;
        let blocked_by = match blockers {
            Blockers::Required => {
                parse_blockers(&take_required(&mut values, TaskField::BlockedBy)?)?
            }
            Blockers::Refused => Vec::new(),
        };
        let style = values.remove(&TaskField::Style);

        Ok(Self {
            goal,
            owned_paths,
            scoped,
            out_of_scope,
            interfaces,
            design_pages,
            architecture,
            acceptance,
            tests,
            security_trigger,
            model,
            size,
            blocked_by,
            style,
        })
    }
}

impl fmt::Display for TaskTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_fields(formatter, true)
    }
}

impl TaskTemplate {
    /// The template's fields, one `Field: value` a line; `Blocked by` only when `blockers`.
    pub(super) fn write_fields(
        &self,
        formatter: &mut fmt::Formatter<'_>,
        blockers: bool,
    ) -> fmt::Result {
        write_field(formatter, TaskField::Goal, &self.goal)?;
        let paths_field = if self.scoped {
            TaskField::Scope
        } else {
            TaskField::OwnedPaths
        };
        write_field(formatter, paths_field, &self.owned_paths)?;
        if let Some(out_of_scope) = &self.out_of_scope {
            write_field(formatter, TaskField::OutOfScope, out_of_scope)?;
        }
        write_field(formatter, TaskField::Interfaces, &self.interfaces)?;
        write_field(formatter, TaskField::DesignPages, &self.design_pages)?;
        if !self.architecture.is_empty() {
            write_field(formatter, TaskField::Architecture, &self.architecture)?;
        }
        writeln!(formatter, "{}:", TaskField::Acceptance)?;
        for statement in &self.acceptance {
            writeln!(formatter, "  - {statement}")?;
        }
        write_field(formatter, TaskField::Tests, &self.tests)?;
        write_field(
            formatter,
            TaskField::SecurityTrigger,
            &self.security_trigger.to_string(),
        )?;
        write_field(formatter, TaskField::Model, &self.model.to_string())?;
        write_field(formatter, TaskField::Size, &self.size.to_string())?;
        if blockers {
            let written = if self.blocked_by.is_empty() {
                NO_BLOCKERS.to_owned()
            } else {
                let numbers: Vec<String> =
                    self.blocked_by.iter().map(ToString::to_string).collect();
                numbers.join(", ")
            };
            write_field(formatter, TaskField::BlockedBy, &written)?;
        }
        if let Some(style) = &self.style {
            write_field(formatter, TaskField::Style, style)?;
        }
        Ok(())
    }
}

pub(super) fn write_field(
    formatter: &mut fmt::Formatter<'_>,
    field: TaskField,
    value: &str,
) -> fmt::Result {
    let mut lines = value.lines();
    match lines.next() {
        Some(first) if !first.is_empty() => writeln!(formatter, "{field}: {first}")?,
        _ => writeln!(formatter, "{field}:")?,
    }
    for continuation in lines {
        writeln!(formatter, "  {continuation}")?;
    }
    Ok(())
}

fn take_required(
    values: &mut BTreeMap<TaskField, String>,
    field: TaskField,
) -> Result<String, TaskTemplateError> {
    values
        .remove(&field)
        .ok_or(TaskTemplateError::MissingField(field))
}

fn parse_vocabulary<T: FromStr + fmt::Display + VariantArray>(
    values: &mut BTreeMap<TaskField, String>,
    field: TaskField,
) -> Result<T, TaskTemplateError> {
    let value = take_required(values, field)?;
    value.parse::<T>().map_err(|_| {
        let allowed: Vec<String> = T::VARIANTS.iter().map(ToString::to_string).collect();
        TaskTemplateError::NotAllowed {
            field,
            value,
            allowed: allowed.join(", "),
        }
    })
}

fn parse_blockers(value: &str) -> Result<Vec<TicketId>, TaskTemplateError> {
    if matches!(value, "" | NO_BLOCKERS | "-" | "none") {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|number| {
            number
                .trim()
                .parse::<TicketId>()
                .map_err(|_| TaskTemplateError::InvalidValue {
                    field: TaskField::BlockedBy,
                    value: value.to_owned(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests;
