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

/// The id the VCS gave a commit, now the domain's (the scheduling core keys facts on the pull
/// request's head); kept here so `crate::ports::CommitId` still names it.
pub use crate::domain::{CommitId, TreeId};

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

    /// Whether `head` holds the workspace's work: the workspace's latest commit is `head` or an
    /// ancestor of it, and the two commits' trees are equal, so an empty commit on top of the
    /// latest commit holds it and a commit that changes anything does not. A head the repository
    /// does not hold is `false`, never an error: the guard stays short of `land` and never merges
    /// work it has not seen, and a read fetches nothing. A directory that is no workspace is
    /// [`PortError::NotFound`].
    fn holds(&self, workspace: &Path, head: &CommitId) -> Result<bool, PortError>;

    /// The tree of `commit` in the repository at `root`: two commits hold the same files exactly
    /// when their trees are equal, so an empty commit has its parent's tree. A commit the
    /// repository does not hold, and a directory that is no repository, are
    /// [`PortError::NotFound`].
    fn tree_of(&self, root: &Path, commit: &CommitId) -> Result<TreeId, PortError>;

    /// The trees of `commits` in the repository at `root`, in the order given; a commit whose tree
    /// cannot be read is left out. An adapter that must look something up to read a tree does it
    /// once here, not once for each commit. The default asks [`Vcs::tree_of`] for each.
    fn trees_of(&self, root: &Path, commits: &[&CommitId]) -> Vec<(CommitId, TreeId)> {
        commits
            .iter()
            .filter_map(|commit| {
                let tree = self.tree_of(root, commit).ok()?;
                Some(((*commit).clone(), tree))
            })
            .collect()
    }

    /// The change in a workspace against its base, stat only. The workspace is the one the
    /// slice record names and the base comes from the branch pattern: the VCS reads neither
    /// from its own state. A directory that is no workspace is [`PortError::NotFound`].
    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError>;

    /// Whether the repository at `root` holds the tag. A directory that is no repository is
    /// [`PortError::NotFound`].
    fn has_tag(&self, root: &Path, tag: &str) -> Result<bool, PortError>;

    /// Whether the repository at `root` has a remote. A directory that is no repository is
    /// [`PortError::NotFound`], and a VCS that cannot be run is an error, never `false`.
    fn has_remote(&self, root: &Path) -> Result<bool, PortError>;

    /// Every tag of the repository at `root`, by name, in no set order. A directory that is no
    /// repository is [`PortError::NotFound`].
    fn tags(&self, root: &Path) -> Result<Vec<String>, PortError>;

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
