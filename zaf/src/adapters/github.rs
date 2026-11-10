//! GitHub through the `gh` CLI: the board on its issues, the forge on its pull requests, and
//! zaf's own releases for `zaf update`.
//!
//! Every call goes through one [`GhCommand`], which runs `gh` against the
//! manifest's repository and turns a failed call into a
//! [`crate::ports::PortError::Backend`] carrying gh's stderr.

mod board;
mod command;
mod forge;
mod notes;
mod releases;
mod rows;
mod state;

pub use board::{GithubBoard, GithubProjectBoard};
pub use command::GhCommand;
pub use forge::GithubForge;
pub use releases::GithubReleases;
pub use state::ProjectRef;

#[cfg(test)]
mod tests;
