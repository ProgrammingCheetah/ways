//! `zaf plan`: what a plan is worth before it is filed, what it becomes on the board, and what its
//! graph reads as after (M10). `plan check` reads a plan and prints its refusals or its measures;
//! `plan show` reads an epic's graph from its tickets.
//!
//! [`input`] fills what a plan takes from the other epics of its release: the names they provide
//! and the names they need from this epic.
//!
//! [`replan`] fills a re-plan's input: the epic's filed tickets as fixed nodes, the next record
//! number, and the check that a `Drop:` ticket has not started.
//!
//! [`verify`] is `plan verify`: the epic's graph held to O3's structural checks.
//!
//! [`tickets`] is the node-to-ticket step: pure over the optimised plan and the plan as written,
//! it makes one [`TicketToFile`] for each node and posts nothing.
//!
//! [`file`] is `plan file`: it posts the plan's record, files the tickets, closes the dropped ones
//! and posts the filing mark. [`finish`] is `plan file` with empty stdin: it finishes the last
//! record that has no mark, from the record.

pub mod check;
pub mod file;
pub mod finish;
pub mod input;
pub mod replan;
pub mod show;
pub mod tickets;
pub mod verify;

pub use tickets::{TicketError, TicketToFile, tickets};
