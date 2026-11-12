//! `zaf spawn --once`: one tick of the spawner: planning and challenger launches, each open
//! epic getting the one its stage calls for ([`tick`]); a challenged epic's tasks are built by
//! their phases, never by a launched instance; the release branch is not built.
//!
//! Nothing the tick plans from is kept in step by hand: each read derives it.
//! Whether a launch still runs comes from its run folder and the process table
//! ([`runs`]), which launches still hold slots follows from that, a claim counts
//! only while its instance runs (unless its task has a recorded pull request or a running
//! phase, see [`held`]), and the board answers the rest. `.zaf/spawn.state`
//! keeps only the instance counter.
//!
//! [`prepare`] records `lost` for a launch that ended with nobody there to record
//! it, releases every task claimed by an instance that no longer runs, reads the
//! board and the guard, plans the tick with [`tick::plan`] and reserves each planned
//! launch in its run folder, held by the tick until the instance starts. [`launch`]
//! then runs one through the Runner port: it records the launch on the epic, runs
//! the instance's role in `.zaf/runs/<instance slug>/`, where the runner records the
//! instance's own process, with `ZAF_RUN` naming that folder and `ZAF_SIZE` its size
//! ([`launch_size`]) in its environment, keeps the instance's output as `report.md` there and
//! records the exit in the run folder and on the epic. Launch and exit are HTML
//! comments on the epic: `<!-- zaf:launch <instance> -->` and
//! `<!-- zaf:exit <instance> <status> -->`, the status being the exit code, `error`
//! when the runner could not run, `timeout` when the run passed its time bound, or
//! `lost`. A timed-out instance's claims go back to the board at its exit.
//!
//! The phase tick's step 3 ([`inline`]) runs every ready `zaf`-kind action with `zaf do`, one
//! after another, at no cost to the budget; [`outcome::TickOutcome`] is what a tick did, its
//! `ran` filled from there. Step 5 ([`question`]) raises one question on each task whose wait
//! failed with no `on-fail`, at no cost either; its tasks fill `questions`. Step 4 ([`waits`])
//! launches every ready `wait` action detached with `zaf spawn phase`, at no cost and never
//! waiting for one; the waits it launched fill `waits`, and they are not the tick's one model start.
//! Step 6 ([`start`]) starts at most one model phase, the longest ready that fits the budget,
//! never waiting for it; it fills `started`. [`phase_tick`] assembles them: it derives the
//! standing of every claimed or ready task whose type has a `[phases]` list from the forge and
//! the phase run folders, storing none of it, and runs steps 3 to 6 on them in that order into
//! one outcome. Such a task is the phase steps' alone: the tick's plan launches no instance
//! for a task. Step 7 ([`post_tick`]) runs the `post-tick` hook with the whole
//! outcome as JSON on its stdin.
//!
//! A dry run ([`plan_only`] and [`dry_run`]) plans the same tick and describes
//! each launch it would make, writing nothing: no state, no run folder, no comment.
//!
//! [`phase`] starts one phase action of a task (`zaf spawn phase`): it takes the next instance
//! number from the same counter, under the same lock, and writes a phase run folder in
//! `.zaf/phases/` rather than `.zaf/runs/`.

pub mod blockers;
pub mod failures;
pub mod inline;
pub mod outcome;
pub mod phase;
pub mod phase_tick;
pub mod post_tick;
pub mod question;
pub mod runs;
pub mod start;
mod state;
pub mod tick;
pub mod waits;
pub mod watch;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use inline::{TickTask, run_inline, run_zaf_actions};
pub use outcome::{Ran, Started, TickOutcome};
pub use phase_tick::{
    Gathered, PhaseTask, PhaseTick, Steps, Unreadable, gather, phased, run_steps,
};
pub use question::{ask_failed_waits, ask_outcome, question_text};
use runs::Run;
pub use start::{
    EPIC_SIZE, InUse, PlannedModel, STARVE_AFTER, TaskStanding, start_outcome, start_planned,
};
pub use state::{SpawnState, derive, load, save};
pub use tick::{
    EpicCandidate, EpicSkip, NOT_BUILT, PLANNING_ROLE, PastLaunch, Plan, PlannedLaunch, Settings,
    SlotCount, Snapshot,
};
pub use waits::{launch_outcome, launch_waits};

use super::actions::{PHASES_DIR, SliceRecords, running_tasks};
use super::guard::{GUARD_FILE, Level};
use crate::domain::{CHALLENGER_ROLE, Instance, Profile, RoleName, TaskSize, TicketId};
use crate::manifest::{Manifest, ManifestError};
use crate::ports::{
    Board, ClaimRecord, Holder, NewQuestion, PortError, RUN_VAR, RunRequest, Runner, SIZE_VAR,
};

/// The one-line launch prompt of an instance the tick launches (planning, the
/// challenger), with `{instance}`, `{role}`, `{epic}`, `{project}` and `{brief}` placeholders.
pub const EPIC_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/spawn/epic-prompt.txt"
));

/// The planning instance's standing brief when the project keeps none of its own.
pub const PLANNING_BRIEF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/spawn/planning-brief.md"
));

/// The challenger's standing brief when the project keeps none of its own.
pub const CHALLENGER_BRIEF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/spawn/challenger-brief.md"
));

/// The pack's standing brief for `role`: the challenger's, or the planning instance's for any
/// other role the tick launches.
pub fn pack_brief(role: &RoleName) -> &'static str {
    match role.as_str() {
        CHALLENGER_ROLE => CHALLENGER_BRIEF,
        _ => PLANNING_BRIEF,
    }
}

/// The folder under the state folder holding one run folder per instance.
pub const RUNS_DIR: &str = "runs";

/// The folder under the state folder where a project keeps its own standing briefs.
pub const ROLES_DIR: &str = "roles";

/// The instance's output inside its run folder.
pub const REPORT_FILE: &str = "report.md";

/// The pack's brief, written into the run folder when the project has none.
pub const BRIEF_FILE: &str = "brief.md";

/// Why a tick could not be planned or a launch recorded.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// The board failed.
    #[error(transparent)]
    Port(#[from] PortError),
    /// A state file is there but unreadable.
    #[error("cannot read {path}: {reason}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// A state file or the run folder could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// The part of `.zaf/guard.json` the spawner reads.
#[derive(Debug, serde::Deserialize)]
struct GuardLevel {
    level: Level,
}

/// The guard level from `<state dir>/guard.json`; green when the file is absent.
pub fn read_guard(state_dir: &Path) -> Result<Level, SpawnError> {
    let path = state_dir.join(GUARD_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Level::Green),
        Err(source) => {
            return Err(SpawnError::Read {
                path,
                reason: source.to_string(),
            });
        }
    };
    serde_json::from_str::<GuardLevel>(&text)
        .map(|guard| guard.level)
        .map_err(|error| SpawnError::Read {
            path,
            reason: error.to_string(),
        })
}

/// Reads what the tick needs from the board: the ready tasks, the open tasks their blockers
/// lead to and the blockers that can never close (see [`blockers`]); the open epics and the
/// issues an open question waits on; and, from `runs`, every launch recorded so far.
pub fn snapshot(board: &dyn Board, runs: &[Run]) -> Result<Snapshot, PortError> {
    let ready = board.ready()?;
    let found = blockers::read(board, &ready)?;
    let mut open = found.graph;
    for ticket in &ready {
        open.remove(&ticket.id);
    }
    let questioned = board
        .questions()?
        .iter()
        .filter_map(|question| NewQuestion::raised_on(question.body.as_str()))
        .collect();
    Ok(Snapshot {
        ready,
        open,
        dead: found.dead,
        epics: board.epics()?,
        questioned,
        launched: runs.iter().filter_map(past_launch).collect(),
    })
}

/// A run folder's launch, when it records the instance and a readable reservation time.
fn past_launch(run: &Run) -> Option<PastLaunch> {
    let record = run.record.as_ref()?;
    Some(PastLaunch {
        instance: record.instance.parse().ok()?,
        reserved_at: humantime::parse_rfc3339_weak(&record.reserved_at).ok()?,
    })
}

/// What [`prepare`] did before any launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The tick's plan; its launches are reserved in their run folders.
    pub plan: Plan,
    /// Launches that ended with nobody there to record it, each recorded as `lost`.
    pub lost: Vec<String>,
    /// The tasks claimed by an instance that no longer runs, each released.
    pub released: Vec<Released>,
}

/// A task released from an instance that no longer runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Released {
    /// The task.
    pub task: TicketId,
    /// The instance that held it.
    pub instance: Instance,
}

/// The file a tick holds locked from reading the counter to saving it, so two ticks never
/// plan the same instance number or the same slot.
pub const SPAWN_LOCK_FILE: &str = "spawn.lock";

/// Takes the spawner's lock, waiting for a tick that holds it; the OS drops it with the file.
fn lock(state_dir: &Path) -> Result<std::fs::File, SpawnError> {
    let (file, path) = open_lock(state_dir)?;
    file.lock()
        .map(|()| file)
        .map_err(|source| SpawnError::Write { path, source })
}

/// The lock file, created when it is missing, and its path; nothing locked yet.
fn open_lock(state_dir: &Path) -> Result<(std::fs::File, PathBuf), SpawnError> {
    let path = state_dir.join(SPAWN_LOCK_FILE);
    std::fs::create_dir_all(state_dir)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
        })
        .map(|file| (file, path.clone()))
        .map_err(|source| SpawnError::Write { path, source })
}

/// Whether another tick holds the spawner's lock right now, found without waiting for it: the
/// lock is taken and let go at once when it is free. A tick that takes it between this look and its
/// own `prepare` is waited for there, as plain `--once` waits (`zaf spawn --once --no-wait`).
pub fn lock_is_held(state_dir: &Path) -> Result<bool, SpawnError> {
    let (file, path) = open_lock(state_dir)?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(source)) => Err(SpawnError::Write { path, source }),
    }
}

/// The tasks held by the forge's facts or by a running phase, whatever instance claimed them:
/// those with a recorded pull request (`.zaf/slices/<task>.pr`) and those with a live phase
/// process (`.zaf/phases/`, #437). A phase run folder that cannot be read is an error, never a
/// guess, so no claim is released on a read that failed.
pub fn held(
    state_dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<BTreeSet<TicketId>, SpawnError> {
    let phases = state_dir.join(PHASES_DIR);
    let mut held = SliceRecords::in_state_dir(state_dir).tasks_with_pr();
    held.extend(
        running_tasks(&phases, alive).map_err(|reason| SpawnError::Read {
            path: phases,
            reason,
        })?,
    );
    Ok(held)
}

/// `orphaned(claims, runs, alive, held)`: the claims held by an instance this machine launched
/// that no longer runs: its run folder
/// is there and its holder is dead. A claim by an instance with no run folder here (one
/// started by hand, or on another machine) is not this machine's to judge, so it stays.
/// A task in `held` (see [`held`]) is not orphaned: a task with a recorded pull request is held
/// by its forge facts and one with a running phase by that phase, so its claim stays until the
/// task is done (the ruling on #426).
pub fn orphaned<'a>(
    claims: &'a [ClaimRecord],
    runs: &[Run],
    alive: &dyn Fn(&Holder) -> bool,
    held: &BTreeSet<TicketId>,
) -> Vec<&'a ClaimRecord> {
    let gone: BTreeSet<&str> = runs
        .iter()
        .filter(|run| !run.running(alive))
        .map(|run| run.slug.as_str())
        .collect();
    claims
        .iter()
        .filter(|claim| gone.contains(slug(&claim.instance).as_str()))
        .filter(|claim| !held.contains(&claim.task))
        .collect()
}

/// Plans one tick and reserves its launches, each held by `holder`, the process that will
/// run them, until its runner starts the instance.
///
/// Before planning, the run folders and the process table are read: a launch that ended
/// with nobody to record it gets `lost` on its epic and in its run folder, and every task
/// claimed by an instance that no longer runs is released, so another instance can claim it.
pub fn prepare(
    board: &dyn Board,
    state_dir: &Path,
    manifest: &Manifest,
    alive: &dyn Fn(&Holder) -> bool,
    holder: &Holder,
) -> Result<Prepared, SpawnError> {
    let _held = lock(state_dir)?;
    let (mut state, found) = state::derive(state_dir, alive)?;
    let mut lost = Vec::new();
    for run in found.iter().filter(|run| run.lost(alive)) {
        let Some(instance) = run.instance() else {
            continue;
        };
        let name = instance.to_string();
        board.comment(instance.epic, &exit_marker(&name, runs::LOST))?;
        runs::finish(&run.dir, runs::LOST)?;
        lost.push(name);
    }
    let claims = board.claims()?;
    let mut released = Vec::new();
    let held = held(state_dir, alive)?;
    for claim in orphaned(&claims, &found, alive, &held) {
        board.release(claim.task, &claim.instance)?;
        released.push(Released {
            task: claim.task,
            instance: claim.instance.clone(),
        });
    }
    let snapshot = snapshot(board, &found)?;
    let level = read_guard(state_dir)?;
    let plan = tick::plan(&snapshot, &state, level, Settings::from(manifest));
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    for planned in &plan.launches {
        runs::reserve(state_dir, planned, holder, &now)?;
        state.next_instance = state
            .next_instance
            .max(planned.instance.number.get().saturating_add(1));
    }
    save(state_dir, &state)?;
    Ok(Prepared {
        plan,
        lost,
        released,
    })
}

/// Releases every task in `claims` that `instance` holds, so another instance can claim it.
fn release_held(
    board: &dyn Board,
    claims: &[ClaimRecord],
    instance: &Instance,
) -> Result<Vec<Released>, PortError> {
    let mut released = Vec::new();
    for claim in claims.iter().filter(|claim| &claim.instance == instance) {
        board.release(claim.task, instance)?;
        released.push(Released {
            task: claim.task,
            instance: instance.clone(),
        });
    }
    Ok(released)
}

/// The comment that records a launch on its epic.
pub fn launch_marker(instance: &str) -> String {
    format!("<!-- zaf:launch {instance} -->")
}

/// The comment that records an exit on its epic.
pub fn exit_marker(instance: &str, status: &str) -> String {
    format!("<!-- zaf:exit {instance} {status} -->")
}

/// The run folder's name: the instance with spaces and `#` turned into single dashes.
pub fn slug(instance: &Instance) -> String {
    let mut slug = String::new();
    for character in instance.to_string().chars() {
        let character = if character == ' ' || character == '#' {
            '-'
        } else {
            character
        };
        if !(character == '-' && slug.ends_with('-')) {
            slug.push(character);
        }
    }
    slug
}

/// The launch prompt: one line naming the instance, the role, the epic and the project,
/// pointing at the standing brief ([`EPIC_PROMPT`]).
pub fn prompt(launch: &PlannedLaunch, project: &str, brief: &Path) -> String {
    EPIC_PROMPT
        .trim_end()
        .replace("{instance}", &launch.instance.to_string())
        .replace("{role}", launch.instance.role.as_str())
        .replace("{epic}", &launch.instance.epic.to_string())
        .replace("{project}", project)
        .replace("{brief}", &brief.display().to_string())
}

/// Where a launch runs and what it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The state folder, `<root>/.zaf`.
    pub state_dir: PathBuf,
    /// The project's name, for the prompt.
    pub project: String,
    /// The launched role's profile.
    pub profile: Profile,
    /// The environment the launch's process runs with, on its [`RunRequest`].
    pub env: BTreeMap<String, String>,
}

/// The manifest's `[env]` table for a launch: `{root}` is the absolute project root and
/// `{version}` the release the instances build. The runner sets these on top of the
/// inherited environment, and a name set here wins over an inherited one.
pub fn manifest_env(
    manifest: &Manifest,
    root: &Path,
    version: &str,
) -> Result<BTreeMap<String, String>, ManifestError> {
    manifest.env.resolve(root, Some(version))
}

/// How a launched instance ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchExit {
    /// It ran and exited with this code.
    Code(i32),
    /// The runner could not run it.
    Error(String),
    /// The run passed its time bound and was stopped; the runner's message.
    TimedOut(String),
}

impl LaunchExit {
    /// The status word in the exit comment: the exit code, `timeout` or `error`.
    pub fn status(&self) -> String {
        match self {
            Self::Code(code) => code.to_string(),
            Self::TimedOut(_) => "timeout".to_owned(),
            Self::Error(_) => "error".to_owned(),
        }
    }

    /// Whether the run passed its time bound and was stopped.
    pub fn timed_out(&self) -> bool {
        matches!(self, Self::TimedOut(_))
    }

    /// What went wrong, for a run that did not exit on its own.
    pub fn problem(&self) -> Option<&str> {
        match self {
            Self::Code(_) => None,
            Self::Error(reason) | Self::TimedOut(reason) => Some(reason),
        }
    }
}

/// One finished launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    /// The instance.
    pub instance: Instance,
    /// How it ended.
    pub exit: LaunchExit,
    /// Its run folder, holding `report.md`.
    pub run_dir: PathBuf,
}

/// Runs one reserved launch to its exit and records how it ended in its run folder.
///
/// The standing brief is `<state dir>/roles/<role>.md` when present, else the
/// pack's, written into the run folder. The runner writes the instance's own
/// process into the run folder's `pid` once it starts, so the launch is held by
/// the instance, not by this tick. A runner that cannot run is recorded
/// as an `error` exit, not returned as an error; a board or file failure is.
/// A run that timed out is recorded as a `timeout` exit and every task the
/// instance still held is released; any other exit keeps its claims.
pub fn launch(
    board: &dyn Board,
    runner: &dyn Runner,
    context: &LaunchContext,
    planned: &PlannedLaunch,
) -> Result<Launched, SpawnError> {
    let instance = planned.instance.to_string();
    let run_dir = run_dir(context, planned);
    let request = match start(board, context, planned) {
        Ok(request) => request,
        Err(error) => {
            runs::finish(&run_dir, &LaunchExit::Error(String::new()).status())?;
            return Err(error);
        }
    };
    let (exit, written) = match runner.run(&request) {
        Ok(output) => {
            let report = run_dir.join(REPORT_FILE);
            let written =
                std::fs::write(&report, &output.transcript).map_err(|source| SpawnError::Write {
                    path: report,
                    source,
                });
            (LaunchExit::Code(output.exit_code), written)
        }
        Err(error @ PortError::TimedOut { .. }) => {
            (LaunchExit::TimedOut(error.to_string()), Ok(()))
        }
        Err(error) => (LaunchExit::Error(error.to_string()), Ok(())),
    };
    // The end is recorded before anything that can fail, so the slot is free whatever follows.
    runs::finish(&run_dir, &exit.status())?;
    written?;
    board.comment(
        planned.instance.epic,
        &exit_marker(&instance, &exit.status()),
    )?;
    if exit.timed_out() {
        // A timed-out instance is as good as lost: its tasks go back to the board.
        release_held(board, &board.claims()?, &planned.instance)?;
    }
    Ok(Launched {
        instance: planned.instance.clone(),
        exit,
        run_dir,
    })
}

/// A way to get a board and a runner of one's own for a role, for a launch thread.
pub type PortsFn<'a> = dyn Fn(&RoleName) -> Result<LaunchPorts, PortError> + Sync + 'a;

/// The board and the runner one launch thread works with.
pub type LaunchPorts = (Box<dyn Board>, Box<dyn Runner>);

/// Runs the planned launches in parallel, one thread each, and joins them all.
///
/// Each job is a launch with the context it runs under (its own environment).
/// A thread builds its own board and runner with `ports`, so the ports need not
/// be shared; each launch writes only its own run folder. The results come back
/// in the jobs' order. A thread that cannot get its ports, or that panics,
/// records an `error` exit in its run folder and reports the failure.
pub fn launch_all(
    jobs: &[(&PlannedLaunch, LaunchContext)],
    ports: &PortsFn<'_>,
) -> Vec<Result<Launched, SpawnError>> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(planned, context)| {
                scope.spawn(move || match ports(&planned.instance.role) {
                    Ok((board, runner)) => {
                        launch(board.as_ref(), runner.as_ref(), context, planned)
                    }
                    Err(error) => {
                        let status = LaunchExit::Error(String::new()).status();
                        runs::finish(&run_dir(context, planned), &status)?;
                        Err(SpawnError::Port(error))
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .zip(jobs)
            .map(|(handle, (planned, context))| {
                handle.join().unwrap_or_else(|_| {
                    // A panicked launch still records its end, so its slot is free.
                    let status = LaunchExit::Error(String::new()).status();
                    runs::finish(&run_dir(context, planned), &status)?;
                    Err(SpawnError::Port(PortError::Backend(
                        "a launch thread panicked".to_owned(),
                    )))
                })
            })
            .collect()
    })
}

/// A launch's run folder.
fn run_dir(context: &LaunchContext, planned: &PlannedLaunch) -> PathBuf {
    context
        .state_dir
        .join(RUNS_DIR)
        .join(slug(&planned.instance))
}

/// Writes the brief, marks the launch started and records it on the epic.
fn start(
    board: &dyn Board,
    context: &LaunchContext,
    planned: &PlannedLaunch,
) -> Result<RunRequest, SpawnError> {
    let instance = planned.instance.to_string();
    let (run_dir, brief, request) = request_for(context, planned);
    std::fs::create_dir_all(&run_dir).map_err(|source| SpawnError::Write {
        path: run_dir.clone(),
        source,
    })?;
    if brief.starts_with(&run_dir) {
        let text = pack_brief(&planned.instance.role);
        std::fs::write(&brief, text).map_err(|source| SpawnError::Write {
            path: brief.clone(),
            source,
        })?;
    }
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    runs::mark_started(&run_dir, &now)?;
    board.comment(planned.instance.epic, &launch_marker(&instance))?;
    Ok(request)
}

/// A launch's run folder, its standing brief and its request, touching nothing.
///
/// The brief is `<state dir>/roles/<role>.md` when present, else the pack's, which
/// [`start`] writes into the run folder. The runner records the instance's process in
/// the run folder's `pid`.
fn request_for(context: &LaunchContext, planned: &PlannedLaunch) -> (PathBuf, PathBuf, RunRequest) {
    let run_dir = run_dir(context, planned);
    let role = &planned.instance.role;
    let own = context
        .state_dir
        .join(ROLES_DIR)
        .join(format!("{}.md", role.as_str()));
    let brief = if own.is_file() {
        own
    } else {
        run_dir.join(BRIEF_FILE)
    };
    let mut env = context.env.clone();
    env.extend(run_env(&run_dir, role));
    let request = RunRequest {
        role: role.clone(),
        profile: context.profile,
        prompt: prompt(planned, &context.project, &brief),
        dir: run_dir.clone(),
        env,
        holder_file: Some(run_dir.join(runs::HOLDER_FILE)),
    };
    (run_dir, brief, request)
}

/// The size a launch of `role` carries in [`SIZE_VAR`]: `XS` for a planning or challenger
/// instance; none for any other role.
pub fn launch_size(role: &RoleName) -> Option<TaskSize> {
    match role.as_str() {
        PLANNING_ROLE | CHALLENGER_ROLE => Some(TaskSize::XS),
        _ => None,
    }
}

/// What marks a launch's process as a zaf instance for the machine read: [`RUN_VAR`], the
/// absolute path of its run folder, and [`SIZE_VAR`] when its role has a size.
fn run_env(run_dir: &Path, role: &RoleName) -> BTreeMap<String, String> {
    let absolute = std::path::absolute(run_dir).unwrap_or_else(|_| run_dir.to_owned());
    let mut env = BTreeMap::from([(RUN_VAR.to_owned(), absolute.display().to_string())]);
    if let Some(size) = launch_size(role) {
        env.insert(SIZE_VAR.to_owned(), size.name().to_owned());
    }
    env
}

/// Plans one tick the way [`prepare`] does, changing nothing: launches that ended with
/// nobody to record it are returned as lost, not recorded, and nothing is reserved.
pub fn plan_only(
    board: &dyn Board,
    state_dir: &Path,
    manifest: &Manifest,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Prepared, SpawnError> {
    let (state, found) = state::derive(state_dir, alive)?;
    let lost = found
        .iter()
        .filter(|run| run.lost(alive))
        .filter_map(Run::instance)
        .map(|instance| instance.to_string())
        .collect();
    let snapshot = snapshot(board, &found)?;
    let level = read_guard(state_dir)?;
    let plan = tick::plan(&snapshot, &state, level, Settings::from(manifest));
    Ok(Prepared {
        plan,
        lost,
        released: Vec::new(),
    })
}

/// What a runner would run for a request: the command line and the environment it sets.
pub type DescribeFn<'a> =
    dyn Fn(&RunRequest) -> Result<(String, BTreeMap<String, String>), PortError> + 'a;

/// One launch a dry run would make.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DryLaunch {
    /// The instance.
    pub instance: String,
    /// Its epic.
    pub epic: u64,
    /// The profile it would run in.
    pub profile_dir: PathBuf,
    /// The environment its process would get, every layer applied.
    pub env: BTreeMap<String, String>,
    /// The full command line.
    pub command: String,
}

impl DryLaunch {
    /// The launch as indented lines under a heading.
    pub fn render_text(&self) -> String {
        let mut lines = vec![
            format!("would launch {}", self.instance),
            format!("  epic #{}", self.epic),
            format!("  profile {}", self.profile_dir.display()),
        ];
        lines.extend(
            self.env
                .iter()
                .map(|(name, value)| format!("  env {name}={value}")),
        );
        lines.push(format!("  command {}", self.command));
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

/// Describes each planned launch with `describe`, launching nothing and writing nothing.
pub fn dry_run(
    jobs: &[(&PlannedLaunch, LaunchContext)],
    profile_dir: &dyn Fn(Profile) -> PathBuf,
    describe: &DescribeFn<'_>,
) -> Result<Vec<DryLaunch>, PortError> {
    jobs.iter()
        .map(|(planned, context)| {
            let (_, _, request) = request_for(context, planned);
            let (command, env) = describe(&request)?;
            Ok(DryLaunch {
                instance: planned.instance.to_string(),
                epic: planned.instance.epic.get(),
                profile_dir: profile_dir(context.profile),
                env,
                command,
            })
        })
        .collect()
}

/// A role's profile from the manifest; the pack's `build` when the role is absent.
pub fn role_profile(manifest: &Manifest, role: &RoleName) -> Profile {
    manifest
        .roles
        .get(role)
        .map_or(Profile::Build, |section| section.profile)
}

impl Plan {
    /// The plan as text: the slots, the branches not built, every skip.
    pub fn render_text(&self) -> String {
        let slots = &self.slots;
        let mut lines = vec![format!(
            "slots {} of {} (guard {}), {} running, {} free",
            slots.allowed, slots.total, slots.level, slots.running, slots.free
        )];
        lines.extend(
            NOT_BUILT
                .iter()
                .map(|branch| format!("{branch}: skipped: not built")),
        );
        for (epic, skip) in &self.epics {
            lines.push(format!("{epic} skipped: {}", epic_reason(*skip)));
        }
        for cycle in &self.cycles {
            lines.push(format!("blocker cycle: {}", blockers::render_cycle(cycle)));
        }
        lines.extend(self.dead.iter().map(ToString::to_string));
        for launch in &self.launches {
            lines.push(format!("launching {}", launch.instance));
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    /// The plan as JSON, with the finished launches and the lost instances.
    pub fn to_json(&self, launched: &[Launched], lost: &[String]) -> serde_json::Value {
        let launched: Vec<serde_json::Value> = launched
            .iter()
            .map(|launch| {
                let error = launch.exit.problem();
                serde_json::json!({
                    "instance": launch.instance.to_string(),
                    "epic": launch.instance.epic.get(),
                    "exit": launch.exit.status(),
                    "error": error,
                    "report": launch.run_dir.join(REPORT_FILE),
                })
            })
            .collect();
        let epics: Vec<serde_json::Value> = self
            .epics
            .iter()
            .map(|(epic, skip)| {
                serde_json::json!({ "epic": epic.get(), "reason": <&'static str>::from(*skip) })
            })
            .collect();
        let branches: Vec<serde_json::Value> = NOT_BUILT
            .iter()
            .map(|branch| serde_json::json!({ "branch": branch, "reason": "not built" }))
            .collect();
        let cycles: Vec<Vec<u64>> = self
            .cycles
            .iter()
            .map(|cycle| blockers::cycle_numbers(cycle))
            .collect();
        let dead: Vec<blockers::DeadLine> =
            self.dead.iter().map(blockers::DeadLine::from).collect();
        let slots = &self.slots;
        serde_json::json!({
            "slots": {
                "total": slots.total,
                "guard": slots.level.to_string(),
                "allowed": slots.allowed,
                "running": slots.running,
                "free": slots.free,
            },
            "launched": launched,
            "lost": lost,
            "skipped": { "epics": epics, "branches": branches },
            "cycles": cycles,
            "dead_blockers": dead,
        })
    }
}

fn epic_reason(skip: EpicSkip) -> &'static str {
    match skip {
        EpicSkip::Unsigned => "not signed (zaf epic sign is the Engineer's)",
        EpicSkip::Questioned => "a question is open on it",
        EpicSkip::PlanningRuns => "a planning instance runs on it",
        EpicSkip::PlannedNothing => "planning ran since the signature and filed no task",
        EpicSkip::ChallengerRuns => "the challenger runs on it",
        EpicSkip::ChallengerEnded => "the challenger ran and left no report",
        EpicSkip::Done => "every task is finished",
        EpicSkip::BuiltByPhases => "challenged: its tasks are built by their phases",
        EpicSkip::NoFreeSlot => "no free slot",
        EpicSkip::GuardRed => "guard red",
    }
}

#[cfg(test)]
mod tests;
