//! The slice environment of an action run: `ZAF_BASE` on any run, and `ZAF_SLICE`, `ZAF_PR` and
//! `ZAF_TASK_TITLE` on a `target: task` run (docs/zaf-actions.md §3 "Environment", rulings 20 and 23,
//! assumptions 3 and 10).
//!
//! These are the values the slice actions act on, so zaf sets them last and a caller can set none
//! of them: a value already in the caller's environment is replaced, and one zaf has no value for
//! is removed rather than left to whatever the caller put there. The slice and the title come from
//! the board and the branch patterns through [`crate::app::slice::resolve`], the pull request from
//! the record `slice-pr` left ([`SliceRecords`]). A board that cannot answer starts nothing.

use std::fmt;
use std::process::Command;

use super::records::SliceRecords;
use super::run::RunRequest;
use crate::app::slice::{SliceQuery, resolve};
use crate::domain::TicketId;
use crate::manifest::BranchesSection;
use crate::ports::{Board, Forge, Vcs};

/// The integration branch, from `ZAF_VERSION` and the branch pattern.
pub const BASE_VAR: &str = "ZAF_BASE";

/// The task's slice: the task branch pattern, filled.
pub const SLICE_VAR: &str = "ZAF_SLICE";

/// The task's pull request, from the record `slice-pr` left.
pub const PR_VAR: &str = "ZAF_PR";

/// The task's title, from the board.
pub const TASK_TITLE_VAR: &str = "ZAF_TASK_TITLE";

/// The variables only a `target: task` run has.
const TASK_VARS: [&str; 3] = [SLICE_VAR, PR_VAR, TASK_TITLE_VAR];

/// The placeholder for the release version in a branch pattern.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The placeholders only a ticket can fill.
const TICKET_PLACEHOLDERS: [&str; 2] = ["{task}", "{epic}"];

/// What a run needs to name a task's slice: the board, the branch patterns and the release.
#[derive(Clone)]
pub struct SliceContext<'a> {
    /// The board, or why there is none; a run that needs it and finds none starts nothing.
    pub board: Result<&'a dyn Board, String>,
    /// The forge, or why there is none; an action with `phases` that finds none starts nothing.
    pub forge: Result<&'a dyn Forge, String>,
    /// The version control, or why there is none; read like the forge.
    pub vcs: Result<&'a dyn Vcs, String>,
    /// The manifest's `[branches]`.
    pub branches: &'a BranchesSection,
    /// `ZAF_VERSION`: the release the branches build toward; `None` when unset or blank.
    pub version: Option<&'a str>,
}

impl fmt::Debug for SliceContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SliceContext")
            .field(
                "board",
                &self.board.as_ref().map(|_| "..").map_err(Clone::clone),
            )
            .field(
                "forge",
                &self.forge.as_ref().map(|_| "..").map_err(Clone::clone),
            )
            .field(
                "vcs",
                &self.vcs.as_ref().map(|_| "..").map_err(Clone::clone),
            )
            .field("branches", &self.branches)
            .field("version", &self.version)
            .finish()
    }
}

/// What a run sets last: each variable with its value, or `None` for one that is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SliceEnv(Vec<(&'static str, Option<String>)>);

impl SliceEnv {
    /// The variables for `request`. `target_task` says whether the action declares
    /// `target: task`; then its first argument names the task. An error is a body that must not
    /// start, in the words of its `NOT_RUN` line.
    pub(super) fn for_run(request: &RunRequest<'_>, target_task: bool) -> Result<Self, String> {
        let context = request.slice.as_ref();
        let mut vars = Vec::with_capacity(1 + TASK_VARS.len());
        if !target_task {
            vars.push((BASE_VAR, base_of(context)));
            vars.extend(TASK_VARS.map(|var| (var, None)));
            return Ok(Self(vars));
        }
        let task = request
            .args
            .first()
            .and_then(|task| task.to_str())
            .and_then(|task| task.parse::<TicketId>().ok())
            .ok_or("a `target: task` action needs the task number as its first argument")?;
        let context = context.ok_or("no project to name the task's slice in")?;
        let board = context
            .board
            .as_ref()
            .map_err(|reason| format!("cannot read {task} from the board: {reason}"))?;
        let query = SliceQuery {
            task,
            version: context.version.map(str::to_owned),
        };
        let target = resolve(*board, context.branches, &query)
            .map_err(|error| format!("cannot name the slice of {task}: {error}"))?;
        let pr = request
            .project_root
            .and_then(|root| SliceRecords::new(root).pr(task));
        vars.push((BASE_VAR, Some(target.base.as_ref().to_owned())));
        vars.push((SLICE_VAR, Some(target.slice.as_ref().to_owned())));
        vars.push((PR_VAR, pr.map(|pr| pr.get().to_string())));
        vars.push((TASK_TITLE_VAR, Some(target.ticket.title.to_string())));
        Ok(Self(vars))
    }

    /// Puts the variables on `command`. Call it after every other `env` call: a variable set
    /// here replaces the caller's, and a removed one is not inherited.
    pub(super) fn apply(&self, command: &mut Command) {
        for (name, value) in &self.0 {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
    }
}

/// The integration branch for a run that names no task, `None` when the project, the version or
/// the pattern leaves it unfilled.
fn base_of(context: Option<&SliceContext<'_>>) -> Option<String> {
    let context = context?;
    let pattern = context.branches.integration.as_ref();
    let base = if pattern.contains(VERSION_PLACEHOLDER) {
        pattern.replace(VERSION_PLACEHOLDER, context.version?)
    } else {
        pattern.to_owned()
    };
    // A pattern that names the task or epic needs a ticket, and this run has none.
    (!TICKET_PLACEHOLDERS.iter().any(|name| base.contains(name))).then_some(base)
}

#[cfg(test)]
mod tests;
