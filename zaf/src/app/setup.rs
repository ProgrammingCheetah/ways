//! `zaf setup`: prepare `.zaf/`, fetch the design repo, run doctor, hand over what is left.
//!
//! It never installs a tool: every tool doctor reports as missing, too old or
//! unknown becomes a step of a worksheet for the Engineer. Running it again
//! changes nothing that is already in place.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::doctor::{Commands, Report, ToolStatus, design_dir, doctor};
use crate::manifest::{DesignKind, Manifest};

/// The folders under `.zaf/`.
pub const LAYOUT_DIRS: [&str; 3] = ["roles", "profiles", "runs"];
/// The empty log of the keeper's questions.
pub const ASKLOG_FILE: &str = "asklog.jsonl";
/// Keeps everything under `.zaf/` out of version control.
pub const IGNORE_FILE: &str = ".gitignore";

/// What happened to the design repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOutcome {
    /// `gh repo clone` ran and succeeded.
    Cloned,
    /// The folder was already there.
    Present,
    /// The clone failed; the message is what it said.
    Failed(String),
}

/// One thing the Engineer must do that `setup` cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The action, one sentence.
    pub action: String,
    /// How the Engineer knows it is done.
    pub done_when: String,
}

/// What `setup` did and what remains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    /// The design repo, when the manifest declares one to clone.
    pub clone: Option<CloneOutcome>,
    /// Doctor, run after the folders and the clone.
    pub doctor: Report,
    /// The Engineer's steps; empty when nothing is left.
    pub worksheet: Vec<Step>,
}

impl SetupReport {
    /// Whether nothing is left for the Engineer.
    pub fn is_ready(&self) -> bool {
        self.worksheet.is_empty() && self.doctor.passed()
    }

    /// The worksheet as numbered lines, each step with its "Done when", and a closing "Tell me".
    pub fn worksheet_lines(&self) -> Vec<String> {
        if self.worksheet.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for (number, step) in self.worksheet.iter().enumerate() {
            lines.push(format!("{}. {}", number + 1, step.action));
            lines.push(format!("   Done when: {}", step.done_when));
        }
        lines.push("Tell me: the output of `zaf doctor`.".to_owned());
        lines
    }
}

/// Creates the layout under `state_dir`; anything already there is left as it is.
fn ensure_layout(state_dir: &Path) -> io::Result<()> {
    for dir in LAYOUT_DIRS {
        fs::create_dir_all(state_dir.join(dir))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_dir.join(ASKLOG_FILE))?;
    let ignore = state_dir.join(IGNORE_FILE);
    if !ignore.exists() {
        fs::write(ignore, "*\n")?;
    }
    Ok(())
}

fn clone_design(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Option<CloneOutcome> {
    if manifest.design.kind != DesignKind::GitSibling {
        return None;
    }
    let repo = manifest.design.repo.as_ref()?;
    let dir = design_dir(root, &manifest.design)?;
    if dir.is_dir() {
        return Some(CloneOutcome::Present);
    }
    let target = dir.to_string_lossy();
    let outcome = match commands.run("gh", &["repo", "clone", repo.as_ref(), &target]) {
        Ok(output) if output.success => CloneOutcome::Cloned,
        Ok(output) => CloneOutcome::Failed(output.text.trim().to_owned()),
        Err(error) => CloneOutcome::Failed(error.to_string()),
    };
    Some(outcome)
}

fn worksheet(
    manifest: &Manifest,
    root: &Path,
    clone: Option<&CloneOutcome>,
    report: &Report,
) -> Vec<Step> {
    let mut steps = Vec::new();
    for check in report.failing_tools() {
        let tool = &check.tool;
        let wanted = &check.wanted;
        let action = match &check.status {
            ToolStatus::Missing => format!("Install `{tool}` (the manifest requires `{wanted}`)."),
            ToolStatus::TooOld { found, .. } => {
                format!("Upgrade `{tool}` from {found} to a version satisfying `{wanted}`.")
            }
            ToolStatus::Unknown | ToolStatus::Ok { .. } => {
                format!("Make `{tool} --version` print a semantic version satisfying `{wanted}`.")
            }
        };
        steps.push(Step {
            action,
            done_when: format!("`{tool} --version` prints a version satisfying `{wanted}`."),
        });
    }
    if let (Some(CloneOutcome::Failed(why)), Some(repo), Some(dir)) = (
        clone,
        manifest.design.repo.as_ref(),
        design_dir(root, &manifest.design),
    ) {
        steps.push(Step {
            action: format!(
                "Clone the design repo: `gh repo clone {repo} {}` (it said: {why}).",
                dir.display()
            ),
            done_when: format!("`{}` exists.", dir.display()),
        });
    }
    if !report.workspaces.ok {
        steps.push(Step {
            action: format!(
                "Make `{}` creatable, or point `[workspaces] path` at a folder that is.",
                report.workspaces.path.display()
            ),
            done_when: "`zaf doctor` prints `workspaces dir: ok`.".to_owned(),
        });
    }
    steps
}

/// Prepares `state_dir`, clones a declared design repo that is missing, runs doctor and
/// builds the worksheet. Idempotent.
pub fn setup(
    root: &Path,
    state_dir: &Path,
    manifest: &Manifest,
    commands: &dyn Commands,
) -> io::Result<SetupReport> {
    ensure_layout(state_dir)?;
    let clone = clone_design(root, manifest, commands);
    let report = doctor(root, manifest, commands);
    let worksheet = worksheet(manifest, root, clone.as_ref(), &report);
    Ok(SetupReport {
        clone,
        doctor: report,
        worksheet,
    })
}

/// The files `setup` makes under the state folder, for callers that list them.
pub fn layout_paths(state_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = LAYOUT_DIRS.iter().map(|dir| state_dir.join(dir)).collect();
    paths.push(state_dir.join(ASKLOG_FILE));
    paths.push(state_dir.join(IGNORE_FILE));
    paths
}

#[cfg(test)]
mod tests;
