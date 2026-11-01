//! The task kind: what is checkable about a task, line by line, with no model.
//!
//! The light gate runs; each test named on `Tests:` runs through the manifest's
//! `[gate] tests` template; each interface id on `Architecture:` is searched as a
//! symbol inside the owned paths; every `Acceptance:` line is listed as manual
//! for the reviewer. Names are the backticked spans of a field, or, when it has
//! none, its comma- or semicolon-separated items that are one identifier each;
//! any other item is prose and listed as manual.

mod symbols;

use std::path::Path;

use super::{Shell, Verdict};
use crate::domain::TaskTemplate;
use crate::manifest::CommandLine;
use crate::ports::{Gate, GateLevel, GateVerdict, PortError};

/// The placeholder a `[gate] tests` template carries for the test's name.
pub const NAME_PLACEHOLDER: &str = "{name}";

/// What a line checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum LineKind {
    /// The light gate.
    Gate,
    /// A test named on `Tests:`.
    Test,
    /// An interface id on `Architecture:`.
    Interface,
    /// An `Acceptance:` line.
    Acceptance,
}

/// How a line came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineStatus {
    /// Checked and holds.
    Pass,
    /// Checked and does not hold, and why.
    Fail(String),
    /// Prose: for the reviewer to judge.
    Manual,
    /// Not checkable here: the project declares nothing to check it with.
    Skipped(String),
}

/// One line of a task check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLine {
    /// What it checks.
    pub kind: LineKind,
    /// The test name, interface id or acceptance text.
    pub subject: String,
    /// How it came out.
    pub status: LineStatus,
}

/// A finished task check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCheck {
    /// Every line, gate first, then tests, interfaces and acceptance.
    pub lines: Vec<TaskLine>,
}

impl TaskCheck {
    /// Clean only when no line failed.
    pub fn verdict(&self) -> Verdict {
        let failed = self
            .lines
            .iter()
            .any(|line| matches!(line.status, LineStatus::Fail(_)));
        if failed {
            Verdict::MustFix
        } else {
            Verdict::Clean
        }
    }
}

/// What a task check runs with; `None` where the manifest declares nothing.
pub struct TaskInputs<'a> {
    /// The gate, when `[gate]` is declared.
    pub gate: Option<&'a dyn Gate>,
    /// The `[gate] tests` template, when declared.
    pub tests: Option<&'a CommandLine>,
    /// The shell that runs each named test.
    pub shell: &'a dyn Shell,
    /// The project root the owned paths are relative to.
    pub root: &'a Path,
}

/// Checks a task's template against the tree.
pub fn check_task(template: &TaskTemplate, inputs: &TaskInputs<'_>) -> TaskCheck {
    let mut lines = vec![gate_line(inputs.gate)];

    let (tests, test_prose) = names(&template.tests);
    for name in tests {
        let status = test_status(&name, inputs);
        lines.push(line(LineKind::Test, name, status));
    }
    lines.extend(manual(LineKind::Test, test_prose));

    let (interfaces, interface_prose) = names(&template.architecture);
    let owned = owned_paths(&template.owned_paths, inputs.root);
    for id in interfaces {
        let symbol = id.rsplit("::").next().unwrap_or(&id);
        let status = if owned.iter().any(|path| symbols::defines(path, symbol)) {
            LineStatus::Pass
        } else {
            LineStatus::Fail("not found in the owned paths".to_owned())
        };
        lines.push(line(LineKind::Interface, id.clone(), status));
    }
    lines.extend(manual(LineKind::Interface, interface_prose));

    lines.extend(manual(LineKind::Acceptance, template.acceptance.clone()));
    TaskCheck { lines }
}

fn line(kind: LineKind, subject: String, status: LineStatus) -> TaskLine {
    TaskLine {
        kind,
        subject,
        status,
    }
}

fn manual(kind: LineKind, subjects: Vec<String>) -> impl Iterator<Item = TaskLine> {
    subjects
        .into_iter()
        .map(move |subject| line(kind, subject, LineStatus::Manual))
}

fn gate_line(gate: Option<&dyn Gate>) -> TaskLine {
    let status = match gate.map(|gate| gate.run(GateLevel::Light)) {
        None => LineStatus::Skipped("no [gate] declared".to_owned()),
        Some(Ok(gate_run)) if gate_run.verdict == GateVerdict::Passed => LineStatus::Pass,
        Some(Ok(gate_run)) => LineStatus::Fail(last_line(&gate_run.output)),
        Some(Err(PortError::Unavailable(reason))) => LineStatus::Skipped(reason),
        Some(Err(error)) => LineStatus::Fail(error.to_string()),
    };
    line(LineKind::Gate, GateLevel::Light.to_string(), status)
}

fn test_status(name: &str, inputs: &TaskInputs<'_>) -> LineStatus {
    let Some(template) = inputs.tests else {
        return LineStatus::Skipped("no [gate] tests template declared".to_owned());
    };
    let command = CommandLine::from(template.as_ref().replace(NAME_PLACEHOLDER, name));
    match inputs.shell.run(&command) {
        Ok(gate_run) if gate_run.verdict == GateVerdict::Passed => LineStatus::Pass,
        Ok(gate_run) => LineStatus::Fail(last_line(&gate_run.output)),
        Err(error) => LineStatus::Fail(error.to_string()),
    }
}

fn last_line(output: &str) -> String {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("failed")
        .to_owned()
}

/// The names in a field, and its prose items.
fn names(field: &str) -> (Vec<String>, Vec<String>) {
    let field = field.trim();
    if matches!(field, "" | "—" | "-" | "none") {
        return (Vec::new(), Vec::new());
    }
    if field.contains('`') {
        let quoted = field
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        return (quoted, Vec::new());
    }
    let mut found = Vec::new();
    let mut prose = Vec::new();
    for item in field
        .split([',', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if is_identifier(item) {
            found.push(item.to_owned());
        } else {
            prose.push(item.to_owned());
        }
    }
    (found, prose)
}

fn is_identifier(text: &str) -> bool {
    text.chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':'))
}

/// The paths named in `Owned paths:` that exist under the root, globs cut at the first `*`.
fn owned_paths(field: &str, root: &Path) -> Vec<std::path::PathBuf> {
    field
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
        .map(|token| {
            token.trim_matches(|character| matches!(character, '`' | '(' | ')' | '"' | '\''))
        })
        .map(|token| {
            token
                .split('*')
                .next()
                .unwrap_or(token)
                .trim_end_matches('/')
        })
        .filter(|token| {
            token.contains(['/', '.']) && !token.starts_with('/') && !token.contains("..")
        })
        .map(|token| root.join(token))
        .filter(|path| path.exists())
        .collect()
}

#[cfg(test)]
mod tests;
