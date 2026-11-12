//! Exit codes, the same for every verb, and the mapping from port and composition errors.
//!
//! `0` success, `1` a domain refusal (not ready, not claimed by you, conflict),
//! `2` a backend failure, `64` listed but not built. Clap keeps its own `2` for
//! usage errors.

use crate::composition::{Composition, CompositionError};
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
        PortError::Unavailable(_) | PortError::TimedOut { .. } | PortError::Backend(_) => BACKEND,
    }
}

/// The exit code a composition error maps to: a kind with no adapter yet is listed but not
/// built; every other one is the environment, not the domain.
pub fn code_for_composition(error: &CompositionError) -> u8 {
    match error {
        CompositionError::NotBuilt { .. } => NOT_IMPLEMENTED,
        _ => BACKEND,
    }
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

/// Says which absolute design folder and index the project's design source was read from, after
/// it could not be read; says nothing for a project without a design source.
pub fn note_design_paths(path: &str, composition: &Composition) {
    if let Some(tried) = composition.design_paths_tried() {
        eprintln!("zaf {path}: the design source could not be read; tried {tried}");
    }
}

/// Prints a composition error the way every verb does and returns its code.
pub fn fail_composition(path: &str, error: &CompositionError) -> u8 {
    eprintln!("zaf {path}: {error}");
    code_for_composition(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_not_built_is_listed_but_not_built_and_the_rest_a_backend_failure() {
        let not_built = CompositionError::NotBuilt {
            port: "Forge",
            section: "forge",
            kind: "forgejo".to_owned(),
        };
        assert_eq!(code_for_composition(&not_built), NOT_IMPLEMENTED);
        let absent = CompositionError::Absent {
            port: "Forge",
            section: "forge",
        };
        assert_eq!(code_for_composition(&absent), BACKEND);
    }
}
