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
    /// A run passed its time bound and was stopped: the bound and the command line.
    #[error("run timed out after {}s: {command}", .bound.as_secs_f64())]
    TimedOut {
        /// The time bound.
        bound: std::time::Duration,
        /// The command line that was run.
        command: String,
    },
    /// The backend failed in a way the caller cannot act on.
    #[error("backend failed: {0}")]
    Backend(String),
}

impl PortError {
    /// The error for a call whose every failure leaves the caller nothing to change but the
    /// backend: a refusal or a missing thing becomes [`PortError::Backend`] with its words kept;
    /// an unavailable backend and a timed-out run stay as they are.
    pub fn into_backend(self) -> Self {
        match self {
            Self::NotFound(message)
            | Self::Conflict(message)
            | Self::Rejected(message)
            | Self::Backend(message) => Self::Backend(message),
            other @ (Self::Unavailable(_) | Self::TimedOut { .. }) => other,
        }
    }
}
