//! The one way the jj adapter runs `jj`: at the repository root or in a slice's workspace.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ports::{CommitId, PortError};

/// Where a jj command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// The repository root, without snapshotting its working copy: the root's
    /// checkout is someone else's, and a slice verb must not touch it.
    Root,
    /// The repository root, letting jj snapshot its working copy as usual: only
    /// for `workspace add`, which refuses to run without the working copy.
    RootWithWorkingCopy,
    /// The slice's workspace, snapshotting its working copy first as jj does.
    Workspace,
}

/// Runs `jj` for one slice: plain, uncoloured, unpaged, with stderr kept for the error.
#[derive(Debug, Clone)]
pub(super) struct JjCommand {
    repo_root: PathBuf,
    workspace: PathBuf,
}

impl JjCommand {
    /// A runner for the repository at `repo_root` and the slice workspace at `workspace`.
    pub(super) fn new(repo_root: &Path, workspace: &Path) -> Self {
        Self {
            repo_root: repo_root.to_owned(),
            workspace: workspace.to_owned(),
        }
    }

    /// Runs `jj` with `args`; its stdout, or [`PortError::Backend`] carrying its stderr.
    pub(super) fn run(&self, place: Place, args: &[&str]) -> Result<String, PortError> {
        let mut command = Command::new("jj");
        command.args(["--no-pager", "--color=never"]);
        match place {
            Place::Root => {
                command.current_dir(&self.repo_root);
                command.arg("--ignore-working-copy");
            }
            Place::RootWithWorkingCopy => {
                command.current_dir(&self.repo_root);
            }
            Place::Workspace => {
                command.current_dir(&self.workspace);
            }
        }
        command.args(args);
        let output = command
            .output()
            .map_err(|error| PortError::Unavailable(format!("jj: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let failure = format!("jj {}: {}", args.join(" "), stderr.trim());
            return Err(PortError::Backend(failure));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The commits a revset names, newest first; an empty revset is an empty list.
    pub(super) fn commit_ids(
        &self,
        place: Place,
        revset: &str,
    ) -> Result<Vec<CommitId>, PortError> {
        let stdout = self.run(
            place,
            &[
                "log",
                "--no-graph",
                "-r",
                revset,
                "-T",
                r#"commit_id ++ "\n""#,
            ],
        )?;
        let commit_ids = stdout.lines().map(CommitId::from).collect();
        Ok(commit_ids)
    }

    /// The one commit a revset names.
    pub(super) fn commit_id(&self, place: Place, revset: &str) -> Result<CommitId, PortError> {
        let mut commit_ids = self.commit_ids(place, revset)?;
        if commit_ids.len() != 1 {
            let count = commit_ids.len();
            return Err(PortError::Backend(format!(
                "`{revset}` names {count} commits, not one"
            )));
        }
        Ok(commit_ids.remove(0))
    }

    /// Whether a revset names no commit.
    pub(super) fn is_none(&self, place: Place, revset: &str) -> Result<bool, PortError> {
        Ok(self.commit_ids(place, revset)?.is_empty())
    }

    /// A boolean commit keyword (`empty`, `conflict`) of the workspace's working-copy commit.
    pub(super) fn working_copy_is(&self, keyword: &str) -> Result<bool, PortError> {
        let stdout = self.run(
            Place::Workspace,
            &["log", "--no-graph", "-r", "@", "-T", keyword],
        )?;
        Ok(stdout.trim() == "true")
    }

    /// The paths `jj resolve --list` reports conflicted in the working-copy commit.
    pub(super) fn conflicted_paths(&self) -> Result<Vec<String>, PortError> {
        let stdout = self.run(Place::Workspace, &["resolve", "--list"])?;
        let paths = stdout.lines().filter_map(conflicted_path).collect();
        Ok(paths)
    }
}

/// The path of one `jj resolve --list` line: `<path>   <n>-sided conflict…`.
pub(super) fn conflicted_path(line: &str) -> Option<String> {
    let suffix_start = line.rfind("-sided conflict")?;
    let path = line[..suffix_start]
        .trim_end_matches(|character: char| character.is_ascii_digit())
        .trim_end();
    (!path.is_empty()).then(|| path.to_owned())
}

/// A string literal for a revset or fileset, quoted and escaped.
pub(super) fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The revset of exactly the local bookmark `name`, never a change id that happens to match.
pub(super) fn bookmark(name: &str) -> String {
    format!("bookmarks(exact:{})", quoted(name))
}
