//! The step syntax of `[check.<target>] steps` and the step kinds, one module each.
//!
//! A step is `ci`, `command:<cmd>`, `runner:<name>:<role>`, `bot:<login>` or
//! `codeql`, with an optional `?flagged` suffix that runs it only for a
//! security-flagged task.

mod bot;
mod ci;
mod codeql;
mod command;
mod runner;

pub use runner::{BRIEF_FILE, CRITIQUE_BRIEF, SECURITY_BRIEF};

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

use super::StepPorts;
use crate::domain::{Finding, FindingSource, RoleName};
use crate::manifest::{CommandLine, RunnerName};
use crate::ports::{Author, PortError, PrNumber};

/// The suffix that runs a step only for a security-flagged task.
pub const FLAGGED_SUFFIX: &str = "?flagged";

/// A runner step: run a role on a declared runner and read its report.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunnerStep {
    /// The runner, as `[runners.<name>]` declares it.
    pub runner: RunnerName,
    /// The role it runs; a role naming `security` gets the security brief.
    pub role: RoleName,
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StepKind {
    /// Wait on the forge's checks; each failed check is a must-fix.
    Ci,
    /// Run a command line in the project; its exit code and report lines count.
    Command(CommandLine),
    /// Run a review role on a runner; its report lines count.
    Runner(RunnerStep),
    /// Wait for a review bot's review, then read its comments as findings.
    Bot(Author),
    /// Read the forge's code scanning alerts for the head; each is a must-fix.
    Codeql,
}

/// One configured step.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Step {
    /// What it does.
    pub kind: StepKind,
    /// Whether it runs only for a security-flagged task.
    pub flagged_only: bool,
}

/// A step entry outside the step syntax.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    /// An empty entry.
    #[error("an empty check step")]
    Empty,
    /// A kind this build does not run.
    #[error(
        "check step `{0}`: unknown kind (known: ci, command:<cmd>, runner:<name>:<role>, bot:<login>, codeql)"
    )]
    UnknownKind(String),
    /// A known kind missing its parts.
    #[error("check step `{0}`: expected `command:<cmd>`, `runner:<name>:<role>` or `bot:<login>`")]
    MissingPart(String),
}

impl Step {
    /// The source its findings carry: the step as written, without `?flagged`.
    pub fn source(&self) -> FindingSource {
        FindingSource::from(self.kind.to_string())
    }
}

impl fmt::Display for StepKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ci => formatter.write_str("ci"),
            Self::Command(line) => write!(formatter, "command:{line}"),
            Self::Runner(step) => write!(formatter, "runner:{}:{}", step.runner, step.role),
            Self::Bot(login) => write!(formatter, "bot:{login}"),
            Self::Codeql => formatter.write_str("codeql"),
        }
    }
}

impl fmt::Display for Step {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suffix = if self.flagged_only {
            FLAGGED_SUFFIX
        } else {
            ""
        };
        write!(formatter, "{}{suffix}", self.kind)
    }
}

impl FromStr for Step {
    type Err = StepError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let (body, flagged_only) = match trimmed.strip_suffix(FLAGGED_SUFFIX) {
            Some(body) => (body.trim_end(), true),
            None => (trimmed, false),
        };
        if body.is_empty() {
            return Err(StepError::Empty);
        }
        let missing_part_error = || StepError::MissingPart(text.to_owned());
        let kind = match body.split_once(':') {
            None if body == "ci" => StepKind::Ci,
            None if body == "codeql" => StepKind::Codeql,
            Some(("bot", login)) => {
                let login = login.trim();
                if login.is_empty() || login.contains(':') {
                    return Err(missing_part_error());
                }
                StepKind::Bot(Author::from(login))
            }
            Some(("command", line)) => {
                let line = line.trim();
                if line.is_empty() {
                    return Err(missing_part_error());
                }
                StepKind::Command(CommandLine::from(line))
            }
            Some(("runner", rest)) => {
                let (runner, role) = rest.split_once(':').ok_or_else(missing_part_error)?;
                let (runner, role) = (runner.trim(), role.trim());
                if runner.is_empty() || role.is_empty() || role.contains(':') {
                    return Err(missing_part_error());
                }
                let runner_step = RunnerStep {
                    runner: RunnerName::from(runner),
                    role: RoleName::from(role),
                };
                StepKind::Runner(runner_step)
            }
            _ => return Err(StepError::UnknownKind(text.to_owned())),
        };
        Ok(Self { kind, flagged_only })
    }
}

/// What a step needs to run, beside the ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepContext {
    /// The pull request under check.
    pub pr: PrNumber,
    /// The project root.
    pub root: PathBuf,
    /// A directory of the step's own, for a runner's brief and report.
    pub dir: PathBuf,
    /// When the step must give up.
    pub deadline: Instant,
    /// How often `ci` looks at the forge's checks.
    pub poll: Duration,
}

/// Runs one step to its findings; an error is a failed step.
pub fn run(
    step: &Step,
    ports: &dyn StepPorts,
    context: &StepContext,
) -> Result<Vec<Finding>, PortError> {
    let source = step.source();
    match &step.kind {
        StepKind::Ci => ci::run(&source, ports, context),
        StepKind::Command(line) => command::run(&source, line, ports),
        StepKind::Runner(runner_step) => runner::run(&source, runner_step, ports, context),
        StepKind::Bot(login) => bot::run(&source, login, ports, context),
        StepKind::Codeql => codeql::run(&source, ports, context),
    }
}

#[cfg(test)]
mod tests;
