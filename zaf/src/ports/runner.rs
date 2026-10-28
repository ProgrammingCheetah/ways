//! Runners: run a role with a profile and capture what it produced.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::PortError;
use crate::domain::{Profile, RoleName};

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
    /// Runs the role and waits for it; an absent runner is [`PortError::Unavailable`].
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError>;
}

#[cfg(test)]
mod tests;
