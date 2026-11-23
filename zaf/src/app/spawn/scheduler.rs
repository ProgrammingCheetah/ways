//! Starting the project's scheduler detached (M13; D19): `zaf spawn --watch --version <v>`, run
//! as the binary a phase's launcher would start with the project's root as its folder.
//!
//! One function, [`start`], does it for the chair's `b` and for a verb alike: it hands back the
//! scheduler whose lock already holds (nothing is started), or starts one and says its pid, or
//! says why it could not. What touches the machine is behind [`Processes`]: the binary's path,
//! the lock read, the signal, the detach and the tick request. The chair's adapter
//! (`cli::chair::build::machine`) is this machine's.
//!
//! The scheduler leads a session of its own, reads nothing (stdin is `/dev/null`) and writes
//! into its own log under `.zaf/logs/` ([`log_name`]). It is no phase's and no instance's, and no
//! Claude session's: every `ZAF_` variable but `ZAF_HOME`, and every variable whose name starts
//! with `CLAUDE`, is dropped from what it inherits ([`dropped`]).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::app::chair::ReleaseVersion;
use crate::app::hub::controls::Failure;
use crate::app::schedule::lock::Holding;
use crate::composition::STATE_DIR;
use crate::ports::Holder;

/// The folder of the logs inside the state folder.
const LOGS_DIR: &str = "logs";

/// The one `ZAF_` variable a scheduler started here keeps: the zaf home it was given.
const KEPT: &str = "ZAF_HOME";

/// The prefix of every Claude variable (`CLAUDECODE`, `CLAUDE_CONFIG_DIR`, ...), none of which a
/// scheduler inherits.
const CLAUDE_PREFIX: &str = "CLAUDE";

/// A process started detached: what runs, where, and the log it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The binary.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub folder: PathBuf,
    /// The log its stdout and stderr go to, made new.
    pub log: PathBuf,
    /// The variables it does not inherit.
    pub dropped: Vec<OsString>,
}

/// What the scheduler's controls need of the machine.
pub trait Processes: Send + Sync {
    /// The binary to start `zaf` as.
    fn program(&self) -> Result<PathBuf, String>;
    /// The scheduler the lock in `state_dir` names, while one runs.
    fn scheduler(&self, state_dir: &Path) -> Option<Holding>;
    /// Sends SIGTERM to `holder`, when it is still the process recorded.
    fn terminate(&self, holder: &Holder);
    /// Starts `launch` detached: its pid, or why it did not start or ended at once.
    fn detach(&self, launch: &Launch) -> Result<u32, String>;
    /// Asks the scheduler of `state_dir` for a tick: the scheduler asked, none when none runs.
    fn ask_tick(&self, state_dir: &Path) -> Result<Option<Holding>, String>;
}

/// What [`start`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// A scheduler was started: its pid and the log it writes.
    Started {
        /// The new scheduler's pid.
        pid: u32,
        /// Its log, under `.zaf/logs/` of the project.
        log: PathBuf,
    },
    /// A scheduler already holds the lock: nothing was started.
    Running(Holding),
}

impl Start {
    /// The pid of the scheduler that runs, new or handed back.
    pub fn pid(&self) -> u32 {
        match self {
            Self::Started { pid, .. } => *pid,
            Self::Running(holding) => holding.pid,
        }
    }
}

/// The variables of `names` a scheduler started here does not inherit: every `ZAF_` one but
/// [`KEPT`], and every one whose name starts with `CLAUDE`.
pub fn dropped<'a>(names: impl IntoIterator<Item = &'a OsStr>) -> Vec<OsString> {
    names
        .into_iter()
        .filter(|name| {
            name.to_str().is_some_and(|name| {
                (name.starts_with("ZAF_") && name != KEPT) || name.starts_with(CLAUDE_PREFIX)
            })
        })
        .map(OsStr::to_owned)
        .collect()
}

/// The scheduler's arguments: `spawn --watch --version <version>`.
pub fn watch_args(version: &ReleaseVersion) -> Vec<String> {
    vec![
        "spawn".to_owned(),
        "--watch".to_owned(),
        "--version".to_owned(),
        version.to_string(),
    ]
}

/// The scheduler's log under `.zaf/logs/`, stamped as a step's log is: the time in UTC to the
/// second, then `spawn-watch-<version>`.
pub fn log_name(at: SystemTime, version: &ReleaseVersion) -> String {
    let stamp: String = humantime::format_rfc3339_seconds(at)
        .to_string()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    format!("{stamp}_spawn-watch-{version}.log")
}

/// Starts the scheduler of the project at `root` for `version`, detached, through `processes`:
/// the scheduler whose lock already holds (nothing started), or the one started, or why none
/// was. A scheduler the lock holds is no refusal here; a missing binary or a start that failed
/// is [`Failure::Failed`].
pub fn start(
    root: &Path,
    version: &ReleaseVersion,
    processes: &dyn Processes,
) -> Result<Start, Failure> {
    let names: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
    start_with(root, version, processes, &names, SystemTime::now())
}

/// [`start`] with the environment's variable `names` and the time `now` given.
fn start_with(
    root: &Path,
    version: &ReleaseVersion,
    processes: &dyn Processes,
    names: &[OsString],
    now: SystemTime,
) -> Result<Start, Failure> {
    let state_dir = root.join(STATE_DIR);
    if let Some(holding) = processes.scheduler(&state_dir) {
        return Ok(Start::Running(holding));
    }
    let program = processes.program().map_err(Failure::Failed)?;
    let launch = Launch {
        program,
        args: watch_args(version),
        folder: root.to_owned(),
        log: state_dir.join(LOGS_DIR).join(log_name(now, version)),
        dropped: dropped(names.iter().map(OsString::as_os_str)),
    };
    let pid = processes.detach(&launch).map_err(Failure::Failed)?;
    Ok(Start::Started {
        pid,
        log: launch.log,
    })
}

#[cfg(test)]
mod tests;
