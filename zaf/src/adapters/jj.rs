//! The jj adapter for [`Vcs`]: what zaf reads of a slice, which is a jj workspace plus a bookmark
//! of the same name.
//!
//! The slice's own work (open, merge the base in, commit, publish, drop) is the project's
//! `slice-*` actions; the adapter reads the workspace the slice record names, against the base
//! the branch pattern names. Works on a plain or a colocated jj repository.

mod command;

use std::path::Path;

use command::{JjCommand, bookmark, commit_id_revset, is_commit_id, quoted};

use crate::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, TreeId, Vcs};

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

    /// The runner for the project's root, which reads without a snapshot of the Engineer's working
    /// copy; [`PortError::NotFound`] when the directory holds no jj workspace.
    fn root_command(root: &Path) -> Result<JjCommand, PortError> {
        if !root.join(".jj").is_dir() {
            return Err(PortError::NotFound(format!("workspace {}", root.display())));
        }
        Ok(JjCommand::read_only(root))
    }

    /// The git folder jj keeps its commits in, where a commit names its tree.
    fn git_store(jj: &JjCommand) -> Result<String, PortError> {
        Ok(jj.run(&["git", "root"])?.trim().to_owned())
    }

    /// The tree of `id` in the git folder `store`.
    fn tree_in(store: &str, id: &str) -> Result<TreeId, PortError> {
        // The commit comes from the forge: only a full commit id is let in, and anything else is
        // a commit this repository does not hold.
        if !is_commit_id(id) {
            return Err(PortError::NotFound(format!("commit {id}")));
        }
        // jj itself has no template that prints a tree.
        let output = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(store)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{id}^{{tree}}"))
            .output()
            .map_err(|error| PortError::Unavailable(format!("git: {error}")))?;
        let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || tree.is_empty() {
            return Err(PortError::NotFound(format!("commit {id}")));
        }
        Ok(TreeId::from(tree))
    }

    /// The revset of where the slice forked from its base.
    fn fork_point(base: &BaseName) -> String {
        format!("fork_point({} | @)", bookmark(base.as_ref()))
    }
}

impl Vcs for JjVcs {
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError> {
        // The revision the slice's bookmark is settled on when it is published: the working
        // copy's parent. A merge in progress (`sync-base` leaves one, conflicts and all) has two
        // parents and is the work itself, past any published head, so it is the working copy.
        Self::workspace_command(workspace)?.commit_id("(@ & merges()) | (@- ~ (@ & merges())-)")
    }

    fn holds(&self, workspace: &Path, head: &CommitId) -> Result<bool, PortError> {
        let jj = Self::workspace_command(workspace)?;
        let latest = self.latest_commit(workspace)?;
        // The head comes from the forge and goes into a revset: only a full commit id is let in,
        // and anything else is a head this repository does not hold.
        let head = head.as_str();
        if !is_commit_id(head) {
            return Ok(false);
        }
        // Held when the latest commit is the head or one of its ancestors. A head the repository
        // does not have names nothing, so it is not held.
        let from = commit_id_revset(latest.as_str());
        let to = commit_id_revset(head);
        if jj.commit_ids(&format!("{from} & ::{to}"))?.is_empty() {
            return Ok(false);
        }
        // The same tree: nothing differs between the two commits.
        let changes = jj.run(&["diff", "--summary", "--from", &from, "--to", &to])?;
        Ok(changes.trim().is_empty())
    }

    fn tree_of(&self, root: &Path, commit: &CommitId) -> Result<TreeId, PortError> {
        let jj = Self::root_command(root)?;
        let id = commit.as_str();
        if !is_commit_id(id) {
            return Err(PortError::NotFound(format!("commit {id}")));
        }
        Self::tree_in(&Self::git_store(&jj)?, id)
    }

    fn trees_of(&self, root: &Path, commits: &[&CommitId]) -> Vec<(CommitId, TreeId)> {
        // The git folder is the same for every commit: found once.
        let Ok(store) = Self::root_command(root).and_then(|jj| Self::git_store(&jj)) else {
            return Vec::new();
        };
        commits
            .iter()
            .filter_map(|commit| {
                let tree = Self::tree_in(&store, commit.as_str()).ok()?;
                Some(((*commit).clone(), tree))
            })
            .collect()
    }

    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError> {
        let jj = Self::workspace_command(workspace)?;
        let fork_point = Self::fork_point(base);
        let diff = jj.run(&["diff", "--git", "--from", &fork_point, "--to", "@"])?;
        let files = parse_git_stat(&diff);
        Ok(DiffStat { files })
    }

    fn has_tag(&self, root: &Path, tag: &str) -> Result<bool, PortError> {
        let names = format!("exact:{}", quoted(tag));
        let listed = Self::root_command(root)?.run(&["tag", "list", &names])?;
        Ok(!listed.trim().is_empty())
    }

    fn has_remote(&self, root: &Path) -> Result<bool, PortError> {
        let listed = Self::root_command(root)?.run(&["git", "remote", "list"])?;
        Ok(!listed.trim().is_empty())
    }

    fn tags(&self, root: &Path) -> Result<Vec<String>, PortError> {
        let listed = Self::root_command(root)?.run(&["tag", "list", "-T", r#"name ++ "\n""#])?;
        let mut names: Vec<String> = listed.lines().map(str::to_owned).collect();
        names.sort();
        names.dedup();
        Ok(names)
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
