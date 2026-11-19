//! GitHub: the board on its issues and the forge on its pull requests, through the `gh` CLI,
//! and zaf's own releases for `zaf update`, read over `curl` from GitHub's public API.
//!
//! Every call goes through one [`GhCommand`], which runs `gh` against the
//! manifest's repository and turns a failed call into a
//! [`crate::ports::PortError::Backend`] carrying gh's stderr.

mod board;
mod command;
mod forge;
mod public_releases;
mod rows;
mod state;

pub use board::{GithubBoard, GithubProjectBoard};
pub use command::{GhCommand, GhRun};
pub use forge::GithubForge;
pub use public_releases::PublicReleases;
pub use state::ProjectRef;

#[cfg(test)]
mod tests;
