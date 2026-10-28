//! The task template: the body of every task ticket, one `Field: value` per line.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

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

/// The body of a task ticket.
///
/// Free-text fields stay text: their meaning is the prose. Fields with a
/// vocabulary (security trigger, model, size, blockers) are typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTemplate {
    /// One sentence, the user-visible or system-visible outcome.
    pub goal: String,
    /// The files or folders this task may change; nothing else.
    pub owned_paths: String,
    /// New or changed ports, types and routes; who defines, who consumes.
    pub interfaces: String,
    /// The design pages the task builds from.
    pub design_pages: String,
    /// The module id and the interface ids this task defines or consumes.
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
        let owned_paths = take_required(&mut values, TaskField::OwnedPaths)?;
        let interfaces = take_required(&mut values, TaskField::Interfaces)?;
        let design_pages = take_required(&mut values, TaskField::DesignPages)?;
        let architecture = take_required(&mut values, TaskField::Architecture)?;
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
        write_field(formatter, TaskField::OwnedPaths, &self.owned_paths)?;
        write_field(formatter, TaskField::Interfaces, &self.interfaces)?;
        write_field(formatter, TaskField::DesignPages, &self.design_pages)?;
        write_field(formatter, TaskField::Architecture, &self.architecture)?;
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
    if value.is_empty() {
        writeln!(formatter, "{field}:")
    } else {
        writeln!(formatter, "{field}: {value}")
    }
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
