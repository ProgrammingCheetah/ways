//! The jj adapter for [`Vcs`]: a slice is a jj workspace plus a bookmark of the same name.
//!
//! A slice `feature/v1-export-2` on base `feature/v1` is a workspace named
//! `feature/v1-export-2` at `<workspaces>/feature/v1-export-2`, whose working-copy
//! change sits on a bookmark named `feature/v1-export-2`. The bookmark is the
//! slice's committed line; the working copy is its uncommitted work. The base is
//! recorded in the workspace's own jj config, so it leaves with the workspace.
//! Works on a plain or a colocated jj repository.

mod command;

use std::path::{Component, Path, PathBuf};

use command::{JjCommand, Place, bookmark, quoted};

use crate::manifest::WorkspacesSection;
use crate::ports::{
    BaseMerge, BaseName, CommitId, DiffStat, FileStat, PortError, Publication, SliceName,
    SliceOpening, Vcs,
};

/// The workspace config key holding a slice's base.
const BASE_KEY: &str = "zaf.slice-base";

/// What jj prints when a push finds the remote moved since the last fetch.
const REMOTE_MOVED: &str = "unexpectedly moved on the remote";

/// Slices as jj workspaces and bookmarks in one repository.
#[derive(Debug, Clone)]
pub struct JjVcs {
    repo_root: PathBuf,
    workspaces: PathBuf,
}

impl JjVcs {
    /// The repository at `repo_root`, with slice workspaces where `[workspaces]` puts them.
    ///
    /// An inherent constructor: it joins two facts from two places, the project
    /// root and a manifest section, which no std conversion takes.
    pub fn new(repo_root: &Path, workspaces: &WorkspacesSection) -> Self {
        Self {
            repo_root: repo_root.to_owned(),
            workspaces: repo_root.join(&workspaces.path),
        }
    }

    /// The directory of a slice's workspace, open or not. A slice name that
    /// would leave the workspaces directory (absolute, or with `..`) is [`PortError::Rejected`].
    pub fn slice_dir(&self, slice: &SliceName) -> Result<PathBuf, PortError> {
        let relative = Path::new(slice.as_ref());
        let stays_inside = relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
        if !stays_inside || slice.as_ref().is_empty() {
            return Err(PortError::Rejected(format!(
                "slice name {slice} is not a relative path"
            )));
        }
        Ok(self.workspaces.join(relative))
    }

    fn command(&self, dir: &Path) -> JjCommand {
        JjCommand::new(&self.repo_root, dir)
    }

    /// The base recorded in a workspace, or `None` when no slice workspace is there.
    fn recorded_base(&self, dir: &Path) -> Option<BaseName> {
        if !dir.join(".jj").is_dir() {
            return None;
        }
        // `config get` fails on a missing key; either way there is no slice here.
        let stdout = self
            .command(dir)
            .run(Place::Workspace, &["config", "get", BASE_KEY])
            .ok()?;
        Some(BaseName::from(stdout.trim()))
    }

    /// An open slice's runner and base, or [`PortError::NotFound`].
    fn open(&self, slice: &SliceName) -> Result<(JjCommand, BaseName), PortError> {
        let dir = self.slice_dir(slice)?;
        let base = self
            .recorded_base(&dir)
            .ok_or_else(|| PortError::NotFound(format!("slice {slice}")))?;
        Ok((self.command(&dir), base))
    }

    /// Opens a clean slice for a call that moves its working copy.
    fn open_clean(&self, slice: &SliceName) -> Result<(JjCommand, BaseName), PortError> {
        let (jj, base) = self.open(slice)?;
        if !jj.working_copy_is("empty")? {
            return Err(PortError::Rejected(format!(
                "slice {slice} has uncommitted work; commit it first"
            )));
        }
        Ok((jj, base))
    }

    /// The revset of where the slice forked from its base.
    fn fork_point(base: &BaseName) -> String {
        format!("fork_point({} | @)", bookmark(base.as_ref()))
    }
}

impl Vcs for JjVcs {
    fn open_slice(&self, slice: &SliceName, base: &BaseName) -> Result<SliceOpening, PortError> {
        let dir = self.slice_dir(slice)?;
        if dir.exists() {
            return match self.recorded_base(&dir) {
                Some(recorded) if recorded == *base => Ok(SliceOpening::AlreadyOpen),
                Some(recorded) => Err(PortError::Conflict(format!(
                    "slice {slice} is open on {recorded}"
                ))),
                None => Err(PortError::Conflict(format!(
                    "{} exists and is not a slice workspace",
                    dir.display()
                ))),
            };
        }
        let jj = self.command(&dir);
        let base_revset = bookmark(base.as_ref());
        if jj.is_none(Place::Root, &base_revset)? {
            return Err(PortError::NotFound(format!("base bookmark {base}")));
        }
        let slice_revset = bookmark(slice.as_ref());
        if jj.is_none(Place::Root, &slice_revset)? {
            jj.run(
                Place::Root,
                &["bookmark", "create", slice.as_ref(), "-r", &base_revset],
            )?;
        }
        // A workspace whose directory was removed by hand still has a name to free.
        jj.run(Place::Root, &["workspace", "forget", slice.as_ref()])?;
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                PortError::Backend(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        let dir_text = dir.to_string_lossy();
        jj.run(
            Place::RootWithWorkingCopy,
            &[
                "workspace",
                "add",
                "--name",
                slice.as_ref(),
                "-r",
                &slice_revset,
                &dir_text,
            ],
        )?;
        jj.run(
            Place::Workspace,
            &["config", "set", "--workspace", BASE_KEY, base.as_ref()],
        )?;
        Ok(SliceOpening::Opened)
    }

    fn merge_base(&self, slice: &SliceName) -> Result<BaseMerge, PortError> {
        let (jj, base) = self.open_clean(slice)?;
        let base_revset = bookmark(base.as_ref());
        let slice_revset = bookmark(slice.as_ref());
        let base_missing = format!("{base_revset} ~ ::{slice_revset}");
        if jj.is_none(Place::Workspace, &base_missing)? {
            return Ok(BaseMerge::UpToDate);
        }
        let slice_own = format!("{slice_revset} ~ ::{base_revset}");
        if jj.is_none(Place::Workspace, &slice_own)? {
            // Nothing of the slice's own: the base's tip becomes the slice's.
            jj.run(
                Place::Workspace,
                &["bookmark", "set", slice.as_ref(), "-r", &base_revset],
            )?;
            jj.run(Place::Workspace, &["new", &slice_revset])?;
            return Ok(BaseMerge::Merged);
        }
        let message = format!("Merge {base} into {slice}");
        jj.run(
            Place::Workspace,
            &["new", &slice_revset, &base_revset, "-m", &message],
        )?;
        if jj.working_copy_is("conflict")? {
            let paths = jj.conflicted_paths()?;
            let merge_commit = jj.commit_id(Place::Workspace, "@")?;
            jj.run(Place::Workspace, &["new", &slice_revset])?;
            jj.run(Place::Workspace, &["abandon", merge_commit.as_ref()])?;
            return Ok(BaseMerge::Conflicted { paths });
        }
        jj.run(Place::Workspace, &["new"])?;
        jj.run(
            Place::Workspace,
            &["bookmark", "set", slice.as_ref(), "-r", "@-"],
        )?;
        Ok(BaseMerge::Merged)
    }

    fn commit(&self, slice: &SliceName, message: &str) -> Result<CommitId, PortError> {
        let (jj, _base) = self.open(slice)?;
        if jj.working_copy_is("empty")? {
            return Err(PortError::Rejected(format!(
                "nothing to commit for `{message}`"
            )));
        }
        jj.run(Place::Workspace, &["commit", "-m", message])?;
        jj.run(
            Place::Workspace,
            &["bookmark", "set", slice.as_ref(), "-r", "@-"],
        )?;
        jj.commit_id(Place::Workspace, "@-")
    }

    fn publish(&self, slice: &SliceName) -> Result<Publication, PortError> {
        let (jj, _base) = self.open(slice)?;
        let published = format!(
            "{} & remote_bookmarks(exact:{})",
            bookmark(slice.as_ref()),
            quoted(slice.as_ref())
        );
        if !jj.is_none(Place::Workspace, &published)? {
            return Ok(Publication::AlreadyPublished);
        }
        let pattern = format!("exact:{slice}");
        jj.run(Place::Workspace, &["git", "push", "--bookmark", &pattern])
            .map_err(|error| match error {
                PortError::Backend(failure) if failure.contains(REMOTE_MOVED) => {
                    PortError::Conflict(format!("the remote has work {slice} lacks: {failure}"))
                }
                other => other,
            })?;
        Ok(Publication::Published)
    }

    fn is_clean(&self, slice: &SliceName) -> Result<bool, PortError> {
        let (jj, _base) = self.open(slice)?;
        jj.working_copy_is("empty")
    }

    fn diff_stat(&self, slice: &SliceName) -> Result<DiffStat, PortError> {
        let (jj, base) = self.open(slice)?;
        let fork_point = Self::fork_point(&base);
        let diff = jj.run(
            Place::Workspace,
            &["diff", "--git", "--from", &fork_point, "--to", "@"],
        )?;
        let files = parse_git_stat(&diff);
        Ok(DiffStat { files })
    }

    fn diff_hunks(&self, slice: &SliceName, path: &str) -> Result<String, PortError> {
        let (jj, base) = self.open(slice)?;
        let fork_point = Self::fork_point(&base);
        let fileset = format!("root-file:{}", quoted(path));
        let hunks = jj.run(
            Place::Workspace,
            &[
                "diff",
                "--git",
                "--from",
                &fork_point,
                "--to",
                "@",
                &fileset,
            ],
        )?;
        if hunks.is_empty() {
            return Err(PortError::NotFound(format!("no change to {path}")));
        }
        Ok(hunks)
    }

    fn drop_slice(&self, slice: &SliceName) -> Result<(), PortError> {
        let dir = self.slice_dir(slice)?;
        let jj = self.command(&dir);
        jj.run(Place::Root, &["workspace", "forget", slice.as_ref()])?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|error| {
                PortError::Backend(format!("cannot remove {}: {error}", dir.display()))
            })?;
        }
        remove_empty_parents(&dir, &self.workspaces);
        Ok(())
    }
}

/// Removes the directories a nested slice name left between `dir` and `root`, while empty.
fn remove_empty_parents(dir: &Path, root: &Path) {
    let mut current = dir.parent();
    while let Some(parent) = current.filter(|parent| parent.starts_with(root) && *parent != root) {
        // `remove_dir` refuses a non-empty directory: another slice still lives there.
        if std::fs::remove_dir(parent).is_err() {
            return;
        }
        current = parent.parent();
    }
}

/// Lines added and removed per file in a `--git` diff, in path order.
fn parse_git_stat(diff: &str) -> Vec<FileStat> {
    let mut files: Vec<FileStat> = Vec::new();
    let mut in_hunk = false;
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            let file = FileStat {
                path: header_path(header),
                added: 0,
                removed: 0,
            };
            files.push(file);
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        let Some(file) = files.last_mut().filter(|_| in_hunk) else {
            continue;
        };
        if line.starts_with('+') {
            file.added += 1;
        } else if line.starts_with('-') {
            file.removed += 1;
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

/// The path of a `diff --git a/<old> b/<new>` header: the new path.
fn header_path(header: &str) -> String {
    // Same old and new path (the common case) splits exactly in half, spaces and all.
    let half = header.len().saturating_sub(5) / 2;
    if let Some(path) = header.get(2..2 + half)
        && header == format!("a/{path} b/{path}")
    {
        return path.to_owned();
    }
    header
        .rsplit_once(" b/")
        .map_or(header, |(_, new_path)| new_path)
        .to_owned()
}

#[cfg(test)]
mod tests;
