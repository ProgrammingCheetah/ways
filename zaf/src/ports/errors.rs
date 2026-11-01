//! The one error every port returns.

/// A port call that did not succeed, by what the caller can do about it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    /// The thing addressed does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Someone else holds it, or the state disagrees with the call (a claim by another instance).
    #[error("conflict: {0}")]
    Conflict(String),
    /// The backend refused the call as made (nothing to commit, a task not ready).
    #[error("rejected: {0}")]
    Rejected(String),
    /// The backend or a declared piece of it is not there (a runner offline, a gate not declared).
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// The backend failed in a way the caller cannot act on.
    #[error("backend failed: {0}")]
    Backend(String),
}
