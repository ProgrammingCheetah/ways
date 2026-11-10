//! The task template: the body of every task ticket, one `Field: value` per line.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use strum::VariantArray;

use super::IssueNumber;

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
    /// `Size`: `S` or `M`.
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

/// How big a task is; anything larger is split before it reaches the board.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::VariantArray,
)]
pub enum TaskSize {
    /// Small.
    S,
    /// Medium.
    M,
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
}

impl TaskProblem {
    /// What the problem is about: the field's label, or `line <n>`.
    pub fn name(&self) -> String {
        match self {
            Self::Bad { field, .. } | Self::Missing(field) => field.to_string(),
            Self::MissingScope => format!("{} or {}", TaskField::Scope, TaskField::OwnedPaths),
            Self::Unknown { line, .. } | Self::Misplaced { line, .. } => format!("line {line}"),
        }
    }

    /// What was written, when something was.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Bad { value, .. } => Some(value),
            Self::Unknown { label, .. } => Some(label),
            Self::Misplaced { text, .. } => Some(text),
            Self::Missing(_) | Self::MissingScope => None,
        }
    }

    /// What would have been accepted, when that is a short list of forms.
    pub fn accepts(&self) -> Option<String> {
        match self {
            Self::Bad { accepts, .. } => Some(accepts.clone()),
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
                write!(
                    formatter,
                    "{field}: {} (accepts: {accepts})",
                    written.join(" ")
                )
            }
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
            &values,
            TaskField::SecurityTrigger,
            "no | yes <which>",
            |value| value.parse::<SecurityTrigger>().is_ok(),
        );
        check_form(
            &mut problems,
            &values,
            TaskField::Model,
            &forms::<TaskModel>(),
            |value| value.parse::<TaskModel>().is_ok(),
        );
        check_form(
            &mut problems,
            &values,
            TaskField::Size,
            &forms::<TaskSize>(),
            |value| value.parse::<TaskSize>().is_ok(),
        );
        check_form(
            &mut problems,
            &values,
            TaskField::BlockedBy,
            BLOCKERS_ACCEPTS,
            |value| parse_blockers(value).is_ok(),
        );
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
    pub blocked_by: Vec<IssueNumber>,
    /// The style rules to follow, when the ticket names them.
    pub style: Option<String>,
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
        let blocked_by = parse_blockers(&take_required(&mut values, TaskField::BlockedBy)?)?;
        let style = values.remove(&TaskField::Style);

        let template = Self {
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
        };
        Ok(template)
    }
}

impl fmt::Display for TaskTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        let blockers = if self.blocked_by.is_empty() {
            NO_BLOCKERS.to_owned()
        } else {
            let numbers: Vec<String> = self.blocked_by.iter().map(ToString::to_string).collect();
            numbers.join(", ")
        };
        write_field(formatter, TaskField::BlockedBy, &blockers)?;
        if let Some(style) = &self.style {
            write_field(formatter, TaskField::Style, style)?;
        }
        Ok(())
    }
}

fn write_field(formatter: &mut fmt::Formatter<'_>, field: TaskField, value: &str) -> fmt::Result {
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

fn parse_vocabulary<T: FromStr>(
    values: &mut BTreeMap<TaskField, String>,
    field: TaskField,
) -> Result<T, TaskTemplateError> {
    let value = take_required(values, field)?;
    value
        .parse::<T>()
        .map_err(|_| TaskTemplateError::InvalidValue { field, value })
}

fn parse_blockers(value: &str) -> Result<Vec<IssueNumber>, TaskTemplateError> {
    if matches!(value, "" | NO_BLOCKERS | "-" | "none") {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|number| {
            number
                .trim()
                .parse::<IssueNumber>()
                .map_err(|_| TaskTemplateError::InvalidValue {
                    field: TaskField::BlockedBy,
                    value: value.to_owned(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests;
