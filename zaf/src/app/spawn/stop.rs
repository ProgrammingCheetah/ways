//! `zaf spawn stop <task>`: ends the instance running a task and releases its claim, so a later
//! tick can start it again (docs/zaf-actions.md §5.1, claims).
//!
//! The instance is a phase run folder in `.zaf/phases/` whose `task` is the task, with no `exit`
//! recorded and a holder that is alive. [`stop`] asks it to end (`SIGTERM`, to the holder and to
//! every process of its group), waits up to a grace for it to go, then kills what is left
//! (`SIGKILL`), and only then releases the task's claim through the Board port, so the task reads
//! ready again. The group is read before the first signal, since the holder's group is gone once
//! the holder is.
//!
//! A stopped run records no `exit`: the launcher shares the group and goes with the action, so
//! nobody is left to record one, and a dead holder with no exit reads as no exit code, never as 0.
//! Nothing is stored and no tick is asked for. The whole stop holds `spawn.lock`, so a tick never
//! starts a phase between the signals and the release.
//!
//! A task with no live instance is [`StopError::NoInstance`] and nothing changes.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Released, SpawnError, lock};
use crate::app::actions::PHASES_DIR;
use crate::domain::TicketId;
use crate::ports::{Board, Holder, PortError};

/// How long an instance gets to end after `SIGTERM` before it is killed.
pub const GRACE: Duration = Duration::from_secs(5);

/// How long [`stop`] waits between two looks at the process table.
pub const POLL: Duration = Duration::from_millis(50);

/// How long a killed instance gets to be gone before [`stop`] gives up on it.
const KILL_WAIT: Duration = Duration::from_secs(2);

/// The process table and the signals, as [`stop`] uses them.
pub trait Processes {
    /// Whether the recorded process still runs.
    fn alive(&self, holder: &Holder) -> bool;
    /// The process group the holder belongs to, which [`Processes::signal`] reaches as a whole;
    /// none when it has none to signal alone (its own zaf's, or it is gone).
    fn group_of(&self, holder: &Holder) -> Option<u32>;
    /// Sends the signal to the holder and to every process of `group`.
    fn signal(&self, holder: &Holder, group: Option<u32>, signal: Signal);
    /// Waits `time` before the next look.
    fn pause(&self, time: Duration);
}

/// A signal [`Processes::signal`] sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Asks the process to end.
    Terminate,
    /// Ends it.
    Kill,
}

/// One instance that was running the task and was ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoppedRun {
    /// Its phase run folder.
    pub run_dir: PathBuf,
    /// The instance, `<action> instance <n> on #<epic>`; empty when the folder does not say.
    pub instance: String,
    /// Its process.
    pub pid: u32,
    /// Whether it did not end within the grace and was killed.
    pub killed: bool,
}

/// What a stop did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// The task.
    pub task: TicketId,
    /// The instances ended; one in practice, since a task's phases never overlap.
    pub runs: Vec<StoppedRun>,
    /// The claims released: the task's, when it was claimed.
    pub released: Vec<Released>,
}

/// Why a task was not stopped.
#[derive(Debug, thiserror::Error)]
pub enum StopError {
    /// No instance runs the task: nothing was changed.
    #[error("no instance is running {0}")]
    NoInstance(TicketId),
    /// An instance would not end, even killed; its claim is kept.
    #[error("{pid} ({instance}) did not end, even killed; {task}'s claim is kept")]
    Survived {
        /// The task.
        task: TicketId,
        /// The instance.
        instance: String,
        /// Its process.
        pid: u32,
    },
    /// A run folder could not be read, or the lock taken.
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    /// The board failed.
    #[error(transparent)]
    Port(#[from] PortError),
}

/// A live run of the task, as read from its folder.
struct Live {
    run_dir: PathBuf,
    instance: String,
    holder: Holder,
}

/// Ends every instance running `task` and releases its claim.
///
/// Each instance gets `SIGTERM` and `grace` to be gone, then `SIGKILL`; every process of its group
/// gets the same signals. The claim is released after they are gone, and only when it is held: a
/// task nobody claimed is left as it is. An instance that is still there after the kill is
/// [`StopError::Survived`], and the claim stays.
pub fn stop(
    task: TicketId,
    state_dir: &Path,
    board: &dyn Board,
    processes: &dyn Processes,
    grace: Duration,
) -> Result<Stopped, StopError> {
    let _held = lock(state_dir)?;
    let live = live_runs(&state_dir.join(PHASES_DIR), task, processes)?;
    if live.is_empty() {
        return Err(StopError::NoInstance(task));
    }
    let mut runs = Vec::new();
    for run in live {
        let killed = end(&run.holder, processes, grace);
        if processes.alive(&run.holder) {
            return Err(StopError::Survived {
                task,
                instance: run.instance,
                pid: run.holder.pid,
            });
        }
        runs.push(StoppedRun {
            run_dir: run.run_dir,
            instance: run.instance,
            pid: run.holder.pid,
            killed,
        });
    }
    let mut released = Vec::new();
    for claim in board.claims()?.iter().filter(|claim| claim.task == task) {
        board.release(task, &claim.instance)?;
        released.push(Released {
            task,
            instance: claim.instance.clone(),
        });
    }
    Ok(Stopped {
        task,
        runs,
        released,
    })
}

/// Ends one process and its group: `SIGTERM`, a wait of up to `grace`, then `SIGKILL`. Returns
/// whether the kill was needed. Once the holder is gone the group's stragglers are killed too,
/// since nothing of a stopped instance may go on.
fn end(holder: &Holder, processes: &dyn Processes, grace: Duration) -> bool {
    let group = processes.group_of(holder);
    processes.signal(holder, group, Signal::Terminate);
    let gone = wait_gone(holder, processes, grace);
    processes.signal(holder, group, Signal::Kill);
    if !gone {
        wait_gone(holder, processes, KILL_WAIT);
    }
    !gone
}

/// Looks every [`POLL`] until the holder is gone or `limit` has passed; whether it is gone.
fn wait_gone(holder: &Holder, processes: &dyn Processes, limit: Duration) -> bool {
    let mut waited = Duration::ZERO;
    while processes.alive(holder) {
        if waited >= limit {
            return false;
        }
        processes.pause(POLL);
        waited += POLL;
    }
    true
}

/// The run folders under `dir` that run `task` now: no `exit` recorded and a holder that is
/// alive. An absent folder is none; a folder with no `task` file is not a run folder.
fn live_runs(
    dir: &Path,
    task: TicketId,
    processes: &dyn Processes,
) -> Result<Vec<Live>, SpawnError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(unreadable(dir, &source)),
    };
    let mut paths = BTreeSet::new();
    for entry in entries {
        let path = entry.map_err(|source| unreadable(dir, &source))?.path();
        if path.is_dir() {
            paths.insert(path);
        }
    }
    let mut live = Vec::new();
    for path in paths {
        let Some(text) = read_text(&path.join("task"))? else {
            continue;
        };
        if text.trim().parse::<u64>().ok() != Some(task.get()) {
            continue;
        }
        if path.join(super::phase::EXIT_FILE).exists() {
            continue;
        }
        let Some(holder) =
            read_text(&path.join(super::phase::PID_FILE))?.and_then(|text| Holder::parse(&text))
        else {
            continue;
        };
        if !processes.alive(&holder) {
            continue;
        }
        let instance = read_text(&path.join(super::phase::INSTANCE_FILE))?
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        live.push(Live {
            run_dir: path,
            instance,
            holder,
        });
    }
    Ok(live)
}

fn unreadable(path: &Path, source: &std::io::Error) -> SpawnError {
    SpawnError::Read {
        path: path.to_owned(),
        reason: source.to_string(),
    }
}

/// A file's text; none when it is absent.
fn read_text(path: &Path) -> Result<Option<String>, SpawnError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(unreadable(path, &source)),
    }
}

#[cfg(test)]
mod tests;
