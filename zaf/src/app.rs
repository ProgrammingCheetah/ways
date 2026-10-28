//! Use cases, one per verb: a Query or Command in, a flat Result out.
//!
//! Each verb lands here as it is built, one module per verb, and sees only the
//! traits of [`crate::ports`], never an adapter; [`crate::composition`] hands
//! the live ones in and [`crate::fakes`] the test ones.

pub mod ask;
pub mod asklog;
pub mod board;
pub mod brief;
pub mod changes;
pub mod check;
pub mod doctor;
pub mod init;
pub mod setup;
pub mod slice;
