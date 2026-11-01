//! What `zaf check` is pointed at: a task by its issue number, or a pull request.

use std::fmt;
use std::str::FromStr;

use crate::domain::IssueNumber;
use crate::ports::PrNumber;

/// A check target as typed: `26` or `#26` for a task; `pr:12` or `pr:12:26` for
/// pull request 12, the second form naming the task it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// A task: its acceptance checks.
    Task(IssueNumber),
    /// A pull request, and the task it carries when given.
    Pr {
        /// The pull request.
        pr: PrNumber,
        /// The task, whose security trigger decides the `?flagged` steps.
        task: Option<IssueNumber>,
    },
}

/// A target that is neither a task number nor `pr:<n>[:<task>]`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a check target: give a task (`26`) or a pull request (`pr:12`, `pr:12:26`)")]
pub struct TargetError(String);

impl Target {
    /// A file-name-safe name for the target, for the stored result.
    pub fn slug(&self) -> String {
        match self {
            Self::Task(task) => format!("task-{}", task.get()),
            Self::Pr { pr, .. } => format!("pr-{}", pr.to_string().trim_start_matches('#')),
        }
    }
}

impl FromStr for Target {
    type Err = TargetError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let target_error = || TargetError(text.to_owned());
        let trimmed = text.trim();
        let Some(rest) = trimmed.strip_prefix("pr:") else {
            return trimmed
                .parse::<IssueNumber>()
                .map(Self::Task)
                .map_err(|_| target_error());
        };
        let (pr_text, task_text) = match rest.split_once(':') {
            Some((pr_text, task_text)) => (pr_text, Some(task_text)),
            None => (rest, None),
        };
        let pr = pr_text
            .parse::<IssueNumber>()
            .map(|number| PrNumber::from(number.get()))
            .map_err(|_| target_error())?;
        let task = task_text
            .map(str::parse::<IssueNumber>)
            .transpose()
            .map_err(|_| target_error())?;
        Ok(Self::Pr { pr, task })
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Task(task) => write!(formatter, "task {task}"),
            Self::Pr { pr, task: None } => write!(formatter, "pr {pr}"),
            Self::Pr {
                pr,
                task: Some(task),
            } => write!(formatter, "pr {pr} for task {task}"),
        }
    }
}

#[cfg(test)]
mod tests;
