//! `zaf check`: what is checkable about a task, and the step engine for a pull request.
//!
//! A task target runs the light gate, its named tests, a symbol search for its
//! interface ids, and lists its prose acceptance lines as manual ([`task`]).
//! A pull request target runs the manifest's `[check.pr] steps` through the
//! [`engine`]: each step kind is one module under [`step`], every step runs in
//! its own thread under a timeout, and the findings come back deduplicated and
//! tagged by source ([`report`]). Both end as a [`record::CheckRecord`], the
//! shape that is printed and stored for `--status`.
//!
//! The folder is `check/mod.rs` rather than `check.rs` beside it because the
//! task that built it owned only this folder.

pub mod engine;
pub mod record;
pub mod report;
pub mod step;
pub mod target;
pub mod task;

pub use engine::{CheckReport, Engine, EngineSettings, StepPorts, StepResult, StepStatus};
pub use record::{CheckRecord, RecordLine};
pub use report::{CheckedFinding, dedupe, parse_report};
pub use step::{RunnerStep, Step, StepError, StepKind};
pub use target::{Target, TargetError};
pub use task::{LineKind, LineStatus, TaskCheck, TaskInputs, TaskLine, check_task};

pub use crate::ports::Shell;

/// How a check ended, as the exit code says it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize,
)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Nothing failed and no must-fix finding: exit 0.
    Clean,
    /// A must-fix finding or a failed line: exit 1.
    MustFix,
    /// A step failed or timed out, so the result is incomplete: exit 2.
    StepFailed,
}

impl Verdict {
    /// The exit code: 0 clean, 1 must-fix present, 2 a step failed or timed out.
    pub fn code(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::MustFix => 1,
            Self::StepFailed => 2,
        }
    }
}
