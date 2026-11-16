//! The last tick and whether a scheduler runs, as two readers (epic #1 §3).
//!
//! Every real tick leaves one line in the trace, `.zaf/ticks.jsonl` ([`crate::app::schedule::ticks`]),
//! and a scheduler holds `.zaf/scheduler.lock` while it runs ([`crate::app::schedule::lock`]).
//! This module writes neither. [`TickRecord::last`] reads the last line of the trace back, and
//! [`watch_running`] reads whether a scheduler holds the lock. Both only read, take no lock, and
//! are for the ones that show the project's state.

use std::path::Path;
use std::time::SystemTime;

use crate::adapters::process;
use crate::app::schedule::lock;
use crate::app::schedule::ticks;
use crate::app::schedule::trace::{Did, Move};
use crate::domain::TicketId;

/// What the last tick did, as the trace's last line holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickRecord {
    /// When the tick began.
    pub at: SystemTime,
    /// What it did: the model phase it started, the waits it launched, the actions it ran, the
    /// questions it raised.
    pub did: Did,
    /// The tickets that moved since the tick before.
    pub moved: Vec<Move>,
}

impl TickRecord {
    /// The last tick in the trace under `state_dir`: its last line that reads back. None when
    /// there is no trace, it cannot be read, or no line in it reads.
    pub fn last(state_dir: &Path) -> Option<Self> {
        let lines = ticks::read(state_dir).ok().flatten()?;
        let last = lines.into_iter().next_back()?;
        Some(Self {
            at: last.at,
            did: last.did,
            moved: last.moved,
        })
    }

    /// Whether `task` is named among what the tick started: its model phase, a wait it launched
    /// or an action it ran.
    pub fn started_task(&self, task: TicketId) -> bool {
        self.did
            .started
            .as_ref()
            .is_some_and(|started| started.task == task)
            || self.did.waits.iter().any(|wait| wait.task == task)
            || self.did.ran.iter().any(|ran| ran.task == task)
    }
}

/// Whether a scheduler runs for the project whose state folder is `state_dir`: a process holds
/// `.zaf/scheduler.lock` ([`lock::running`]). A scheduler that was killed holds nothing, so it is
/// never read as running.
pub fn watch_running(state_dir: &Path) -> bool {
    lock::running(state_dir, &process::alive).is_some()
}

#[cfg(test)]
mod tests;
