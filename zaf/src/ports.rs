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
mod runner;
mod vcs;

pub use board::{Board, ClaimOutcome, ClaimRecord, NewQuestion, Ticket};
pub use design::{Design, Page};
pub use errors::PortError;
pub use forge::{
    Author, Check, CheckName, CheckState, Forge, PrMerge, PrNumber, PrRequest, ReviewComment,
    RuleId, ScanningAlert,
};
pub use gate::{Gate, GateLevel, GateRun, GateVerdict};
pub use runner::{RunOutput, RunRequest, Runner};
pub use vcs::{
    BaseMerge, BaseName, CommitId, DiffStat, FileStat, Publication, SliceName, SliceOpening, Vcs,
};
