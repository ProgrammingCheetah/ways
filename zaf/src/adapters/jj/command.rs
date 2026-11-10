//! The one way the jj adapter runs `jj`: in a slice's workspace.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ports::{CommitId, PortError};

/// Runs `jj` in one slice's workspace: plain, uncoloured, unpaged, with stderr kept for the error.
#[derive(Debug, Clone)]
pub(super) struct JjCommand {
    workspace: PathBuf,
}

impl JjCommand {
    /// A runner for the slice workspace at `workspace`.
    pub(super) fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_owned(),
        }
    }

    /// Runs `jj` with `args`, snapshotting the working copy first as jj does; its stdout, or
    /// [`PortError::Backend`] carrying its stderr.
    pub(super) fn run(&self, args: &[&str]) -> Result<String, PortError> {
        let mut command = Command::new("jj");
        command.args(["--no-pager", "--color=never"]);
        command.current_dir(&self.workspace);
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
    fn commit_ids(&self, revset: &str) -> Result<Vec<CommitId>, PortError> {
        let stdout = self.run(&[
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            r#"commit_id ++ "\n""#,
        ])?;
        let commit_ids = stdout.lines().map(CommitId::from).collect();
        Ok(commit_ids)
    }

    /// The one commit a revset names.
    pub(super) fn commit_id(&self, revset: &str) -> Result<CommitId, PortError> {
        let mut commit_ids = self.commit_ids(revset)?;
        if commit_ids.len() != 1 {
            let count = commit_ids.len();
            return Err(PortError::Backend(format!(
                "`{revset}` names {count} commits, not one"
            )));
        }
        Ok(commit_ids.remove(0))
    }
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
