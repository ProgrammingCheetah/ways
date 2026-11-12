//! The VCS, in slice terms, for what zaf reads of a slice: its latest commit and its change.
//!
//! Slice terms, not VCS terms (branch, worktree, bookmark, stage), because
//! VCSes disagree on the latter and a verb should not care.

use std::fmt;
use std::path::Path;

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

/// The VCS behind every slice, read only: the slice's own work (open, merge the base in, commit,
/// publish, drop) is the project's `slice-*` actions.
pub trait Vcs {
    /// The workspace's latest commit by its git id, hand-made or not, with uncommitted work no
    /// part of it. The workspace is the one the slice record names. A directory that is no
    /// workspace is [`PortError::NotFound`].
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError>;

    /// The change in a workspace against its base, stat only. The workspace is the one the
    /// slice record names and the base comes from the branch pattern: the VCS reads neither
    /// from its own state. A directory that is no workspace is [`PortError::NotFound`].
    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError>;

    /// The hunks of one changed file of a workspace against its base, on request; a file
    /// with no change is [`PortError::NotFound`].
    fn diff_hunks(
        &self,
        workspace: &Path,
        base: &BaseName,
        path: &str,
    ) -> Result<String, PortError>;
}

#[cfg(test)]
mod tests;
