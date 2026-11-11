//! `zaf spawn phase <task> <action>`: starts one phase action of a task as its own process
//! (docs/zaf-actions.md §5.1).
//!
//! [`start`] reads the task, derives its standing in its type's phase list
//! ([`derive_standing`]) and refuses unless `action` is the task's current phase and no phase of
//! the task runs. Under the spawner's lock it takes the next instance number and writes the phase
//! run folder in `.zaf/phases/` in the format [`derive_standing`] reads (the `task` file last, so
//! its time is the start). The caller then starts the launcher on that folder and returns at
//! once; [`Started`] holds what it needs.
//!
//! [`launch_of`] reads that folder back for the launcher: the action, the task, its size and the
//! instance the action runs as. The launcher is the entry point #439 and #440 extend.
//!
//! [`end`] is the launcher's first step once its child has exited: it records the exit code in the
//! run folder's `exit` file ([`record_exit`]), and only then runs what follows, the post-phase hook
//! and the tick's ask (#439, #440). The code is [`exit_code`]: the child's own, or `128 + n` for a
//! signal `n`. It is a recorded act kept beside the launch record, never a stored queue.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{SpawnError, lock, slug};
use crate::app::actions::{
    ActionName, ListedPhase, PHASES_DIR, Standing, derive_standing, gather_standing_facts, parse,
    resolve,
};
use crate::domain::{DEFAULT_TASK_TYPE, Instance, InstanceNumber, RoleName, TaskSize, TicketId};
use crate::manifest::Manifest;
use crate::ports::{Board, Forge, Holder, PortError};

/// The file of a phase run folder that names the size the action runs at (`ZAF_SIZE`).
pub const SIZE_FILE: &str = "size";
/// The file of a phase run folder that names the instance the action runs as (`ZAF_INSTANCE`).
pub const INSTANCE_FILE: &str = "instance";
/// The file of a phase run folder that holds the task's number; written last.
pub const TASK_FILE: &str = "task";
/// The file of a phase run folder that holds the action's name.
pub const ACTION_FILE: &str = "action";
/// The file of a phase run folder that holds the pull request's head when it started.
pub const HEAD_FILE: &str = "head";
/// The file of a phase run folder that holds the action's process, as a holder.
pub const PID_FILE: &str = "pid";
/// The file of a phase run folder that holds the process's exit code, written once it has ended.
pub const EXIT_FILE: &str = "exit";

/// Why a phase was not started.
#[derive(Debug, thiserror::Error)]
pub enum PhaseError {
    /// The state forbids it: not the current phase, or the task's phase already runs.
    #[error("{0}")]
    Refused(String),
    /// Something it needs failed or is missing.
    #[error("{0}")]
    Failed(String),
}

impl From<SpawnError> for PhaseError {
    fn from(error: SpawnError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<PortError> for PhaseError {
    fn from(error: PortError) -> Self {
        Self::Failed(error.to_string())
    }
}

/// A phase run folder written for a launcher to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The phase run folder, `<state dir>/phases/<name>`.
    pub run_dir: PathBuf,
    /// The instance the action runs as, `<action> instance <n> on #<epic>`.
    pub instance: String,
}

/// What a launcher reads back from a phase run folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The action to run.
    pub action: ActionName,
    /// The task.
    pub task: TicketId,
    /// The task's size, as `ZAF_SIZE` carries it.
    pub size: String,
    /// The instance, as `ZAF_INSTANCE` carries it.
    pub instance: String,
}

/// What [`start`] reads from.
pub struct Ports<'a> {
    /// The board the task is read from.
    pub board: &'a dyn Board,
    /// The forge the standing's facts are read from.
    pub forge: &'a dyn Forge,
    /// The manifest holding `[phases]`.
    pub manifest: &'a Manifest,
    /// The project root.
    pub root: &'a Path,
    /// The machine's zaf home, for the machine tier of the actions.
    pub home: &'a Path,
    /// The state folder, `<root>/.zaf`.
    pub state_dir: &'a Path,
    /// Whether a recorded process still runs.
    pub alive: &'a dyn Fn(&Holder) -> bool,
}

/// Checks that `action` is the current phase of `task` and that none of its phases runs, takes
/// the next instance number and writes the phase run folder. Nothing is started.
///
/// The current phase is the standing's `Ready` action, or the `on-fail` action of a `Failed` wait.
/// The check, the folder and `launch` all run under the spawner's lock, so two starts never both
/// pass. `launch` starts the launcher on the folder and returns its process, which is recorded as
/// the run's `pid` before the lock is let go: the run reads as running from the moment it is
/// written, and the launcher replaces the record with the action's own process.
pub fn start(
    ports: &Ports<'_>,
    task: TicketId,
    action: &str,
    launch: &dyn Fn(&Started) -> Result<Holder, String>,
) -> Result<Started, PhaseError> {
    let ticket = ports.board.task(task)?;
    let epic = ticket
        .epic
        .ok_or_else(|| PhaseError::Refused(format!("{task} is not a task of an epic")))?;
    let size = ticket
        .template()
        .map_err(|error| PhaseError::Refused(format!("{task} is not a task: {error}")))?
        .size;
    let task_type = ticket
        .kind
        .as_ref()
        .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str());
    let names = ports.manifest.phase_list(task_type).ok_or_else(|| {
        PhaseError::Refused(format!("[phases] has no list for the type {task_type}"))
    })?;
    let list = listed(ports.root, ports.home, names)?;

    let _held = lock(ports.state_dir)?;
    let facts = gather_standing_facts(
        ports.forge,
        ports.root,
        task,
        SystemTime::now(),
        ports.alive,
    )
    .map_err(|error| PhaseError::Failed(error.to_string()))?;
    let standing = derive_standing(&list, &facts);
    check_current(&standing, task, action)?;

    let mut state = super::state::load(ports.state_dir)?;
    let number = state.next_instance;
    let instance = Instance {
        role: RoleName::from(action),
        epic,
        number: InstanceNumber::from(number),
    };
    state.next_instance = number.saturating_add(1);
    super::save(ports.state_dir, &state)?;

    let run_dir =
        ports
            .state_dir
            .join(PHASES_DIR)
            .join(format!("{}-task-{}", slug(&instance), task.get()));
    let head = facts
        .pull
        .as_ref()
        .and_then(|pull| pull.head.clone())
        .unwrap_or_default();
    let write = |name: &str, text: &str| -> Result<(), PhaseError> {
        let path = run_dir.join(name);
        std::fs::write(&path, text).map_err(|source| {
            PhaseError::Failed(format!("cannot write {}: {source}", path.display()))
        })
    };
    std::fs::create_dir_all(&run_dir).map_err(|source| {
        PhaseError::Failed(format!("cannot make {}: {source}", run_dir.display()))
    })?;
    write(ACTION_FILE, action)?;
    write(HEAD_FILE, &head)?;
    write(SIZE_FILE, size.name())?;
    write(INSTANCE_FILE, &instance.to_string())?;
    // Last: its time is when the run started.
    write(TASK_FILE, &task.get().to_string())?;
    let started = Started {
        run_dir,
        instance: instance.to_string(),
    };
    let holder = launch(&started).map_err(PhaseError::Failed)?;
    record_pid(&started.run_dir, &holder).map_err(PhaseError::Failed)?;
    Ok(started)
}

/// Refuses unless `action` is the phase `standing` calls for.
fn check_current(standing: &Standing, task: TicketId, action: &str) -> Result<(), PhaseError> {
    match standing {
        Standing::Running { action: running } => Err(PhaseError::Refused(format!(
            "{task}'s phase {running} already runs"
        ))),
        Standing::Ready { action: ready, .. } if ready.as_str() == action => Ok(()),
        Standing::Failed {
            on_fail: Some(next),
            ..
        } if next.as_str() == action => Ok(()),
        other => Err(PhaseError::Refused(format!(
            "{action} is not {task}'s current phase: {}",
            describe(other)
        ))),
    }
}

/// The standing in a few words, for a refusal.
fn describe(standing: &Standing) -> String {
    match standing {
        Standing::Running { action } => format!("{action} runs"),
        Standing::Ready { action, .. } => format!("{action} is ready"),
        Standing::Waiting { action } => format!("{action} waits"),
        Standing::Failed {
            action,
            on_fail: Some(next),
            ..
        } => format!("{action} failed, {next} is next"),
        Standing::Failed { action, .. } => format!("{action} failed and names no on-fail action"),
        Standing::Done => "every phase is done".to_owned(),
    }
}

/// The task's phase list with what each action declares. An action that cannot be found or read,
/// or that declares no kind, cannot be a phase.
fn listed(root: &Path, home: &Path, names: &[ActionName]) -> Result<Vec<ListedPhase>, PhaseError> {
    let mut list = Vec::new();
    for name in names {
        let failed = |reason: String| PhaseError::Failed(format!("phase {name}: {reason}"));
        let found = resolve(root, home, name.as_str())
            .map_err(|error| failed(error.to_string()))?
            .ok_or_else(|| failed("no such action".to_owned()))?;
        let text = std::fs::read_to_string(&found.path)
            .map_err(|error| failed(format!("cannot read {}: {error}", found.path.display())))?;
        let metadata = parse(&found.path, &text)
            .map_err(|error| failed(error.to_string()))?
            .metadata;
        let (Some(kind), Some(ends)) = (metadata.kind, metadata.ends) else {
            return Err(failed("declares no `kind` and `ends`".to_owned()));
        };
        list.push(ListedPhase {
            action: name.clone(),
            kind,
            ends,
            on_fail: metadata.on_fail,
        });
    }
    Ok(list)
}

/// Reads a phase run folder back for its launcher.
pub fn launch_of(run_dir: &Path) -> Result<Launch, String> {
    let read = |name: &str| -> Result<String, String> {
        let path = run_dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text.trim().to_owned()),
            Err(source) if source.kind() == ErrorKind::NotFound => {
                Err(format!("{} is missing", path.display()))
            }
            Err(source) => Err(format!("cannot read {}: {source}", path.display())),
        }
    };
    let task = read(TASK_FILE)?;
    let size = read(SIZE_FILE)?;
    size.parse::<TaskSize>()
        .map_err(|_| format!("{} is not a size", SIZE_FILE))?;
    Ok(Launch {
        action: ActionName::from(read(ACTION_FILE)?),
        task: task
            .parse()
            .map_err(|_| format!("{TASK_FILE} holds {task:?}, not a task"))?,
        size,
        instance: read(INSTANCE_FILE)?,
    })
}

/// Records a process in the run folder as its `pid`, as a holder. The file is replaced whole, so a
/// reader sees the old record or the new one, never half of either.
pub fn record_pid(run_dir: &Path, holder: &Holder) -> Result<(), String> {
    let path = run_dir.join(PID_FILE);
    let staging = run_dir.join(format!("{PID_FILE}.new"));
    std::fs::write(&staging, holder.to_string())
        .and_then(|()| std::fs::rename(&staging, &path))
        .map_err(|source| format!("cannot write {}: {source}", path.display()))
}

/// The exit code to record for a process that ended: its own `code`, or the shell's `128 + n` when
/// signal `n` killed it, so a phase killed by SIGTERM records `143`.
pub fn exit_code(code: Option<i32>, signal: Option<i32>) -> i32 {
    match (code, signal) {
        (Some(code), _) => code,
        (None, Some(signal)) => 128 + signal,
        (None, None) => 255,
    }
}

/// Records `code` in the run folder as its `exit` file: the decimal code and a newline. It is
/// staged whole and renamed into place, so a reader sees no file or the whole of it, never half.
pub fn record_exit(run_dir: &Path, code: i32) -> Result<(), String> {
    let path = run_dir.join(EXIT_FILE);
    let staging = run_dir.join(format!("{EXIT_FILE}.new"));
    std::fs::write(&staging, format!("{code}\n"))
        .and_then(|()| std::fs::rename(&staging, &path))
        .map_err(|source| format!("cannot write {}: {source}", path.display()))
}

/// The file of a phase run folder that holds what a failed `post-phase` hook said.
pub const POST_PHASE_LOG: &str = "post-phase.log";

/// What the `post-phase` hook gets on stdin (ruling 21): one JSON object naming the task, the
/// phase's action and the exit code the launcher recorded.
pub fn post_phase_input(task: TicketId, action: &ActionName, code: i32) -> String {
    let action = serde_json::Value::from(action.as_str()).to_string();
    format!(
        "{{\"task\": {}, \"action\": {action}, \"exit\": {code}}}\n",
        task.get()
    )
}

/// Logs a failed `post-phase` hook in the run folder, as its `post-phase.log`. It is all the
/// failure changes: the exit code and the standing are not touched.
pub fn record_hook_failure(run_dir: &Path, text: &str) -> Result<(), String> {
    let path = run_dir.join(POST_PHASE_LOG);
    std::fs::write(&path, text)
        .map_err(|source| format!("cannot write {}: {source}", path.display()))
}

/// What a launcher does when its child has ended with `code`: records the exit first, then runs
/// `after`, the steps that follow (post-phase, the tick's ask). A record that fails is returned,
/// and `after` still runs, since a failed write must not swallow the phase's end.
pub fn end(run_dir: &Path, code: i32, after: &dyn Fn()) -> Result<(), String> {
    let recorded = record_exit(run_dir, code);
    after();
    recorded
}

#[cfg(test)]
mod tests;
