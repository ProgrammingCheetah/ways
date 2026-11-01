//! `zaf status`: the board, running instances, slots, guard and open questions on one screen.
//!
//! The board gives the ready tasks, the live claims and the open questions.
//! `.zaf/spawn.state`, the spawner's file, gives what is running.
//! The guard line is whatever `zaf guard` last wrote.

use std::path::{Path, PathBuf};

use super::guard::{self, GuardError, GuardState};
use super::spawn;
use crate::ports::{Board, PortError, Ticket};

/// The spawner's state file under the state folder.
pub const SPAWN_STATE_FILE: &str = "spawn.state";

/// Why the status could not be read.
#[derive(Debug, thiserror::Error)]
pub enum StatusError {
    /// The board failed.
    #[error(transparent)]
    Board(#[from] PortError),
    /// `.zaf/spawn.state` is there but unreadable.
    #[error("cannot read {path}: {reason}")]
    SpawnState {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// `.zaf/guard.json` is there but unreadable.
    #[error(transparent)]
    Guard(#[from] GuardError),
}

/// A ticket as a line of the screen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TicketLine {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
}

/// A live claim.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClaimLine {
    /// The task.
    pub task: u64,
    /// The instance holding it.
    pub instance: String,
}

/// One launch the spawner started and has not seen finish.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    /// The task it works on.
    pub task: u64,
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// Its process, when known.
    #[serde(default)]
    pub pid: Option<u32>,
    /// When it started, RFC 3339, when known.
    #[serde(default)]
    pub started_at: Option<String>,
}

/// A running instance and the tasks it carries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstanceLine {
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// Its tasks.
    pub tasks: Vec<u64>,
}

/// The slots: how many there are and how many are free (a slot is one distinct running instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Slots {
    /// The manifest's `[spawn] slots`.
    pub total: u32,
    /// Slots without a launch.
    pub free: u32,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Status {
    /// Tasks open for a claim.
    pub ready: Vec<TicketLine>,
    /// Claims held now.
    pub claimed: Vec<ClaimLine>,
    /// Running instances, from `spawn.state`: one slot each, however many tasks.
    pub running_count: usize,
    /// The running instances, each once with the tasks it carries.
    pub instances: Vec<InstanceLine>,
    /// Every recorded launch, one per task, from `spawn.state`.
    pub running: Vec<Launch>,
    /// The slots.
    pub slots: Slots,
    /// The guard as last computed; none if `zaf guard` never ran.
    pub guard: Option<GuardState>,
    /// Questions still waiting for the Engineer.
    pub questions: Vec<TicketLine>,
}

/// Reads `spawn.state` through the spawner's own reader, so the state owns the count.
/// Its `questions` array is not used: the board is the source of open questions.
fn read_spawn_state(state_dir: &Path) -> Result<spawn::SpawnState, StatusError> {
    spawn::load(state_dir).map_err(|error| match error {
        spawn::SpawnError::Read { path, reason } => StatusError::SpawnState { path, reason },
        spawn::SpawnError::Write { path, source } => StatusError::SpawnState {
            path,
            reason: source.to_string(),
        },
        spawn::SpawnError::Port(error) => StatusError::Board(error),
    })
}

/// Reads the board, `spawn.state` and `guard.json` into one screen.
///
/// The open questions come from the board, not from `spawn.state`.
pub fn status(board: &dyn Board, state_dir: &Path, slots: u32) -> Result<Status, StatusError> {
    let state = read_spawn_state(state_dir)?;
    let ready = board.ready()?.into_iter().map(ticket_line).collect();
    let claimed = board
        .claims()?
        .into_iter()
        .map(|claim| ClaimLine {
            task: claim.task.get(),
            instance: claim.instance.to_string(),
        })
        .collect();
    let questions = board.questions()?.into_iter().map(ticket_line).collect();
    // A batch of tasks shares one instance and one slot; the state counts them.
    let in_use = state.running();
    let instances = state
        .running_instances()
        .into_iter()
        .map(|instance| InstanceLine {
            tasks: state.tasks_of(&instance),
            instance: instance.to_string(),
        })
        .collect();
    Ok(Status {
        ready,
        claimed,
        slots: Slots {
            total: slots,
            free: slots.saturating_sub(u32::try_from(in_use).unwrap_or(u32::MAX)),
        },
        running_count: in_use,
        instances,
        running: state.launches,
        guard: guard::read(state_dir)?,
        questions,
    })
}

fn ticket_line(ticket: Ticket) -> TicketLine {
    TicketLine {
        number: ticket.number.get(),
        title: ticket.title,
    }
}

fn numbers(lines: impl Iterator<Item = u64>) -> String {
    let list: Vec<String> = lines.map(|number| format!("#{number}")).collect();
    if list.is_empty() {
        String::new()
    } else {
        format!(" ({})", list.join(" "))
    }
}

impl Status {
    /// The screen as text.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let mut line = |text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        line(format!(
            "ready     {}{}",
            self.ready.len(),
            numbers(self.ready.iter().map(|t| t.number))
        ));
        line(format!(
            "claimed   {}{}",
            self.claimed.len(),
            numbers(self.claimed.iter().map(|c| c.task))
        ));
        line(format!(
            "questions {}{}",
            self.questions.len(),
            numbers(self.questions.iter().map(|q| q.number))
        ));
        line(format!("running   {}", self.running_count));
        for instance in &self.instances {
            let tasks: Vec<String> = instance
                .tasks
                .iter()
                .map(|task| format!("#{task}"))
                .collect();
            line(format!("  {}: {}", instance.instance, tasks.join(" ")));
        }
        line(format!(
            "slots     {} free of {}",
            self.slots.free, self.slots.total
        ));
        line(format!(
            "guard     {}",
            self.guard.as_ref().map_or(
                "not computed (run `zaf guard`)".to_owned(),
                GuardState::line
            )
        ));
        for question in &self.questions {
            line(format!("question  #{} {}", question.number, question.title));
        }
        out
    }

    /// The whole screen as JSON for the spawner.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

#[cfg(test)]
mod tests;
