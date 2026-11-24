//! `zaf spawn phase <task> <action>`: starts one phase action of a task as its own process
//! (docs/zaf-actions.md §5.1).
//!
//! [`start`] reads the task and, under the spawner's lock, gathers its facts as the tick does
//! ([`gather_one`]) and passes them through the tick's own gate ([`may_start`], which asks the
//! scheduling core where the ticket sits): it refuses, in the gate's words, unless the ticket is
//! in the lane at `action` and the action is not getting nowhere. A run that raised a question
//! still open waits on it, and so does a ticket whose epic is unsigned or questioned; a ticket the
//! tick skips (one that is not a task of an epic, or cannot be read) is refused or failed in the
//! words the tick names it by ([`Skip`]). It then
//! releases the task's claim when the instance holding it has ended ([`release_orphaned`]), as
//! the tick does, takes the next instance number and writes the phase run folder in
//! `.zaf/phases/` through [`record::open`], which documents the folder (the `task` file last, so
//! its time is the start). The caller then starts the launcher on that folder and returns at
//! once; [`Started`] holds what it needs.
//!
//! [`launch_of`] reads that folder back for the launcher: the action, the task, its size and the
//! instance the action runs as. The launcher is the entry point #439 and #440 extend.
//!
//! [`end`] is the launcher's first step once its child has exited: it records the head the phase
//! left in the run folder's `head-end` and the exit code in its `exit` file ([`record_exit`]), last,
//! and only then runs what follows, the post-phase hook and the tick's ask (#439, #440). The code is [`exit_code`]: the child's own, or `128 + n` for a
//! signal `n`. It is a recorded act kept beside the launch record, never a stored queue.
//!
//! [`open_inline`] opens the run folder of a `zaf`-kind action the tick runs in its own process,
//! so an inline run leaves the record a launched phase does, once the gate lets it under the same
//! lock: a second tick that read the same lane finds the run there, running, and runs nothing.
//!
//! A model action [`start`] lets through must also find a free slot under the lock: the slots in
//! use are the project's planning and challenger launches and its live model runs, counted as the
//! tick counts them, so two starts that both read a free slot cannot both take it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Released, SpawnError, lock, release_orphaned};
use crate::app::actions::{ActionKind, ActionName, ListedPhase, PHASES_DIR, parse, resolve};
use crate::app::schedule::gather::{Skip, gather_one, gather_one_in, model_runs};
use crate::app::schedule::record;
use crate::app::schedule::tick::{Refusal, may_start};
use crate::domain::schedule::{GroupBase, HeadEnd, PhaseEnd, TicketFacts};
use crate::domain::{
    CommitId, DEFAULT_TASK_TYPE, Instance, InstanceNumber, RoleName, TaskSize, TicketId,
};
use crate::manifest::Manifest;
use crate::ports::{Board, CiFilter, Forge, Holder, PortError, Vcs};

pub use crate::app::schedule::record::{
    ACTION_FILE, EXIT_FILE, HEAD_FILE, INSTANCE_FILE, LAND_HEAD_FILE, OUTPUT_FILE, PID_FILE,
    POST_PHASE_LOG, SIZE_FILE, TASK_FILE, record_exit, record_launcher, record_pid,
};

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
    /// The task's claim, released before the start because the instance holding it had ended.
    pub released: Vec<Released>,
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
    /// The head Land was derived at, as `ZAF_HEAD` carries it; none for a run of any other phase.
    pub head: Option<String>,
}

/// An inline run's folder, opened for the tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineOpened {
    /// The phase run folder.
    pub run_dir: PathBuf,
    /// The head Land was derived at, as `ZAF_HEAD` carries it; none for a run of any other phase.
    pub land_head: Option<CommitId>,
}

/// The head `action`'s run is told as `ZAF_HEAD`: the pull request's head the facts hold, when
/// `action` is the land phase (the phase that ends at `merged`) and the pull request has a head.
/// It is the head Land was derived at, read from the same facts the gate placed the ticket by, so
/// a head pushed after cannot stand in for it.
fn land_head_of(facts: &TicketFacts, action: &ActionName) -> Option<CommitId> {
    facts
        .phases
        .iter()
        .any(|phase| phase.name == *action && phase.ends.contains(&PhaseEnd::Merged))
        .then(|| facts.head().cloned())
        .flatten()
}

/// What [`start`] reads from.
pub struct Ports<'a> {
    /// The board the task is read from.
    pub board: &'a dyn Board,
    /// The forge the standing's facts are read from.
    pub forge: &'a dyn Forge,
    /// The workflows of the forge as they stand at the integration branch, which say whether a
    /// pull request with no check is expected to get one. None when the forge has no such port:
    /// every check is then expected.
    pub ci_filter: Option<&'a dyn CiFilter>,
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
    /// The release the tickets build toward, which names the integration branch whose checks a
    /// merge waits on (`[branches] integration`); none when the caller has none, and then no
    /// branch is read.
    pub version: Option<&'a str>,
    /// The time of the read, which the gather measures a pull request's age against.
    pub now: SystemTime,
    /// The VCS the trees of the heads are read from, for R18's count by tree; none when the
    /// caller has none, and then getting nowhere counts by commit id.
    pub vcs: Option<&'a dyn Vcs>,
}

/// Checks, through the tick's gate, that `action` may start on `task` now, releases the task's
/// claim when the instance holding it has ended, takes the next instance number and writes the
/// phase run folder. Nothing is started.
///
/// The gate is [`may_start`]: the ticket is in the lane at `action`, no phase of it runs, nothing
/// holds it waiting (its epic, a question, a ticket it depends on, the checks at its head), its
/// wait has not failed with no `on-fail`, and `action` is not getting nowhere. A refusal is
/// [`PhaseError::Refused`] in the gate's words. A ticket the tick skips is refused or failed in the
/// words the tick names it by ([`Skip`]): one that is not a task of an epic is refused, `#4 cannot
/// start: it is not a task of an epic`, and one something about which cannot be read (its pull
/// request record included) fails, `#4 cannot be read: <why>`. A model action also needs a free
/// slot: with the project's planning and challenger launches and its live model runs already
/// filling `[spawn] slots` it is refused, `#4 cannot start implement: every slot is in use (1 of
/// 1)`. The check, the release, the folder and `launch` all run under the spawner's lock, so two
/// starts never both pass, and two model starts never both take the last slot. `launch` starts
/// the launcher on the folder and returns its process, which is recorded as the run's `pid`
/// before the lock is let go: the run reads as running from the moment it is written, and the
/// launcher replaces the record with the action's own process. A launcher that recorded the
/// action first keeps its record: the launcher's process is written only while no `pid` is there.
pub fn start(
    ports: &Ports<'_>,
    task: TicketId,
    action: &str,
    launch: &dyn Fn(&Started) -> Result<Holder, String>,
) -> Result<Started, PhaseError> {
    let ticket = ports.board.task(task)?;
    let size = ticket
        .task_body()
        .and_then(|body| body.size())
        .map_err(|error| PhaseError::Refused(format!("{task} is not a task: {error}")))?;
    let task_type = ticket
        .kind
        .as_ref()
        .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str());
    if ports.manifest.phase_list(task_type).is_none() {
        return Err(PhaseError::Refused(format!(
            "[phases] has no list for the type {task_type}"
        )));
    }

    let _held = lock(ports.state_dir)?;
    let action_name = ActionName::from(action);
    // A ticket the tick skips is refused, or fails, in the words the tick skips it with.
    let (facts, beside) = match gather_one(ports, task) {
        Ok(Some(gathered)) => gathered,
        Ok(None) => return Err(PhaseError::Refused(Refusal::NotOpen.reason(task, action))),
        Err(Skip::CannotRun(reason)) => {
            return Err(PhaseError::Refused(format!(
                "{task} cannot start: {reason}"
            )));
        }
        Err(Skip::Unread(reason)) => {
            return Err(PhaseError::Failed(format!(
                "{task} cannot be read: {reason}"
            )));
        }
    };
    let kind = may_start(&facts, &beside, &action_name, SystemTime::now())
        .map_err(|refusal| PhaseError::Refused(refusal.reason(task, action)))?;

    let (launches, runs) = super::state::derive(ports.state_dir, ports.alive)?;
    if kind == ActionKind::Model {
        let slots = usize::try_from(ports.manifest.spawn.slots).unwrap_or(usize::MAX);
        let in_use = launches
            .running()
            .saturating_add(model_runs(ports).map_err(PhaseError::Failed)?);
        if in_use >= slots {
            return Err(PhaseError::Refused(format!(
                "{task} cannot start {action}: every slot is in use ({in_use} of {slots})"
            )));
        }
    }
    let released = release_orphaned(ports.board, ports.state_dir, &runs, ports.alive, Some(task))?;

    let mut state = super::state::load(ports.state_dir)?;
    let number = state.next_instance;
    let instance = Instance {
        role: RoleName::from(action),
        epic: beside.epic,
        number: InstanceNumber::from(number),
    };
    state.next_instance = number.saturating_add(1);
    super::save(ports.state_dir, &state)?;

    let head = facts.head().cloned();
    let run_dir = record::open(
        &ports.state_dir.join(PHASES_DIR),
        &record::Opening {
            task,
            action: &action_name,
            head: head.as_ref(),
            size,
            instance: &instance,
            inline: None,
        },
    )
    .map_err(PhaseError::Failed)?;
    let land = land_head_of(&facts, &action_name);
    if let Some(land) = &land {
        record::mark_land(&run_dir, land).map_err(PhaseError::Failed)?;
    }
    let started = Started {
        run_dir,
        instance: instance.to_string(),
        released,
    };
    let holder = launch(&started).map_err(PhaseError::Failed)?;
    // Only while no record is there: a launcher that was faster has recorded the action (#223).
    record_launcher(&started.run_dir, &holder).map_err(PhaseError::Failed)?;
    Ok(started)
}

/// The task's phase list with what each action declares. An action that cannot be found or read,
/// or that declares no kind, cannot be a phase.
pub fn listed(
    root: &Path,
    home: &Path,
    names: &[ActionName],
) -> Result<Vec<ListedPhase>, PhaseError> {
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
        head: record::land_head(run_dir),
    })
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

/// What the `post-phase` hook gets on stdin (ruling 21): one JSON object naming the task, the
/// phase's action and the exit code the launcher recorded, as
/// `{"task": <n>, "action": "<name>", "exit": <code>}`.
pub fn post_phase_input(task: TicketId, action: &ActionName, code: i32) -> String {
    let action = serde_json::Value::from(action.as_str()).to_string();
    format!(
        "{{\"task\": {}, \"action\": {action}, \"exit\": {code}}}\n",
        task.get()
    )
}

/// Logs a failed `post-phase` hook in the run folder, in its `post-phase.log`. It is all the
/// failure changes: the exit code and the standing are not touched.
pub fn record_hook_failure(run_dir: &Path, text: &str) -> Result<(), String> {
    log_after_phase(run_dir, text)
}

/// Appends `text` to the run folder's `post-phase.log`, what went wrong once the phase had ended:
/// a failed `post-phase` hook, a tick that could not be asked for. The launcher has no stderr
/// anyone reads, and the run folder outlives it.
pub fn log_after_phase(run_dir: &Path, text: &str) -> Result<(), String> {
    let path = run_dir.join(POST_PHASE_LOG);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()))
        .map_err(|source| format!("cannot write {}: {source}", path.display()))
}

/// What a launcher does when its child has ended with `code`, having left `head_end`: records the
/// head it left, then the exit, then runs `after`, the steps that follow (post-phase, the tick's
/// ask); see [`record::end`]. A record that fails is returned, and `after` still runs, since a
/// failed write must not swallow the phase's end.
pub fn end(run_dir: &Path, code: i32, head_end: &HeadEnd, after: &dyn Fn()) -> Result<(), String> {
    record::end(run_dir, code, head_end, after)
}

/// Opens the run folder of a `zaf`-kind action the tick runs inline for `task` (docs/zaf-actions.md
/// §5.1), once the gate lets it. Under the spawner's lock it gathers the ticket again
/// ([`gather_one`]) and asks [`may_start`], so a tick that read the lane before another tick's run
/// of the same action began finds that run there, running, and runs nothing. It then takes the
/// next instance number, as [`start`] does, and opens the folder with no `pid`, since the run is a
/// step of the tick's own process, `this`, which the folder's `inline` mark names: the run reads as
/// running from its first moment until its `exit` is written. Returns the folder, which
/// [`record::end`] ends once the action has run, and, for the land phase, the head Land was derived
/// at, which the action gets as `ZAF_HEAD` ([`InlineOpened`]).
///
/// A ticket the gate holds, one no longer in the lane at `action`, one whose action is not of the
/// `zaf` kind, and one that cannot be read now are [`PhaseError::Refused`], in the gate's words: the
/// action is not run, and the next tick places the ticket again. A folder that cannot be written is
/// [`PhaseError::Failed`].
///
/// `group` is what the tick's group of merges remembers, in memory ([`GroupBase`]): the gate
/// judges R29 at the group's base while the integration branch is still at the group's own last
/// commit, so checks pending at a commit the group just made do not hold its next merge (F30).
/// With none, or at any other head, it judges as the branch reads.
pub fn open_inline(
    ports: &Ports<'_>,
    task: TicketId,
    action: &ActionName,
    this: &Holder,
    group: Option<&GroupBase>,
) -> Result<InlineOpened, PhaseError> {
    let _held = lock(ports.state_dir)?;
    let (facts, beside) = match gather_one_in(ports, task, group) {
        Ok(Some(gathered)) => gathered,
        Ok(None) => {
            return Err(PhaseError::Refused(
                Refusal::NotOpen.reason(task, action.as_str()),
            ));
        }
        Err(Skip::CannotRun(reason)) => {
            return Err(PhaseError::Refused(format!(
                "{task} cannot start: {reason}"
            )));
        }
        Err(Skip::Unread(reason)) => {
            return Err(PhaseError::Refused(format!(
                "{task} cannot be read: {reason}"
            )));
        }
    };
    match may_start(&facts, &beside, action, SystemTime::now()) {
        Ok(ActionKind::Zaf) => {}
        Ok(_) => {
            return Err(PhaseError::Refused(format!(
                "{action} is not a zaf-kind action of {task}"
            )));
        }
        Err(refusal) => {
            return Err(PhaseError::Refused(refusal.reason(task, action.as_str())));
        }
    }
    let mut state = super::state::load(ports.state_dir)?;
    let number = state.next_instance;
    let instance = Instance {
        role: RoleName::from(action.as_str()),
        epic: beside.epic,
        number: InstanceNumber::from(number),
    };
    state.next_instance = number.saturating_add(1);
    super::save(ports.state_dir, &state)?;
    let run_dir = record::open(
        &ports.state_dir.join(PHASES_DIR),
        &record::Opening {
            task,
            action,
            head: facts.head(),
            size: facts.size,
            instance: &instance,
            inline: Some(this),
        },
    )
    .map_err(PhaseError::Failed)?;
    let land_head = land_head_of(&facts, action);
    if let Some(land) = &land_head {
        record::mark_land(&run_dir, land).map_err(PhaseError::Failed)?;
    }
    Ok(InlineOpened { run_dir, land_head })
}

#[cfg(test)]
mod tests;
