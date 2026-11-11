//! Runners: run a role with a profile and capture what it produced.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::PortError;
use crate::domain::{Profile, RoleName, TaskSize};

/// One run of a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The role to run.
    pub role: RoleName,
    /// The profile it runs with.
    pub profile: Profile,
    /// The prompt; the brief travels as a path inside it.
    pub prompt: String,
    /// The directory it runs in.
    pub dir: PathBuf,
    /// Environment for the run, applied after every other layer, so it wins.
    pub env: BTreeMap<String, String>,
    /// Where the run writes its process as a [`Holder`] once the process starts, replacing
    /// what the file held, so a reader can tell whether the run still goes. None writes nothing.
    pub holder_file: Option<PathBuf>,
}

/// A process as zaf records it: its pid and the time the OS says it started.
///
/// The start time tells a pid the OS has since handed to another process from the one
/// recorded, so a recorded holder is alive only while both still match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The process id.
    pub pid: u32,
    /// When the process started, as the OS reports it; compared, never parsed.
    pub start: String,
}

impl Holder {
    /// Reads a holder back from its [`Display`](fmt::Display) form: the pid, a space, the start.
    pub fn parse(text: &str) -> Option<Self> {
        let (pid, start) = text.trim().split_once(' ')?;
        let start = start.trim();
        if start.is_empty() {
            return None;
        }
        Some(Self {
            pid: pid.parse().ok()?,
            start: start.to_owned(),
        })
    }
}

impl fmt::Display for Holder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.pid, self.start)
    }
}

/// The variable naming a launch's run folder, as an absolute path; zaf groups the live
/// processes of the machine by it.
pub const RUN_VAR: &str = "ZAF_RUN";

/// The variable naming a launch's size, a [`TaskSize`] name; a team launch carries none.
pub const SIZE_VAR: &str = "ZAF_SIZE";

/// The variable naming who a launch is, as `<role> instance <n> on #<epic>`.
pub const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// One live zaf instance on the machine, from any project, read from the process table each
/// time it is asked and never stored.
///
/// It is every live process whose environment carries the same [`RUN_VAR`]: the instance and
/// the processes it starts, which inherit its environment. Of an environment only
/// [`RUN_VAR`], [`SIZE_VAR`] and [`INSTANCE_VAR`] are kept, since it holds tokens.
///
/// Its shape: `MachineInstance { run, root, instance, size: Option<TaskSize>, pids }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineInstance {
    /// Its run folder, `<root>/.zaf/runs/<instance slug>`.
    pub run: PathBuf,
    /// The project that launched it: the folder above the run folder's `.zaf`.
    pub root: PathBuf,
    /// Who it is, when its environment says.
    pub instance: Option<String>,
    /// Its size; none for a launch that carries none, or one that does not parse.
    pub size: Option<TaskSize>,
    /// Its live processes, lowest first.
    pub pids: Vec<u32>,
}

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// The process's exit code.
    pub exit_code: i32,
    /// Everything it wrote, as captured.
    pub transcript: String,
}

/// Something that runs a role: a model CLI, a local model, a recorded replay.
pub trait Runner {
    /// Runs the role and waits for it; an absent runner is [`PortError::Unavailable`]
    /// and a run past its time bound is [`PortError::TimedOut`].
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError>;
}

#[cfg(test)]
mod tests;
