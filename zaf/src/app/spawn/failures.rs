//! A model phase that failed three times in a row is not started again; the tick raises one
//! question instead (docs/zaf-actions.md §5.1, #550).
//!
//! When the last [`MODEL_FAILURE_LIMIT`] runs of one model phase on one task all ended non-zero,
//! with no successful run of it between them, [`holds`] holds the task's model phase: step 6 does
//! not plan it, and [`ask_failed_models`] raises one question on the task naming the phase, the
//! exit codes and the last line of the last run's output. A task that already has an open question
//! gets no second one, so a second tick raises none.
//!
//! Nothing is stored. The streak is derived each tick from the phase run folders
//! ([`StandingFacts::failed_streak`]). A ruling lets the phase start again, and the ruled question
//! says where: its text ends with the start of the last run it counted (`Last run: <n>.`), so only
//! the failures after that run count toward the next three. A ruled question whose run is no longer
//! in the streak, because a run succeeded since, counts for nothing.
//!
//! Waits and `zaf`-kind actions are not held here: a failed wait has `on-fail` or a question of
//! its own (step 5, [`super::question`]).

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::phase_tick::PhaseTask;
use super::start::TaskStanding;
use crate::app::actions::{ActionName, MODEL_FAILURE_LIMIT};
use crate::app::board::{self, BoardError};
use crate::domain::TicketId;
use crate::ports::{Board, NewQuestion};

/// What closes a failed model's question: the start of its last run, which a ruling then marks as
/// the point the count starts again from.
const MARKER: &str = "Last run: ";

/// A task whose model phase failed [`MODEL_FAILURE_LIMIT`] times in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    /// The task.
    pub task: TicketId,
    /// The model action that is not started.
    pub action: ActionName,
    /// The exit codes of its last [`MODEL_FAILURE_LIMIT`] runs, oldest first.
    pub codes: Vec<i32>,
    /// The last line of the last run's output; none when it left none.
    pub last_line: Option<String>,
    /// When the last run started, as the question marks it.
    last_run: String,
}

/// A run's start as a question marks it: nanoseconds since the epoch.
fn stamp(at: SystemTime) -> String {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        .to_string()
}

/// The question a held phase raises:
///
/// `<action> failed <n> times in a row with exit codes <a>, <b>, <c>; the last line of its log:
/// "<line>". It is not started again until this is ruled. Last run: <stamp>.`
pub fn question_text(hold: &Hold) -> String {
    let codes: Vec<String> = hold.codes.iter().map(i32::to_string).collect();
    let last = hold.last_line.as_ref().map_or_else(
        || "none was recorded".to_owned(),
        |line| format!("\"{line}\""),
    );
    format!(
        "{} failed {} times in a row with exit codes {}; the last line of its log: {last}. \
         It is not started again until this is ruled. {MARKER}{}.",
        hold.action,
        codes.len(),
        codes.join(", "),
        hold.last_run
    )
}

/// The run a ruled question of `action` counted up to, read from its text.
fn marked_run<'a>(text: &'a str, action: &ActionName) -> Option<&'a str> {
    let text = text.trim();
    if !text.starts_with(&format!("{action} failed ")) {
        return None;
    }
    let (_, run) = text.strip_suffix('.')?.rsplit_once(MARKER)?;
    Some(run)
}

/// The tasks among `tasks` whose model phase is held: it is the task's next model start, and its
/// last [`MODEL_FAILURE_LIMIT`] runs, counted from the last ruled question's run, all ended non-zero.
///
/// The board is asked for a task's questions only when its streak is long enough to matter. A
/// board that cannot list them is the error, and the caller starts nothing on it.
pub fn holds(board: &dyn Board, tasks: &[PhaseTask]) -> Result<Vec<Hold>, BoardError> {
    let mut held = Vec::new();
    for task in tasks {
        let Some((action, _)) = TaskStanding::of(&task.tick, task.size).model_item() else {
            continue;
        };
        let streak = task.tick.facts.failed_streak(&action);
        if streak.len() < MODEL_FAILURE_LIMIT {
            continue;
        }
        let stamps: Vec<String> = streak.iter().map(|run| stamp(run.started_at)).collect();
        let ruled_through = board
            .questions_on(task.tick.task)?
            .iter()
            .filter(|(_, ruling)| ruling.is_some())
            .filter_map(|(ticket, _)| marked_run(NewQuestion::text_of(ticket), &action))
            .filter_map(|marked| stamps.iter().position(|stamp| stamp == marked))
            .max();
        let counted = &streak[ruled_through.map_or(0, |at| at + 1)..];
        if counted.len() < MODEL_FAILURE_LIMIT {
            continue;
        }
        let last = &counted[counted.len() - MODEL_FAILURE_LIMIT..];
        let newest = last[last.len() - 1];
        held.push(Hold {
            task: task.tick.task,
            action,
            codes: last
                .iter()
                .filter_map(|run| run.exit.map(|exit| exit.code))
                .collect(),
            last_line: newest.last_line.clone(),
            last_run: stamp(newest.started_at),
        });
    }
    Ok(held)
}

/// Raises one question on each held task, unless it already has an open question, and lists the
/// tasks it raised one on, in order. Each is raised with [`board::question`], which makes the task
/// wait. Nothing is started and nothing is charged.
pub fn ask_failed_models(board: &dyn Board, held: &[Hold]) -> Result<Vec<TicketId>, BoardError> {
    let mut raised = Vec::new();
    if held.is_empty() {
        return Ok(raised);
    }
    let mut asked: BTreeSet<TicketId> = board
        .questions()?
        .iter()
        .filter_map(|ticket| NewQuestion::raised_on(ticket.body.as_str()))
        .collect();
    for hold in held {
        if !asked.insert(hold.task) {
            continue;
        }
        let command = NewQuestion {
            on: hold.task,
            text: question_text(hold),
            door: false,
        };
        board::question(board, &command)?;
        raised.push(hold.task);
    }
    Ok(raised)
}

#[cfg(test)]
mod tests;
