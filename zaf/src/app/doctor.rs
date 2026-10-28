//! `zaf doctor`: verify the tools the manifest requires and the folders it names.
//!
//! The manifest itself is validated when it loads, so a report exists only for a
//! manifest that already parsed. Tools are asked for `<tool> --version` through
//! [`Commands`], which tests fake and the command line backs with real processes.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::{DesignSection, Manifest, Requirement, ToolName};

pub mod fake;

/// What a finished command wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Standard output and standard error, as captured.
    pub text: String,
    /// Whether the command exited with success.
    pub success: bool,
}

/// Runs a program to completion; the tiny seam that keeps the checks testable.
pub trait Commands {
    /// Runs `program` with `args`; an error means it could not be started (not installed).
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput>;
}

/// How one required tool stands.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolStatus {
    /// Installed and satisfying its requirement.
    Ok {
        /// The installed version.
        found: String,
    },
    /// Could not be started.
    Missing,
    /// Installed, but its version does not satisfy the requirement.
    TooOld {
        /// The installed version.
        found: String,
        /// The requirement as written in the manifest.
        wanted: String,
    },
    /// Installed, but its `--version` output holds no semantic version.
    Unknown,
}

impl ToolStatus {
    /// Whether the tool counts as present and right.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }
}

impl fmt::Display for ToolStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok { .. } => write!(formatter, "ok"),
            Self::Missing => write!(formatter, "missing"),
            Self::TooOld { found, wanted } => write!(formatter, "too old {found} < {wanted}"),
            Self::Unknown => write!(formatter, "unknown (no version in the output)"),
        }
    }
}

/// One `[requires]` entry, checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ToolCheck {
    /// The tool's name.
    pub tool: String,
    /// The requirement as written in the manifest.
    pub wanted: String,
    /// How the tool stands.
    #[serde(flatten)]
    pub status: ToolStatus,
}

/// A folder the manifest names, checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PathCheck {
    /// What the folder is for.
    pub what: &'static str,
    /// Where it is, resolved against the project root.
    pub path: PathBuf,
    /// Whether it passes.
    pub ok: bool,
}

/// Everything `doctor` found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    /// One entry per `[requires]` tool, in name order.
    pub tools: Vec<ToolCheck>,
    /// The design folder, when the design kind is not `none`.
    pub design: Option<PathCheck>,
    /// The workspaces folder, which must be creatable.
    pub workspaces: PathCheck,
}

impl Report {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.tools.iter().all(|check| check.status.is_ok())
            && self.design.as_ref().is_none_or(|check| check.ok)
            && self.workspaces.ok
    }

    /// The tools that are missing, too old or unknown.
    pub fn failing_tools(&self) -> impl Iterator<Item = &ToolCheck> {
        self.tools.iter().filter(|check| !check.status.is_ok())
    }

    /// One line per check, the way the command line prints them.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .tools
            .iter()
            .map(|check| format!("{}: {}", check.tool, check.status))
            .collect();
        lines.extend(self.design.iter().map(PathCheck::line));
        lines.push(self.workspaces.line());
        lines
    }
}

impl PathCheck {
    fn line(&self) -> String {
        let state = match (self.what, self.ok) {
            (_, true) => "ok",
            ("design dir", false) => "missing",
            (_, false) => "not creatable",
        };
        format!("{}: {state} ({})", self.what, self.path.display())
    }
}

/// The first `major.minor.patch` in the text, ignoring any pre-release or build suffix.
///
/// A tool's own words around it (`gh version 2.40.1 (2024-01-01)`, `claude 2.1.3 (...)`) are skipped.
pub fn first_semver(text: &str) -> Option<semver::Version> {
    let mut previous: Option<char> = None;
    for (index, ch) in text.char_indices() {
        let at_boundary = previous.is_none_or(|before| {
            matches!(before, 'v' | 'V') || !(before.is_ascii_alphanumeric() || before == '.')
        });
        previous = Some(ch);
        if !ch.is_ascii_digit() || !at_boundary {
            continue;
        }
        let rest = &text[index..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if let Ok(version) = semver::Version::parse(rest[..end].trim_end_matches('.')) {
            return Some(version);
        }
    }
    None
}

fn check_tool(commands: &dyn Commands, tool: &ToolName, requirement: &Requirement) -> ToolCheck {
    let status = match commands.run(tool.as_ref(), &["--version"]) {
        Err(_) => ToolStatus::Missing,
        Ok(output) => match first_semver(&output.text) {
            None => ToolStatus::Unknown,
            Some(found) if requirement.is_met_by(&found) => ToolStatus::Ok {
                found: found.to_string(),
            },
            Some(found) => ToolStatus::TooOld {
                found: found.to_string(),
                wanted: requirement.to_string(),
            },
        },
    };
    ToolCheck {
        tool: tool.to_string(),
        wanted: requirement.to_string(),
        status,
    }
}

/// Where the design folder is, as the composition root resolves it; `None` for kind `none`.
///
/// Both ask [`DesignSection::dir`]: a `path` is relative to the project root, and a
/// `git-sibling` without one is `<parent>/<repo name>`.
pub fn design_dir(root: &Path, design: &DesignSection) -> Option<PathBuf> {
    design.dir(root)
}

/// Whether `path` is, or could be made by creating folders under, a writable folder.
fn creatable(path: &Path) -> bool {
    let mut probe = path;
    loop {
        match std::fs::metadata(probe) {
            Ok(meta) => return meta.is_dir() && !meta.permissions().readonly(),
            Err(_) => match probe.parent() {
                Some(parent) => probe = parent,
                None => return false,
            },
        }
    }
}

/// Checks the required tools, the design folder and the workspaces folder of a loaded manifest.
pub fn doctor(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Report {
    let tools = manifest
        .requires
        .iter()
        .map(|(tool, requirement)| check_tool(commands, tool, requirement))
        .collect();
    let design = design_dir(root, &manifest.design).map(|path| PathCheck {
        what: "design dir",
        ok: path.is_dir(),
        path,
    });
    let workspaces_path = root.join(&manifest.workspaces.path);
    let workspaces = PathCheck {
        what: "workspaces dir",
        ok: creatable(&workspaces_path),
        path: workspaces_path,
    };
    Report {
        tools,
        design,
        workspaces,
    }
}

#[cfg(test)]
mod tests;
