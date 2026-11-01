//! `.zaf/spawn.state`: the next instance number and the launches still holding a slot.
//!
//! The file is the shape `zaf status` reads: one [`Launch`] per task, every task
//! of a batch under the same instance, plus the `next_instance` counter. An
//! instance holds one slot however many tasks it carries.

use std::path::Path;

use super::SpawnError;
use super::tick::PlannedLaunch;
use crate::app::status::{Launch, SPAWN_STATE_FILE};

/// The number the first instance gets.
const FIRST_INSTANCE: u32 = 1;

fn first_instance() -> u32 {
    FIRST_INSTANCE
}

/// What `.zaf/spawn.state` holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnState {
    /// The number the next instance gets.
    #[serde(default = "first_instance")]
    pub next_instance: u32,
    /// The launches holding a slot, one entry per task.
    #[serde(default)]
    pub launches: Vec<Launch>,
    /// Questions the spawner raised; kept as read, empty for now.
    #[serde(default)]
    pub questions: Vec<u64>,
}

impl Default for SpawnState {
    fn default() -> Self {
        Self {
            next_instance: FIRST_INSTANCE,
            launches: Vec::new(),
            questions: Vec::new(),
        }
    }
}

impl SpawnState {
    /// The instances holding a slot, each once, in the order they were recorded.
    pub fn instances(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for launch in &self.launches {
            if !seen.contains(&launch.instance.as_str()) {
                seen.push(&launch.instance);
            }
        }
        seen
    }

    /// Drops every instance whose process is gone and returns them.
    ///
    /// `alive` answers for a recorded pid; an instance recorded without one
    /// cannot be checked and is dropped too, so it never holds a slot forever.
    pub fn prune(&mut self, alive: &dyn Fn(u32) -> bool) -> Vec<String> {
        let lost: Vec<String> = self
            .instances()
            .into_iter()
            .filter(|instance| {
                !self
                    .launches
                    .iter()
                    .filter(|launch| launch.instance == *instance)
                    .any(|launch| launch.pid.is_some_and(alive))
            })
            .map(str::to_owned)
            .collect();
        self.launches
            .retain(|launch| !lost.contains(&launch.instance));
        lost
    }

    /// Records the planned launches under `pid`, not yet started, and advances the
    /// counter past them (the plan numbers its launches from the counter, in order).
    pub fn reserve(&mut self, planned: &[PlannedLaunch], pid: u32) {
        for launch in planned {
            let instance = launch.instance.to_string();
            for task in &launch.tasks {
                self.launches.push(Launch {
                    task: task.get(),
                    instance: instance.clone(),
                    pid: Some(pid),
                    started_at: None,
                });
            }
            self.next_instance = self.next_instance.saturating_add(1);
        }
    }

    /// Marks an instance's entries as started at `at` (RFC 3339).
    pub fn start(&mut self, instance: &str, at: &str) {
        for launch in &mut self.launches {
            if launch.instance == instance {
                launch.started_at = Some(at.to_owned());
            }
        }
    }

    /// Drops an instance's entries: it exited and its slot is free.
    pub fn finish(&mut self, instance: &str) {
        self.launches.retain(|launch| launch.instance != instance);
    }
}

/// Reads `<state dir>/spawn.state`; an absent file is the empty state.
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

/// Writes `<state dir>/spawn.state`, creating the folder.
pub fn save(state_dir: &Path, state: &SpawnState) -> Result<(), SpawnError> {
    let path = state_dir.join(SPAWN_STATE_FILE);
    let text = serde_json::to_string_pretty(state).unwrap_or_default();
    std::fs::create_dir_all(state_dir)
        .and_then(|()| std::fs::write(&path, text))
        .map_err(|source| SpawnError::Write { path, source })
}

#[cfg(test)]
mod tests;
