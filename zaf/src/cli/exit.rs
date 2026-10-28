//! Exit codes, the same for every verb, and the mapping from port and composition errors.
//!
//! `0` success, `1` a domain refusal (not ready, not claimed by you, conflict),
//! `2` a backend failure, `64` listed but not built. Clap keeps its own `2` for
//! usage errors.

use crate::composition::CompositionError;
use crate::ports::PortError;

/// The verb did what it said.
pub const OK: u8 = 0;
/// The domain refused: the state did not allow it.
pub const REFUSED: u8 = 1;
/// A backend failed or is missing.
pub const BACKEND: u8 = 2;
/// Listed but not built yet.
pub const NOT_IMPLEMENTED: u8 = 64;

/// The exit code a port error maps to.
pub fn code_for(error: &PortError) -> u8 {
    match error {
        PortError::NotFound(_) | PortError::Conflict(_) | PortError::Rejected(_) => REFUSED,
        PortError::Unavailable(_) | PortError::Backend(_) => BACKEND,
    }
}

/// The exit code a composition error maps to: every one is the environment, not the domain.
pub fn code_for_composition(_error: &CompositionError) -> u8 {
    BACKEND
}

/// Says the verb is not built and returns its code.
pub fn not_built(path: &str) -> u8 {
    eprintln!("zaf {path}: not implemented");
    NOT_IMPLEMENTED
}

/// Prints an error the way every verb does and returns its code.
pub fn fail(path: &str, error: &PortError) -> u8 {
    eprintln!("zaf {path}: {error}");
    code_for(error)
}

/// Prints a composition error the way every verb does and returns its code.
pub fn fail_composition(path: &str, error: &CompositionError) -> u8 {
    eprintln!("zaf {path}: {error}");
    code_for_composition(error)
}
