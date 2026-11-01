//! The gate: the project's own checks, full or light, as the manifest declares them.

use super::PortError;

/// Which gate to run.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum GateLevel {
    /// Everything CI runs.
    Full,
    /// The quick subset for a work-in-progress check.
    Light,
}

/// How a gate run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GateVerdict {
    /// Exit code zero.
    Passed,
    /// Anything else.
    Failed,
}

/// A finished gate run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRun {
    /// How it ended.
    pub verdict: GateVerdict,
    /// What it printed, captured.
    pub output: String,
}

/// The project's gate.
pub trait Gate {
    /// Runs the gate at a level; a level the project did not declare is [`PortError::Unavailable`].
    fn run(&self, level: GateLevel) -> Result<GateRun, PortError>;
}

#[cfg(test)]
mod tests;
