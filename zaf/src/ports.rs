//! Ports: narrow traits written from what the verbs need, never from what a backend offers.
//!
//! Each port has an in-memory fake in [`crate::fakes`], its first consumer;
//! the adapters in [`crate::adapters`] are the next. Every method takes `&self`:
//! a backend is a remote, and a fake keeps its state behind a lock. Verbs are
//! idempotent, so the ports are too: repeating a call on a done state succeeds quietly.

mod board;
mod ci_filter;
mod design;
mod errors;
mod forge;
mod gate;
mod releases;
mod runner;
mod shell;
mod tool_host;
mod vcs;
mod verb_runner;

pub use crate::domain::ChangedFiles;
pub use board::{
    AccessCheck, Board, ClaimOutcome, ClaimRecord, IssueTypes, Login, NewQuestion,
    QUESTION_TITLE_LIMIT,
};
pub use ci_filter::CiFilter;
pub use design::{Design, Page};
pub use errors::PortError;
pub use forge::{
    Author, BranchChecks, BranchName, Check, CheckBucket, CheckLink, CheckLog, CheckName, CheckRun,
    CheckState, Forge, JobId, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PrBody, PrLink,
    PrNumber, PrRef, PrTitle, PullRequest, ReviewComment, RuleId, ScanningAlert,
};
pub use gate::{Gate, GateLevel, GateRun, GateVerdict};
pub use releases::Releases;
pub use runner::{
    Holder, INSTANCE_VAR, MachineInstance, RUN_VAR, RunOutput, RunRequest, Runner, SIZE_VAR,
    e_is_live, live_use,
};
pub use shell::Shell;
pub use tool_host::ToolHost;
pub use vcs::{BaseName, CommitId, DiffStat, FileStat, SliceName, TreeId, Vcs};
pub use verb_runner::{VerbOutcome, VerbRun, VerbRunner};
