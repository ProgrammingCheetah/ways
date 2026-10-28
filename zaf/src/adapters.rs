//! Adapters, one per port and backend kind, picked by [`crate::composition`].
//!
//! Every external thing is declared in the manifest with a kind or left absent;
//! an absent one removes its check steps instead of failing them.

pub mod design;
pub mod gate;
pub mod github;
pub mod jj;
pub mod runner;
