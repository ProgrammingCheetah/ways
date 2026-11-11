//! Step 5 of the tick: a failed wait with no `on-fail` raises a question on its task
//! (docs/zaf-actions.md §5.1, the #425 ruling).
//!
//! A task whose standing is `Failed` at a wait that declares no `on-fail` has nowhere to go, so the
//! tick asks the Engineer: one question on the task, `<action> failed at <head>: <what failed>; it
//! declares no on-fail.`, where what failed is the failed checks, or the wait run's exit code. The
//! question makes the task wait (the board labels it `question`). A task that already has an open
//! question gets no second one, whatever that question says, so a second tick raises none.
//!
//! Asking starts nothing and charges nothing: no run folder opens, no process starts, and the budget,
//! which is derived from the process table on every read, is as it was. A failed wait that does
//! declare an `on-fail` is #442's, and the answer to the question is the Engineer's.
//!
//! The module is pure but for the board it is handed: the caller gives each task's phase list and
//! facts, as for [`inline`](super::inline).

use std::collections::BTreeSet;

use super::inline::TickTask;
use super::outcome::TickOutcome;
use crate::app::actions::{ActionName, Standing, StandingFacts, derive_standing, wait_failure};
use crate::app::board::{self, BoardError};
use crate::domain::TicketId;
use crate::ports::{Board, NewQuestion};

/// The question a failed wait with no `on-fail` raises, in this form:
///
/// `<action> failed at <head>: <what failed>; it declares no on-fail.`
///
/// What failed is `checks <names>` when checks failed at the head, else `exit code <n>` of the
/// wait's run; both when both. A wait the forge gave no head for reads `no head`.
pub fn question_text(action: &ActionName, facts: &StandingFacts) -> String {
    let failure = wait_failure(action, facts);
    let head = failure.head.as_deref().unwrap_or("no head");
    let mut what = Vec::new();
    if !failure.checks.is_empty() {
        what.push(format!("failed checks {}", failure.checks.join(", ")));
    }
    if let Some(code) = failure.exit {
        what.push(format!("exit code {code}"));
    }
    if what.is_empty() {
        what.push("a failure with no check or exit code named".to_owned());
    }
    format!(
        "{action} failed at {head}: {}; it declares no on-fail.",
        what.join(", ")
    )
}

/// The wait that failed on `task` with no `on-fail`, when its standing says so.
fn failed_without_on_fail(task: &TickTask) -> Option<ActionName> {
    match derive_standing(&task.list, &task.facts) {
        Standing::Failed {
            action,
            on_fail: None,
            ..
        } => Some(action),
        _ => None,
    }
}

/// Raises one question on each task of `tasks` whose standing is `Failed` with no `on-fail`,
/// unless the task already has an open question, and lists the tasks it raised one on, in order.
///
/// Each question is raised with [`board::question`], which also makes the task wait. Nothing is
/// started and nothing is charged. A board that fails stops the step with its error; the questions
/// raised before it stand, and the next tick finds them open.
pub fn ask_failed_waits(
    board: &dyn Board,
    tasks: &[TickTask],
) -> Result<Vec<TicketId>, BoardError> {
    let mut asked: BTreeSet<TicketId> = BTreeSet::new();
    let mut raised = Vec::new();
    let mut loaded = false;
    for task in tasks {
        let Some(action) = failed_without_on_fail(task) else {
            continue;
        };
        if !loaded {
            asked = board
                .questions()?
                .iter()
                .filter_map(|ticket| NewQuestion::raised_on(ticket.body.as_str()))
                .collect();
            loaded = true;
        }
        if !asked.insert(task.task) {
            continue;
        }
        let command = NewQuestion {
            on: task.task,
            text: question_text(&action, &task.facts),
            door: false,
        };
        board::question(board, &command)?;
        raised.push(task.task);
    }
    Ok(raised)
}

/// The outcome of a tick that did step 5 alone: [`TickOutcome::questions`] filled, the rest empty
/// for the steps that fill them.
pub fn ask_outcome(board: &dyn Board, tasks: &[TickTask]) -> Result<TickOutcome, BoardError> {
    Ok(TickOutcome {
        questions: ask_failed_waits(board, tasks)?,
        ..TickOutcome::default()
    })
}

#[cfg(test)]
mod tests;
