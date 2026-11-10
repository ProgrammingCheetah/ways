//! `.zaf/spawn.state` holds the instance counter and nothing else; the running launches are
//! read from the run folders each time ([`derive`]).
//!
//! [`SpawnState`] is the shape the tick and `zaf status` read: one [`Launch`] per task of
//! every launch that runs now, every task of a batch under the same instance, plus the
//! `next_instance` counter. An instance holds one slot however many tasks it carries. Only
//! the counter is saved: a live source answers everything else.

use std::path::Path;

use super::SpawnError;
use super::runs::{self, Run};
use crate::app::status::{Launch, SPAWN_STATE_FILE};
use crate::domain::Instance;
use crate::ports::Holder;

/// The number the first instance gets.
const FIRST_INSTANCE: u32 = 1;

fn first_instance() -> u32 {
    FIRST_INSTANCE
}

/// The instance counter as saved, and the launches running now as read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnState {
    /// The number the next instance gets.
    #[serde(default = "first_instance")]
    pub next_instance: u32,
    /// The launches running now, one entry per task: read from the run folders by
    /// [`derive`], never saved.
    #[serde(skip)]
    pub launches: Vec<Launch>,
}

impl Default for SpawnState {
    fn default() -> Self {
        Self {
            next_instance: FIRST_INSTANCE,
            launches: Vec::new(),
        }
    }
}

impl SpawnState {
    /// The instances holding a slot, each once, in the order they were read.
    pub fn instances(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for launch in &self.launches {
            if !seen.contains(&launch.instance.as_str()) {
                seen.push(&launch.instance);
            }
        }
        seen
    }

    /// How many slots are held: the distinct instances among the launches, however many tasks each carries.
    ///
    /// The tick's free slots and `zaf status` both read this number; neither recounts launches.
    pub fn running(&self) -> usize {
        self.instances().len()
    }

    /// The running instances, each once, in the order they were read.
    ///
    /// A recorded name that is not an instance is counted by [`running`](Self::running)
    /// (it holds a slot) but cannot be listed here.
    pub fn running_instances(&self) -> Vec<Instance> {
        self.instances()
            .into_iter()
            .filter_map(|name| name.parse::<Instance>().ok())
            .collect()
    }

    /// The tasks an instance carries, in the order they were recorded.
    pub fn tasks_of(&self, instance: &Instance) -> Vec<u64> {
        let name = instance.to_string();
        self.launches
            .iter()
            .filter(|launch| launch.instance == name)
            .map(|launch| launch.task)
            .collect()
    }
}

/// The saved counter with the launches that run now, read from `runs`: one [`Launch`] per
/// task of each run whose holder `alive` confirms. The counter never falls behind a run
/// folder already there, so an instance number is never used twice.
pub fn with_runs(saved: &SpawnState, runs: &[Run], alive: &dyn Fn(&Holder) -> bool) -> SpawnState {
    let mut launches = Vec::new();
    for run in runs.iter().filter(|run| run.running(alive)) {
        let Some(record) = &run.record else {
            continue;
        };
        for task in &record.tasks {
            launches.push(Launch {
                task: *task,
                instance: record.instance.clone(),
                pid: run.holder.as_ref().map(|holder| holder.pid),
                started_at: record.started_at.clone(),
            });
        }
    }
    let past_runs = runs
        .iter()
        .filter_map(Run::number)
        .max()
        .map_or(FIRST_INSTANCE, |highest| highest.saturating_add(1));
    SpawnState {
        next_instance: saved.next_instance.max(past_runs),
        launches,
    }
}

/// Reads the counter and the run folders, and keeps the launches that run now.
pub fn derive(
    state_dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<(SpawnState, Vec<Run>), SpawnError> {
    let saved = load(state_dir)?;
    let runs = runs::read_all(state_dir)?;
    Ok((with_runs(&saved, &runs, alive), runs))
}

/// Reads `<state dir>/spawn.state`; an absent file is the empty state. Fields an older zaf
/// saved (its launches and questions) are ignored.
pub fn load(state_dir: &Path) -> Result<SpawnState, SpawnError> {
    let path = state_dir.join(SPAWN_STATE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SpawnState::default());
        }
        Err(source) => {
            return Err(SpawnError::Read {
                path,
                reason: source.to_string(),
            });
        }
    };
    serde_json::from_str(&text).map_err(|error| SpawnError::Read {
        path,
        reason: error.to_string(),
    })
}

/// Writes `<state dir>/spawn.state` (the counter alone), creating the folder.
pub fn save(state_dir: &Path, state: &SpawnState) -> Result<(), SpawnError> {
    let path = state_dir.join(SPAWN_STATE_FILE);
    let text = serde_json::to_string_pretty(state).unwrap_or_default();
    std::fs::create_dir_all(state_dir)
        .and_then(|()| std::fs::write(&path, text))
        .map_err(|source| SpawnError::Write { path, source })
}

#[cfg(test)]
mod tests;
