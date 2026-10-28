//! The composition root: the one place that reads the manifest and names the adapters.
//!
//! Nothing else knows which backend is live. Every port comes out as a boxed
//! trait object picked by the manifest's kind; a port the manifest leaves out
//! is [`CompositionError::Absent`], which a verb turns into a skipped step or a
//! refusal, never a crash.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::design::DirDesign;
use crate::adapters::gate::ShellGate;
use crate::adapters::github::{GhCommand, GithubBoard, GithubForge};
use crate::adapters::jj::JjVcs;
use crate::adapters::runner::ProcessRunner;
use crate::adapters::shell::SystemShell;
use crate::app::profile::{self, CONFIG_DIR_VAR};
use crate::domain::{Profile, RoleName};
use crate::manifest::{
    BoardKind, CommandLine, DesignKind, Effort, ForgeKind, Manifest, ManifestError, ModelName,
    RunnerName, VcsKind,
};
use crate::ports::{Board, Design, Forge, Gate, RunRequest, Runner, Shell, Vcs};

/// The manifest's file name at the project root.
pub const MANIFEST_FILE: &str = "ZAF.toml";

/// The local, git-ignored state folder under the project root.
pub const STATE_DIR: &str = ".zaf";

/// The runner a role's launch goes through: `[runners.claude]`.
pub const ROLE_RUNNER: &str = "claude";

/// How long one shell line may take before it is killed: a command step, a named test,
/// a tool's `--version`. A gate is longer than any of them and has its own, unbounded run.
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

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
    /// The manifest names no role by that name.
    #[error("no [roles.{0}] in the manifest")]
    NoRole(RoleName),
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

    /// Finds the project root by walking up from `start`.
    ///
    /// Every directory on the way up with a `ZAF.toml` is a candidate. The root is the
    /// topmost candidate whose `[workspaces]` path holds every candidate below it, so a
    /// manifest copied into a slice workspace still resolves to its project; failing
    /// that, the nearest candidate is the root.
    pub fn discover(start: &Path) -> Result<Self, CompositionError> {
        let candidates: Vec<&Path> = start
            .ancestors()
            .filter(|dir| dir.join(MANIFEST_FILE).is_file())
            .collect();
        // Nearest first; the loop below tries the topmost first.
        for (index, candidate) in candidates.iter().enumerate().rev() {
            let Ok(composition) = Self::load(candidate) else {
                continue;
            };
            let workspaces = normalized(&candidate.join(&composition.manifest.workspaces.path));
            let below = &candidates[..index];
            if below
                .iter()
                .all(|lower| normalized(lower).starts_with(&workspaces))
            {
                return Ok(composition);
            }
        }
        match candidates.first() {
            Some(nearest) => Ok(Self::load(nearest)?),
            None => Err(CompositionError::NoProject {
                start: start.to_owned(),
            }),
        }
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

    /// A profile's home, `<root>/.zaf/profiles/<profile>`: what `CLAUDE_CONFIG_DIR` points at.
    pub fn profile_dir(&self, profile: Profile) -> PathBuf {
        profile::home(&self.state_dir(), profile)
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
        let (key, kind) = match section.kind {
            DesignKind::None => return Ok(None),
            DesignKind::RepoDir => ("path", section.kind),
            DesignKind::GitSibling => ("repo", section.kind),
        };
        // The one rule for where the folder is, shared with `doctor` and `setup`.
        let dir = section
            .dir(&self.root)
            .ok_or(CompositionError::MissingKey {
                section: "design",
                key,
                kind: kind.to_string(),
            })?;
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
        self.build_runner(name, None)
    }

    /// The runner `[runners.<name>]` declares, with `{model}` in its command set to `model`.
    pub fn runner_with_model(
        &self,
        name: &RunnerName,
        model: &ModelName,
    ) -> Result<Box<dyn Runner>, CompositionError> {
        self.build_runner(name, Some(model))
    }

    /// The runner a role launches through: `[runners.claude]` with `{model}` and `{effort}`
    /// from `[roles.<role>]`, run inside the role's profile (`CLAUDE_CONFIG_DIR` set to
    /// [`profile_dir`](Self::profile_dir) for requests carrying that profile).
    pub fn runner_for_role(&self, role: &RoleName) -> Result<Box<dyn Runner>, CompositionError> {
        Ok(Box::new(self.role_runner(role)?))
    }

    /// What [`runner_for_role`](Self::runner_for_role) would run for `request`, without
    /// running it: the command line and the environment it sets.
    pub fn role_launch(
        &self,
        request: &RunRequest,
    ) -> Result<(String, BTreeMap<String, String>), CompositionError> {
        let runner = self.role_runner(&request.role)?;
        Ok((runner.command_line(request), runner.env_for(request)))
    }

    fn role_runner(&self, role: &RoleName) -> Result<ProcessRunner, CompositionError> {
        let section = self
            .manifest
            .roles
            .get(role)
            .ok_or_else(|| CompositionError::NoRole(role.clone()))?;
        let runner = self.process_runner(
            &RunnerName::from(ROLE_RUNNER),
            Some(&section.model),
            Some(section.effort),
        )?;
        let home = self.profile_dir(section.profile);
        Ok(runner.with_profile_env(
            section.profile,
            CONFIG_DIR_VAR,
            home.to_string_lossy().into_owned(),
        ))
    }

    fn build_runner(
        &self,
        name: &RunnerName,
        model: Option<&ModelName>,
    ) -> Result<Box<dyn Runner>, CompositionError> {
        Ok(Box::new(self.process_runner(name, model, None)?))
    }

    /// The runner's command with `{model}` and `{effort}` filled where given; a
    /// placeholder left unfilled stays as written.
    fn process_runner(
        &self,
        name: &RunnerName,
        model: Option<&ModelName>,
        effort: Option<Effort>,
    ) -> Result<ProcessRunner, CompositionError> {
        let section = self
            .manifest
            .runners
            .get(name)
            .ok_or_else(|| CompositionError::NoRunner(name.clone()))?;
        let mut command = section.command.to_string();
        if let Some(model) = model {
            command = command.replace("{model}", model.as_ref());
        }
        if let Some(effort) = effort {
            command = command.replace("{effort}", &effort.to_string());
        }
        let timeout = Duration::from_secs(section.timeout_minutes * 60);
        Ok(ProcessRunner::new(CommandLine::from(command), timeout))
    }

    /// A shell that runs command lines at the project root.
    pub fn shell(&self) -> Box<dyn Shell> {
        Box::new(SystemShell::new(&self.root, SHELL_TIMEOUT))
    }
}

/// The path with `.` and `..` folded away lexically, so `a/../b` compares as `b`.
fn normalized(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !folded.pop() {
                    folded.push(component);
                }
            }
            other => folded.push(other),
        }
    }
    folded
}

#[cfg(test)]
mod tests;
