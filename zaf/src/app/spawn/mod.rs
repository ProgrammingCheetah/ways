//! `zaf spawn --once`: one tick of the spawner, the team-instance branch.
//!
//! [`prepare`] drops the launches whose process is gone, reads the board, the
//! state and the guard, plans the tick with [`tick::plan`] and reserves the
//! planned launches in `.zaf/spawn.state`, so each holds its slot from the
//! plan on. [`launch`] then runs one through the Runner port: it records the
//! launch on the epic, runs the `team` role in `.zaf/runs/<instance slug>/`,
//! keeps the instance's output as `report.md` there, records the exit on the
//! epic and frees the slot. Launch and exit are HTML comments on the epic:
//! `<!-- zaf:launch <instance> -->` and `<!-- zaf:exit <instance> <status> -->`,
//! the status being the exit code, `error` when the runner could not run, or
//! `lost` for a launch whose process vanished without one.
//!
//! A dry run ([`plan_only`] and [`dry_run`]) plans the same tick and describes
//! each launch it would make, writing nothing: no state, no run folder, no comment.

mod state;
pub mod tick;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

pub use state::{SpawnState, load, save};
pub use tick::{
    EpicSkip, NOT_BUILT, Plan, PlannedLaunch, Settings, SlotCount, Snapshot, TEAM_ROLE, TaskSkip,
};

use super::guard::{GUARD_FILE, Level};
use crate::domain::{Instance, IssueNumber, Profile, RoleName, TaskLabel};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError, RunRequest, Runner};

/// The one-line launch prompt of a team instance, with `{instance}`, `{epic}`,
/// `{project}`, `{tasks}` and `{brief}` placeholders.
pub const TEAM_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/spawn/team-prompt.txt"
));

/// The team instance's standing brief when the project keeps none of its own.
pub const TEAM_BRIEF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/spawn/team-brief.md"
));

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

/// Reads what the tick needs from the board: the ready tasks, the live claims,
/// and which of the ready tasks' blockers are closed (labelled `done`).
pub fn snapshot(board: &dyn Board) -> Result<Snapshot, PortError> {
    let ready = board.ready()?;
    let claimed = board
        .claims()?
        .into_iter()
        .map(|claim| claim.task)
        .collect();
    let named: BTreeSet<IssueNumber> = ready
        .iter()
        .flat_map(|ticket| tick::blockers(&ticket.body))
        .collect();
    let mut closed = BTreeSet::new();
    for blocker in named {
        match board.task(blocker) {
            Ok(ticket) if ticket.label == Some(TaskLabel::Done) => {
                closed.insert(blocker);
            }
            Ok(_) | Err(PortError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(Snapshot {
        ready,
        claimed,
        closed,
    })
}

/// What [`prepare`] did before any launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The tick's plan; its launches are reserved in the state.
    pub plan: Plan,
    /// Instances dropped because their process is gone, each recorded as `lost`.
    pub lost: Vec<String>,
}

/// Plans one tick and reserves its launches under `pid`, the process that will run them.
///
/// `alive` says whether a recorded pid still runs; the launches of a dead one
/// are dropped and their exit recorded on the epic as `lost`.
pub fn prepare(
    board: &dyn Board,
    state_dir: &Path,
    manifest: &Manifest,
    alive: &dyn Fn(u32) -> bool,
    pid: u32,
) -> Result<Prepared, SpawnError> {
    let mut state = load(state_dir)?;
    let lost = state.prune(alive);
    for instance in &lost {
        if let Ok(parsed) = instance.parse::<Instance>() {
            board.comment(parsed.epic, &exit_marker(instance, "lost"))?;
        }
    }
    let snapshot = snapshot(board)?;
    let level = read_guard(state_dir)?;
    let plan = tick::plan(&snapshot, &state, level, Settings::from(manifest));
    state.reserve(&plan.launches, pid);
    save(state_dir, &state)?;
    Ok(Prepared { plan, lost })
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

/// The launch prompt: one line naming the instance, the epic, the project and the tasks,
/// pointing at the standing brief.
pub fn prompt(launch: &PlannedLaunch, project: &str, brief: &Path) -> String {
    let tasks: Vec<String> = launch.tasks.iter().map(ToString::to_string).collect();
    TEAM_PROMPT
        .trim_end()
        .replace("{instance}", &launch.instance.to_string())
        .replace("{epic}", &launch.instance.epic.to_string())
        .replace("{project}", project)
        .replace("{tasks}", &tasks.join(" "))
        .replace("{brief}", &brief.display().to_string())
}

/// Where a launch runs and what it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The state folder, `<root>/.zaf`.
    pub state_dir: PathBuf,
    /// The project's name, for the prompt.
    pub project: String,
    /// The team role's profile.
    pub profile: Profile,
    /// The environment the launch's process runs with, on its [`RunRequest`].
    pub env: BTreeMap<String, String>,
}

/// How a launched instance ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchExit {
    /// It ran and exited with this code.
    Code(i32),
    /// The runner could not run it.
    Error(String),
}

impl LaunchExit {
    /// The status word in the exit comment.
    pub fn status(&self) -> String {
        match self {
            Self::Code(code) => code.to_string(),
            Self::Error(_) => "error".to_owned(),
        }
    }
}

/// One finished launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    /// The instance.
    pub instance: Instance,
    /// Its tasks.
    pub tasks: Vec<IssueNumber>,
    /// How it ended.
    pub exit: LaunchExit,
    /// Its run folder, holding `report.md`.
    pub run_dir: PathBuf,
}

/// Held while a launch reads, changes and writes `.zaf/spawn.state`, so launches
/// running in parallel threads do not overwrite one another's entries.
static STATE_LOCK: Mutex<()> = Mutex::new(());

/// Loads the state, applies `change` and saves it, all under [`STATE_LOCK`].
fn update(state_dir: &Path, change: impl FnOnce(&mut SpawnState)) -> Result<(), SpawnError> {
    let _held = STATE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut state = load(state_dir)?;
    change(&mut state);
    save(state_dir, &state)
}

/// Runs one reserved launch to its exit and frees its slot.
///
/// The standing brief is `<state dir>/roles/team.md` when present, else the
/// pack's, written into the run folder. A runner that cannot run is recorded
/// as an `error` exit, not returned as an error; a board or file failure is.
pub fn launch(
    board: &dyn Board,
    runner: &dyn Runner,
    context: &LaunchContext,
    planned: &PlannedLaunch,
) -> Result<Launched, SpawnError> {
    let instance = planned.instance.to_string();
    let started = start(board, context, planned);
    let (run_dir, request) = match started {
        Ok(started) => started,
        Err(error) => {
            update(&context.state_dir, |state| state.finish(&instance))?;
            return Err(error);
        }
    };
    let exit = match runner.run(&request) {
        Ok(output) => {
            let report = run_dir.join(REPORT_FILE);
            std::fs::write(&report, &output.transcript).map_err(|source| SpawnError::Write {
                path: report,
                source,
            })?;
            LaunchExit::Code(output.exit_code)
        }
        Err(error) => LaunchExit::Error(error.to_string()),
    };
    board.comment(
        planned.instance.epic,
        &exit_marker(&instance, &exit.status()),
    )?;
    update(&context.state_dir, |state| state.finish(&instance))?;
    Ok(Launched {
        instance: planned.instance.clone(),
        tasks: planned.tasks.clone(),
        exit,
        run_dir,
    })
}

/// A way to get a board and a runner of one's own, for a launch thread.
pub type PortsFn<'a> = dyn Fn() -> Result<LaunchPorts, PortError> + Sync + 'a;

/// The board and the runner one launch thread works with.
pub type LaunchPorts = (Box<dyn Board>, Box<dyn Runner>);

/// Runs the planned launches in parallel, one thread each, and joins them all.
///
/// Each job is a launch with the context it runs under (its own environment).
/// A thread builds its own board and runner with `ports`, so the ports need not
/// be shared; the state file is shared and updated under a lock. The results
/// come back in the jobs' order. A thread that cannot get its ports, or that
/// panics, frees the launch's slot and reports the failure.
pub fn launch_all(
    jobs: &[(&PlannedLaunch, LaunchContext)],
    ports: &PortsFn<'_>,
) -> Vec<Result<Launched, SpawnError>> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(planned, context)| {
                scope.spawn(move || match ports() {
                    Ok((board, runner)) => {
                        launch(board.as_ref(), runner.as_ref(), context, planned)
                    }
                    Err(error) => {
                        let instance = planned.instance.to_string();
                        update(&context.state_dir, |state| state.finish(&instance))?;
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
                    // A panicked launch still frees its slot, so the next tick can use it.
                    let instance = planned.instance.to_string();
                    update(&context.state_dir, |state| state.finish(&instance))?;
                    Err(SpawnError::Port(PortError::Backend(
                        "a launch thread panicked".to_owned(),
                    )))
                })
            })
            .collect()
    })
}

/// Writes the run folder and the brief, marks the launch started and records it on the epic.
fn start(
    board: &dyn Board,
    context: &LaunchContext,
    planned: &PlannedLaunch,
) -> Result<(PathBuf, RunRequest), SpawnError> {
    let instance = planned.instance.to_string();
    let (run_dir, brief, request) = request_for(context, planned);
    std::fs::create_dir_all(&run_dir).map_err(|source| SpawnError::Write {
        path: run_dir.clone(),
        source,
    })?;
    if brief.starts_with(&run_dir) {
        std::fs::write(&brief, TEAM_BRIEF).map_err(|source| SpawnError::Write {
            path: brief.clone(),
            source,
        })?;
    }
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    update(&context.state_dir, |state| state.start(&instance, &now))?;
    board.comment(planned.instance.epic, &launch_marker(&instance))?;
    Ok((run_dir, request))
}

/// A launch's run folder, its standing brief and its request, touching nothing.
///
/// The brief is `<state dir>/roles/team.md` when present, else the pack's, which
/// [`start`] writes into the run folder.
fn request_for(context: &LaunchContext, planned: &PlannedLaunch) -> (PathBuf, PathBuf, RunRequest) {
    let run_dir = context
        .state_dir
        .join(RUNS_DIR)
        .join(slug(&planned.instance));
    let own = context
        .state_dir
        .join(ROLES_DIR)
        .join(format!("{TEAM_ROLE}.md"));
    let brief = if own.is_file() {
        own
    } else {
        run_dir.join(BRIEF_FILE)
    };
    let request = RunRequest {
        role: RoleName::from(TEAM_ROLE),
        profile: context.profile,
        prompt: prompt(planned, &context.project, &brief),
        dir: run_dir.clone(),
        env: context.env.clone(),
    };
    (run_dir, brief, request)
}

/// Plans one tick the way [`prepare`] does, changing nothing: launches whose process is
/// gone are dropped in memory only (and returned as lost), and nothing is reserved.
pub fn plan_only(
    board: &dyn Board,
    state_dir: &Path,
    manifest: &Manifest,
    alive: &dyn Fn(u32) -> bool,
) -> Result<Prepared, SpawnError> {
    let mut state = load(state_dir)?;
    let lost = state.prune(alive);
    let snapshot = snapshot(board)?;
    let level = read_guard(state_dir)?;
    let plan = tick::plan(&snapshot, &state, level, Settings::from(manifest));
    Ok(Prepared { plan, lost })
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
    /// Its tasks.
    pub tasks: Vec<u64>,
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
        let tasks: Vec<String> = self.tasks.iter().map(|task| format!("#{task}")).collect();
        let mut lines = vec![
            format!("would launch {}", self.instance),
            format!("  epic #{}", self.epic),
            format!("  tasks {}", tasks.join(" ")),
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
    profile_dir: &Path,
    describe: &DescribeFn<'_>,
) -> Result<Vec<DryLaunch>, PortError> {
    jobs.iter()
        .map(|(planned, context)| {
            let (_, _, request) = request_for(context, planned);
            let (command, env) = describe(&request)?;
            Ok(DryLaunch {
                instance: planned.instance.to_string(),
                epic: planned.instance.epic.get(),
                tasks: planned.tasks.iter().map(|task| task.get()).collect(),
                profile_dir: profile_dir.to_owned(),
                env,
                command,
            })
        })
        .collect()
}

/// The team role's profile from the manifest; the pack's `build` when the role is absent.
pub fn team_profile(manifest: &Manifest) -> Profile {
    manifest
        .roles
        .get(&RoleName::from(TEAM_ROLE))
        .map_or(Profile::Build, |role| role.profile)
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
        for (task, skip) in &self.tasks {
            lines.push(format!("{task} skipped: {}", task_reason(skip)));
        }
        for launch in &self.launches {
            let tasks: Vec<String> = launch.tasks.iter().map(ToString::to_string).collect();
            lines.push(format!(
                "launching {}: {}",
                launch.instance,
                tasks.join(" ")
            ));
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    /// The plan as JSON, with the finished launches and the lost instances.
    pub fn to_json(&self, launched: &[Launched], lost: &[String]) -> serde_json::Value {
        let numbers =
            |tasks: &[IssueNumber]| -> Vec<u64> { tasks.iter().map(|task| task.get()).collect() };
        let launched: Vec<serde_json::Value> = launched
            .iter()
            .map(|launch| {
                let error = match &launch.exit {
                    LaunchExit::Error(error) => Some(error.clone()),
                    LaunchExit::Code(_) => None,
                };
                serde_json::json!({
                    "instance": launch.instance.to_string(),
                    "epic": launch.instance.epic.get(),
                    "tasks": numbers(&launch.tasks),
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
        let tasks: Vec<serde_json::Value> = self
            .tasks
            .iter()
            .map(|(task, skip)| {
                let (reason, detail) = match skip {
                    TaskSkip::Blocked(by) => ("blocked", serde_json::json!(numbers(by))),
                    TaskSkip::Claimed => ("claimed", serde_json::Value::Null),
                    TaskSkip::Running(instance) => ("running", serde_json::json!(instance)),
                    TaskSkip::NoEpic => ("no_epic", serde_json::Value::Null),
                };
                serde_json::json!({ "task": task.get(), "reason": reason, "detail": detail })
            })
            .collect();
        let branches: Vec<serde_json::Value> = NOT_BUILT
            .iter()
            .map(|branch| serde_json::json!({ "branch": branch, "reason": "not built" }))
            .collect();
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
            "skipped": { "epics": epics, "tasks": tasks, "branches": branches },
        })
    }
}

fn epic_reason(skip: EpicSkip) -> &'static str {
    match skip {
        EpicSkip::NothingLaunchable => "no ready, unblocked, unclaimed task",
        EpicSkip::NoFreeSlot => "no free slot",
        EpicSkip::GuardRed => "guard red",
    }
}

fn task_reason(skip: &TaskSkip) -> String {
    match skip {
        TaskSkip::Blocked(by) => {
            let by: Vec<String> = by.iter().map(ToString::to_string).collect();
            format!("blocked by {}", by.join(" "))
        }
        TaskSkip::Claimed => "claimed".to_owned(),
        TaskSkip::Running(instance) => format!("carried by {instance}"),
        TaskSkip::NoEpic => "no epic".to_owned(),
    }
}

#[cfg(test)]
mod tests;
