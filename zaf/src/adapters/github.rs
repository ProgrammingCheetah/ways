//! GitHub through the `gh` CLI: the board on its issues, the forge on its pull requests.
//!
//! Every call goes through one [`GhCommand`], which runs `gh` against the
//! manifest's repository and turns a failed call into a
//! [`crate::ports::PortError::Backend`] carrying gh's stderr.

mod board;
mod command;
mod forge;
mod notes;
mod rows;

pub use board::GithubBoard;
pub use command::GhCommand;
pub use forge::GithubForge;

#[cfg(test)]
mod tests;
