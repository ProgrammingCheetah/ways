//! `zaf status`: the board, running instances, slots, guard and open questions on one screen.
//!
//! The board gives the ready tasks, the live claims and the open questions.
//! `.zaf/spawn.state`, the spawner's file, gives what is running.
//! The guard line is whatever `zaf guard` last wrote.

use std::path::{Path, PathBuf};

use super::guard::{self, GuardError, GuardState};
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

/// The slots: how many there are and how many are free.
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
    /// Launches running now, from `spawn.state`.
    pub running: Vec<Launch>,
    /// The slots.
    pub slots: Slots,
    /// The guard as last computed; none if `zaf guard` never ran.
    pub guard: Option<GuardState>,
    /// Questions still waiting for the Engineer.
    pub questions: Vec<TicketLine>,
}

/// What the spawner writes to `spawn.state`: a JSON object. Its `questions` array, if any, is
/// not read: the board is the source of open questions.
#[derive(Debug, Default, serde::Deserialize)]
struct SpawnState {
    #[serde(default)]
    launches: Vec<Launch>,
}

fn read_spawn_state(state_dir: &Path) -> Result<SpawnState, StatusError> {
    let path = state_dir.join(SPAWN_STATE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SpawnState::default());
        }
        Err(source) => {
            return Err(StatusError::SpawnState {
                path,
                reason: source.to_string(),
            });
        }
    };
    serde_json::from_str(&text).map_err(|error| StatusError::SpawnState {
        path,
        reason: error.to_string(),
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
    let running = state.launches;
    let in_use = u32::try_from(running.len()).unwrap_or(u32::MAX);
    Ok(Status {
        ready,
        claimed,
        slots: Slots {
            total: slots,
            free: slots.saturating_sub(in_use),
        },
        running,
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
        line(format!("running   {}", self.running.len()));
        for launch in &self.running {
            let pid = launch
                .pid
                .map_or(String::new(), |pid| format!(" pid {pid}"));
            let since = launch
                .started_at
                .as_ref()
                .map_or(String::new(), |at| format!(" since {at}"));
            line(format!(
                "  #{} {}{pid}{since}",
                launch.task, launch.instance
            ));
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
