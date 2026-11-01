//! The VCS, in slice terms: open, merge the base in, commit, publish, inspect.
//!
//! Slice terms, not VCS terms (branch, worktree, bookmark, stage), because
//! VCSes disagree on the latter and a verb should not care.

use std::fmt;

use super::PortError;
use crate::newtype::text_newtype;

text_newtype! {
    /// The name of a slice: one task's line of work and its workspace.
    SliceName
}

text_newtype! {
    /// The line a slice starts from and merges back into (the integration line).
    BaseName
}

text_newtype! {
    /// The id the VCS gave a commit.
    CommitId
}

/// What opening a slice did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceOpening {
    /// A new slice and workspace exist.
    Opened,
    /// The slice already existed on the same base; nothing changed.
    AlreadyOpen,
}

/// What merging the base into a slice did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseMerge {
    /// The slice already had the base.
    UpToDate,
    /// The base's new work is in the slice.
    Merged,
    /// The merge stopped on conflicts in these paths; the slice is unchanged.
    Conflicted {
        /// The conflicting paths.
        paths: Vec<String>,
    },
}

/// What publishing a slice did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publication {
    /// New commits reached the remote.
    Published,
    /// The remote already had every commit.
    AlreadyPublished,
}

/// Lines added and removed in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    /// The file, relative to the repository root.
    pub path: String,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
}

/// A slice's change against its base, file by file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStat {
    /// The changed files, in path order.
    pub files: Vec<FileStat>,
}

impl fmt::Display for DiffStat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for file in &self.files {
            writeln!(formatter, "{} +{} -{}", file.path, file.added, file.removed)?;
        }
        let added: u32 = self.files.iter().map(|file| file.added).sum();
        let removed: u32 = self.files.iter().map(|file| file.removed).sum();
        writeln!(formatter, "{} files +{added} -{removed}", self.files.len())
    }
}

/// The VCS behind every slice.
pub trait Vcs {
    /// Opens a slice and its workspace at a base. Opening an existing slice on
    /// the same base succeeds with [`SliceOpening::AlreadyOpen`]; on another base it is a conflict.
    fn open_slice(&self, slice: &SliceName, base: &BaseName) -> Result<SliceOpening, PortError>;

    /// Merges the slice's base into it; never rewrites the slice's history.
    fn merge_base(&self, slice: &SliceName) -> Result<BaseMerge, PortError>;

    /// Commits the slice's uncommitted work; nothing to commit is [`PortError::Rejected`].
    fn commit(&self, slice: &SliceName, message: &str) -> Result<CommitId, PortError>;

    /// Publishes the slice plainly: a remote with work the slice lacks is a conflict, never overwritten.
    fn publish(&self, slice: &SliceName) -> Result<Publication, PortError>;

    /// Whether the slice has no uncommitted work.
    fn is_clean(&self, slice: &SliceName) -> Result<bool, PortError>;

    /// The slice's change against its base, stat only.
    fn diff_stat(&self, slice: &SliceName) -> Result<DiffStat, PortError>;

    /// The hunks of one changed file, on request.
    fn diff_hunks(&self, slice: &SliceName, path: &str) -> Result<String, PortError>;

    /// Removes the slice's workspace; a slice already gone succeeds quietly.
    fn drop_slice(&self, slice: &SliceName) -> Result<(), PortError>;
}

#[cfg(test)]
mod tests;
