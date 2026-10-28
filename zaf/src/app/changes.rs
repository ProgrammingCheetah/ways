//! `zaf changes`: a task's change against its base, stat first, hunks on request.

use super::slice::{SliceError, SliceTarget, Step};
use crate::ports::{DiffStat, Vcs};

/// What `zaf changes` is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesQuery {
    /// The resolved slice of the task.
    pub target: SliceTarget,
    /// One changed file whose hunks to show; `None` for the stat.
    pub path: Option<String>,
}

/// A task's change, as asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changes {
    /// File by file, lines added and removed.
    Stat(DiffStat),
    /// One file's hunks.
    Hunks {
        /// The file.
        path: String,
        /// Its hunks, as the VCS prints them.
        hunks: String,
    },
}

/// `zaf changes`: the stat, or one file's hunks; a file the slice did not change is not found.
pub fn changes(vcs: &dyn Vcs, query: &ChangesQuery) -> Result<Changes, SliceError> {
    let slice = &query.target.slice;
    let at_diff = |error| SliceError::Port {
        step: Step::Diff,
        error,
    };
    match &query.path {
        None => vcs.diff_stat(slice).map(Changes::Stat).map_err(at_diff),
        Some(path) => {
            let hunks = vcs.diff_hunks(slice, path).map_err(at_diff)?;
            let changes = Changes::Hunks {
                path: path.clone(),
                hunks,
            };
            Ok(changes)
        }
    }
}

#[cfg(test)]
mod tests;
