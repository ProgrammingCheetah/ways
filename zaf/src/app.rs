//! Use cases, one per verb: a Query or Command in, a flat Result out.
//!
//! Each verb lands here as it is built, one module per verb, and sees only the
//! traits of [`crate::ports`], never an adapter; [`crate::composition`] hands
//! the live ones in and [`crate::fakes`] the test ones.

pub mod actions;
pub mod ask;
pub mod asklog;
pub mod begin;
pub mod board;
pub mod brief;
pub mod build;
pub mod changes;
pub mod check;
pub mod doctor;
pub mod epic;
pub mod forge;
pub mod guard;
pub mod hub;
pub mod init;
pub mod profile;
pub mod projects;
pub mod release;
pub mod report;
pub mod schedule;
pub mod setup;
pub mod slice;
pub mod spawn;
pub mod status;
pub mod steps;
pub mod update;
