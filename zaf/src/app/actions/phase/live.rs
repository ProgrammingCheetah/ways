//! The phase runs as `zaf status`, the tick's digest and the claim release read them: which tasks
//! have a phase process running ([`running_tasks`], [`live_phases`]) and which instances of the
//! phase run folders no longer run ([`ended_instances`]). The folders are read through
//! [`crate::app::schedule::record`], which documents them; where a ticket sits is the scheduling
//! core's, never read here.
//!
//! Nothing here writes: it lists a folder and reads files and their times.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::Path;
use std::time::SystemTime;

use crate::app::actions::ActionName;
use crate::app::schedule::record::{read_run, read_text};
use crate::domain::TicketId;
use crate::ports::Holder;

pub use crate::app::schedule::record::PHASES_DIR;

/// The tasks with a phase process running under `dir`: a run folder with no exit recorded and a
/// holder that is alive. An absent folder is none; a folder that is no run folder names no task.
pub fn running_tasks(
    dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<BTreeSet<TicketId>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut tasks = BTreeSet::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(text) = read_text(&path.join("task"))? else {
            continue;
        };
        let Ok(task) = text.trim().parse::<TicketId>() else {
            continue;
        };
        if read_run(&path, task, alive)?.is_some_and(|run| run.alive) {
            tasks.insert(task);
        }
    }
    Ok(tasks)
}

/// A task's phase that runs now, read from its run folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePhase {
    /// The task.
    pub task: TicketId,
    /// The action it runs.
    pub action: ActionName,
    /// The instance it runs as, `<action> instance <n> on #<epic>`, when its folder names one.
    pub instance: Option<String>,
    /// When it started.
    pub started_at: SystemTime,
}

impl LivePhase {
    /// The epic the instance names, when it does.
    pub fn epic(&self) -> Option<TicketId> {
        self.instance
            .as_deref()?
            .parse::<crate::domain::Instance>()
            .ok()
            .map(|instance| instance.epic)
    }
}

/// Every phase that runs under `dir`, the lowest task first: a run folder with no exit recorded
/// and a holder that is alive, the same test [`running_tasks`] makes. An absent folder is none.
pub fn live_phases(dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Result<Vec<LivePhase>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut live = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(task) =
            read_text(&path.join("task"))?.and_then(|text| text.trim().parse::<TicketId>().ok())
        else {
            continue;
        };
        if let Some(run) = read_run(&path, task, alive)?.filter(|run| run.alive) {
            let instance = read_text(&path.join("instance"))?
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty());
            live.push(LivePhase {
                task,
                action: run.action,
                instance,
                started_at: run.started_at,
            });
        }
    }
    live.sort_by_key(|phase| (phase.task, phase.started_at));
    Ok(live)
}

/// The instances of the phase run folders under `dir` that no longer run: a run folder with an
/// exit recorded, or whose holder is gone. A folder with no `instance` file names none. An absent
/// folder is none.
pub fn ended_instances(
    dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<BTreeSet<String>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut instances = BTreeSet::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(task) =
            read_text(&path.join("task"))?.and_then(|text| text.trim().parse::<TicketId>().ok())
        else {
            continue;
        };
        let Some(instance) = read_text(&path.join("instance"))?
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        if read_run(&path, task, alive)?.is_some_and(|run| !run.alive) {
            instances.insert(instance);
        }
    }
    Ok(instances)
}
