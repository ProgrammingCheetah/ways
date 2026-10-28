//! The composition root: the one place that reads the manifest and names the adapters.
//!
//! Nothing else knows which backend is live. Every port comes out as a boxed
//! trait object picked by the manifest's kind; a port the manifest leaves out
//! is [`CompositionError::Absent`], which a verb turns into a skipped step or a
//! refusal, never a crash.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::design::DirDesign;
use crate::adapters::gate::ShellGate;
use crate::adapters::github::{GhCommand, GithubBoard, GithubForge};
use crate::adapters::jj::JjVcs;
use crate::adapters::runner::ProcessRunner;
use crate::manifest::{
    BoardKind, DesignKind, ForgeKind, Manifest, ManifestError, RunnerName, VcsKind,
};
use crate::ports::{Board, Design, Forge, Gate, Runner, Vcs};

/// The manifest's file name at the project root.
pub const MANIFEST_FILE: &str = "ZAF.toml";

/// The local, git-ignored state folder under the project root.
pub const STATE_DIR: &str = ".zaf";

/// How long a runner may take before it is killed; the ladder's rungs are short, a
/// review brief is long, and neither should hang the spawner forever.
pub const RUNNER_TIMEOUT: Duration = Duration::from_secs(45 * 60);

/// Why an adapter could not be built from the manifest.
#[derive(Debug, thiserror::Error)]
pub enum CompositionError {
    /// The manifest could not be loaded.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// No `ZAF.toml` in this directory or any parent.
    #[error("no {MANIFEST_FILE} found in {start} or any directory above it")]
    NoProject {
        /// Where the search started.
        start: PathBuf,
    },
    /// The manifest declares no backend for this port.
    #[error("the manifest declares no [{section}] section; the {port} port is absent")]
    Absent {
        /// The port that has no backend.
        port: &'static str,
        /// The manifest section that would declare one.
        section: &'static str,
    },
    /// The section lacks a key its kind needs. The manifest checks this when it
    /// loads, so this is the defensive form of an unreachable unwrap.
    #[error("[{section}] needs `{key}` for kind `{kind}`")]
    MissingKey {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// The kind that needs it.
        kind: String,
    },
    /// The manifest names no runner by that name.
    #[error("no [runners.{0}] in the manifest")]
    NoRunner(RunnerName),
}

/// A project as zaf sees it: its root, its manifest, and the adapters it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    root: PathBuf,
    manifest: Manifest,
}

impl Composition {
    /// Loads `ZAF.toml` from the project root.
    pub fn load(project_root: &Path) -> Result<Self, ManifestError> {
        let manifest = Manifest::load(&project_root.join(MANIFEST_FILE))?;
        Ok(Self {
            root: project_root.to_owned(),
            manifest,
        })
    }

    /// Finds the project root by walking up from `start` to the nearest `ZAF.toml`.
    ///
    /// A slice workspace under `[workspaces]` finds its project this way too,
    /// because the workspace folder lives inside the project root.
    pub fn discover(start: &Path) -> Result<Self, CompositionError> {
        let mut dir = Some(start);
        while let Some(candidate) = dir {
            if candidate.join(MANIFEST_FILE).is_file() {
                return Ok(Self::load(candidate)?);
            }
            dir = candidate.parent();
        }
        Err(CompositionError::NoProject {
            start: start.to_owned(),
        })
    }

    /// Finds the project root from the current directory.
    pub fn discover_here() -> Result<Self, CompositionError> {
        let here = std::env::current_dir().map_err(|source| {
            CompositionError::Manifest(ManifestError::Read {
                path: PathBuf::from("."),
                source,
            })
        })?;
        Self::discover(&here)
    }

    /// The project root: the directory holding `ZAF.toml`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The local state folder, `<root>/.zaf`.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    /// The project's manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The board the manifest names.
    pub fn board(&self) -> Result<Box<dyn Board>, CompositionError> {
        let section = &self.manifest.board;
        match section.kind {
            BoardKind::GithubIssues => {
                let repo = section.repo.clone().ok_or(CompositionError::MissingKey {
                    section: "board",
                    key: "repo",
                    kind: section.kind.to_string(),
                })?;
                Ok(Box::new(GithubBoard::from(GhCommand::from(repo))))
            }
        }
    }

    /// The version control the manifest names.
    pub fn vcs(&self) -> Result<Box<dyn Vcs>, CompositionError> {
        let section = self.manifest.vcs.as_ref().ok_or(CompositionError::Absent {
            port: "Vcs",
            section: "vcs",
        })?;
        match section.kind {
            VcsKind::Jj => Ok(Box::new(JjVcs::new(&self.root, &self.manifest.workspaces))),
        }
    }

    /// The forge the manifest names.
    pub fn forge(&self) -> Result<Box<dyn Forge>, CompositionError> {
        let section = self
            .manifest
            .forge
            .as_ref()
            .ok_or(CompositionError::Absent {
                port: "Forge",
                section: "forge",
            })?;
        match section.kind {
            ForgeKind::Github => {
                let repo = section.repo.clone().ok_or(CompositionError::MissingKey {
                    section: "forge",
                    key: "repo",
                    kind: section.kind.to_string(),
                })?;
                Ok(Box::new(GithubForge::from(GhCommand::from(repo))))
            }
        }
    }

    /// The design source the manifest names, or `None` for a project without one.
    pub fn design(&self) -> Result<Option<Box<dyn Design>>, CompositionError> {
        let section = &self.manifest.design;
        let dir = match section.kind {
            DesignKind::None => return Ok(None),
            DesignKind::RepoDir => {
                let path = section.path.clone().ok_or(CompositionError::MissingKey {
                    section: "design",
                    key: "path",
                    kind: section.kind.to_string(),
                })?;
                self.root.join(path)
            }
            DesignKind::GitSibling => {
                let repo = section.repo.clone().ok_or(CompositionError::MissingKey {
                    section: "design",
                    key: "repo",
                    kind: section.kind.to_string(),
                })?;
                let name = repo
                    .as_ref()
                    .rsplit('/')
                    .next()
                    .unwrap_or_else(|| repo.as_ref())
                    .trim_end_matches(".git")
                    .to_owned();
                let parent = self.root.parent().unwrap_or(&self.root);
                let sibling = section.path.clone().unwrap_or_else(|| PathBuf::from(name));
                parent.join(sibling)
            }
        };
        let design = DirDesign::new(dir);
        Ok(Some(Box::new(match &section.index {
            Some(index) => design.with_index(index.clone()),
            None => design,
        })))
    }

    /// The gate the manifest names, run at the project root.
    pub fn gate(&self) -> Result<Box<dyn Gate>, CompositionError> {
        let section = self
            .manifest
            .gate
            .as_ref()
            .ok_or(CompositionError::Absent {
                port: "Gate",
                section: "gate",
            })?;
        Ok(Box::new(ShellGate::new(&self.root, section)))
    }

    /// The gate run inside `dir` instead of the project root (a slice workspace).
    pub fn gate_in(&self, dir: &Path) -> Result<Box<dyn Gate>, CompositionError> {
        let section = self
            .manifest
            .gate
            .as_ref()
            .ok_or(CompositionError::Absent {
                port: "Gate",
                section: "gate",
            })?;
        Ok(Box::new(ShellGate::new(dir, section)))
    }

    /// The runner `[runners.<name>]` declares.
    pub fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, CompositionError> {
        let section = self
            .manifest
            .runners
            .get(name)
            .ok_or_else(|| CompositionError::NoRunner(name.clone()))?;
        Ok(Box::new(ProcessRunner::new(
            section.command.clone(),
            RUNNER_TIMEOUT,
        )))
    }
}

#[cfg(test)]
mod tests;
