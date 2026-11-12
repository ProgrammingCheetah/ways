//! The jj adapter for [`Vcs`]: what zaf reads of a slice, which is a jj workspace plus a bookmark
//! of the same name.
//!
//! The slice's own work (open, merge the base in, commit, publish, drop) is the project's
//! `slice-*` actions; the adapter reads the workspace the slice record names, against the base
//! the branch pattern names. Works on a plain or a colocated jj repository.

mod command;

use std::path::Path;

use command::{JjCommand, bookmark, quoted};

use crate::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, Vcs};

/// Slice workspaces read through jj.
#[derive(Debug, Clone, Copy, Default)]
pub struct JjVcs;

impl JjVcs {
    /// The runner for a workspace the caller names, or [`PortError::NotFound`] when the
    /// directory holds no jj workspace. No config key is read: the caller knows the base.
    fn workspace_command(workspace: &Path) -> Result<JjCommand, PortError> {
        if !workspace.join(".jj").is_dir() {
            return Err(PortError::NotFound(format!(
                "workspace {}",
                workspace.display()
            )));
        }
        Ok(JjCommand::new(workspace))
    }

    /// The revset of where the slice forked from its base.
    fn fork_point(base: &BaseName) -> String {
        format!("fork_point({} | @)", bookmark(base.as_ref()))
    }
}

impl Vcs for JjVcs {
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError> {
        // The revision the slice's bookmark is settled on when it is published.
        Self::workspace_command(workspace)?.commit_id("@-")
    }

    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError> {
        let jj = Self::workspace_command(workspace)?;
        let fork_point = Self::fork_point(base);
        let diff = jj.run(&["diff", "--git", "--from", &fork_point, "--to", "@"])?;
        let files = parse_git_stat(&diff);
        Ok(DiffStat { files })
    }

    fn diff_hunks(
        &self,
        workspace: &Path,
        base: &BaseName,
        path: &str,
    ) -> Result<String, PortError> {
        let jj = Self::workspace_command(workspace)?;
        let fork_point = Self::fork_point(base);
        let fileset = format!("root-file:{}", quoted(path));
        let hunks = jj.run(&[
            "diff",
            "--git",
            "--from",
            &fork_point,
            "--to",
            "@",
            &fileset,
        ])?;
        if hunks.is_empty() {
            return Err(PortError::NotFound(format!("no change to {path}")));
        }
        Ok(hunks)
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
