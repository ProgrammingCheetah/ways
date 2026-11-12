//! Adapters, one per port and backend kind, picked by [`crate::composition`].
//!
//! Every external thing is declared in the manifest with a kind or left absent;
//! an absent one removes its check steps instead of failing them. What both issue boards
//! (GitHub's and Forgejo's) keep the same way lives here once: the kind labels and the rules
//! over states (`issues`), and the comments they write and read back (`notes`).

pub mod design;
pub mod forgejo;
pub mod gate;
pub mod github;
mod issues;
pub mod jj;
mod notes;
pub mod process;
pub mod runner;
pub mod shell;
pub mod slot;
