//! Forgejo through its REST API: the board on a repository's issues (board kind
//! `forgejo-issues`) and the forge on its pull requests (forge kind `forgejo`). Releases are not
//! built yet.
//!
//! Every call goes through one [`ForgejoApi`], which runs `curl` against the instance's
//! `/api/v1`, hands it the token on stdin, and turns a failed call into a
//! [`crate::ports::PortError`]: a 404 is `NotFound`, an instance that does not answer is
//! `Unavailable`, any other refusal is `Backend` with Forgejo's message.

mod access;
mod api;
mod board;
mod forge;
mod labels;
mod rows;

pub use api::{ForgejoApi, ForgejoToken, Method, Reply, query_value};
pub use board::ForgejoBoard;
pub use forge::ForgejoForge;
pub use labels::{DUPLICATE_LABEL, NOT_PLANNED_LABEL, state_label, type_label};

#[cfg(test)]
mod tests;
