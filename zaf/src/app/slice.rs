//! A task's slice, resolved: its name and base from the board and the manifest's branch patterns.
//!
//! The slice's own work (open, pull request, merge, drop) is the project's `slice-*` actions;
//! `zaf changes` and the slice environment of an action read the target from here.

use crate::domain::{Ticket, TicketId};
use crate::manifest::{BranchPattern, BranchesSection};
use crate::ports::{BaseName, Board, PortError, SliceName};

/// The placeholder for the task's bare number in a branch pattern.
const TASK_PLACEHOLDER: &str = "{task}";
/// The placeholder for the epic's bare number in a branch pattern.
const EPIC_PLACEHOLDER: &str = "{epic}";
/// The placeholder for the release version in a branch pattern.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The step a port call failed in, so the caller can say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Step {
    /// Reading the task's ticket.
    Ticket,
    /// Reading the slice's change.
    Diff,
}

/// Why resolving a task's slice, or reading its change, stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SliceError {
    /// The task pattern names `{epic}` and the ticket belongs to no epic.
    #[error("{task} belongs to no epic, and the branch pattern names `{{epic}}`")]
    NoEpic {
        /// The task.
        task: TicketId,
    },
    /// A branch pattern names `{version}` and the call gave none.
    #[error("the branch pattern names `{{version}}`, and no version was given")]
    NoVersion,
    /// A port call failed in a step.
    #[error("{step}: {error}")]
    Port {
        /// The step it failed in.
        step: Step,
        /// The port's error.
        #[source]
        error: PortError,
    },
}

impl SliceError {
    /// The step the verb stopped at.
    pub fn step(&self) -> Option<Step> {
        match self {
            Self::NoEpic { .. } | Self::NoVersion => None,
            Self::Port { step, .. } => Some(*step),
        }
    }
}

/// Tags a port error with the step it failed in.
fn at(step: Step) -> impl FnOnce(PortError) -> SliceError {
    move |error| SliceError::Port { step, error }
}

/// The query every slice verb starts from: the task, and the release version its branches name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceQuery {
    /// The task.
    pub task: TicketId,
    /// The release version, for patterns that name `{version}`.
    pub version: Option<String>,
}

/// A task's slice, resolved: its ticket, its slice name and the base it merges into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceTarget {
    /// The task's ticket as the board holds it.
    pub ticket: Ticket,
    /// The slice: the task branch pattern, filled.
    pub slice: SliceName,
    /// The base: the integration branch pattern, filled.
    pub base: BaseName,
}

/// Resolves a task's slice from its ticket and the manifest's branch patterns.
pub fn resolve(
    board: &dyn Board,
    branches: &BranchesSection,
    query: &SliceQuery,
) -> Result<SliceTarget, SliceError> {
    let ticket = board.task(query.task).map_err(at(Step::Ticket))?;
    let slice = fill(&branches.task, &ticket, query.version.as_deref())?;
    let base = fill(&branches.integration, &ticket, query.version.as_deref())?;
    let target = SliceTarget {
        ticket,
        slice: SliceName::from(slice),
        base: BaseName::from(base),
    };
    Ok(target)
}

/// Fills a branch pattern's placeholders; a placeholder without a value is an error.
fn fill(
    pattern: &BranchPattern,
    ticket: &Ticket,
    version: Option<&str>,
) -> Result<String, SliceError> {
    let mut name = pattern.as_ref().replace(TASK_PLACEHOLDER, &bare(ticket.id));
    if name.contains(EPIC_PLACEHOLDER) {
        let epic = ticket.epic.ok_or(SliceError::NoEpic { task: ticket.id })?;
        name = name.replace(EPIC_PLACEHOLDER, &bare(epic));
    }
    if name.contains(VERSION_PLACEHOLDER) {
        let version = version.ok_or(SliceError::NoVersion)?;
        name = name.replace(VERSION_PLACEHOLDER, version);
    }
    Ok(name)
}

/// An issue number without its `#`, as a branch name carries it.
fn bare(number: TicketId) -> String {
    number.get().to_string()
}

#[cfg(test)]
mod tests;
