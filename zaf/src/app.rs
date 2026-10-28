//! Use cases, one per verb: a Query or Command in, a flat Result out.
//!
//! Each verb lands here as it is built, one module per verb, and sees only the
//! traits of [`crate::ports`], never an adapter; [`crate::composition`] hands
//! the live ones in and [`crate::fakes`] the test ones.

pub mod board;
pub mod changes;
pub mod slice;
