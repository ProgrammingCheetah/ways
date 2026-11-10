//! Ports: narrow traits written from what the verbs need, never from what a backend offers.
//!
//! Each port has an in-memory fake in [`crate::fakes`], its first consumer;
//! the adapters in [`crate::adapters`] are the next. Every method takes `&self`:
//! a backend is a remote, and a fake keeps its state behind a lock. Verbs are
//! idempotent, so the ports are too: repeating a call on a done state succeeds quietly.

mod board;
mod design;
mod errors;
mod forge;
mod gate;
mod releases;
mod runner;
mod shell;
mod vcs;

pub use board::{
    Board, ClaimOutcome, ClaimRecord, EpicTask, EpicTree, NewQuestion, QUESTION_TITLE_LIMIT,
    Standing,
};
pub use design::{Design, Page};
pub use errors::PortError;
pub use forge::{
    Author, Check, CheckName, CheckState, Forge, PrNumber, ReviewComment, RuleId, ScanningAlert,
};
pub use gate::{Gate, GateLevel, GateRun, GateVerdict};
pub use releases::Releases;
pub use runner::{Holder, RunOutput, RunRequest, Runner};
pub use shell::Shell;
pub use vcs::{BaseName, CommitId, DiffStat, FileStat, SliceName, Vcs};
