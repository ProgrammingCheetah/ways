//! The composition root: the one place that reads the manifest and names the adapters.
//!
//! Nothing else knows which backend is live. Every port comes out as a boxed
//! trait object picked by the manifest's kind; a port the manifest leaves out
//! is [`CompositionError::Absent`], which a verb turns into a skipped step or a
//! refusal, never a crash.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::design::DirDesign;
use crate::adapters::gate::ShellGate;
use crate::adapters::github::{GhCommand, GithubBoard, GithubForge};
use crate::adapters::jj::JjVcs;
use crate::adapters::runner::{ProcessRunner, single_quote};
use crate::adapters::shell::SystemShell;
use crate::app::profile::{self, CONFIG_DIR_VAR};
use crate::domain::{Profile, RoleName};
use crate::manifest::{
    BoardKind, CommandLine, DesignKind, Effort, FOLDER_FILE, ForgeKind, LEGACY_FILE, Manifest,
    ManifestError, ModelName, ProjectFiles, ROOT_FILE, RunnerName, VcsKind,
};
use crate::ports::{Board, Design, Forge, Gate, RunRequest, Runner, Shell, SliceName, Vcs};

/// The retired manifest file name at the project root; `zaf init` still writes it.
pub const MANIFEST_FILE: &str = LEGACY_FILE;

/// The variable naming the zaf home, where the machine's `config.toml` lives.
pub const ZAF_HOME_VAR: &str = "ZAF_HOME";

/// The variable naming the user's home, under which `.zaf` is the zaf home by default.
pub const HOME_VAR: &str = "HOME";

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
    /// No manifest file in this directory or any parent.
    #[error(
        "no {ROOT_FILE}, {FOLDER_FILE} or {LEGACY_FILE} found in {start} or any directory above it"
    )]
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

/// The zaf home, where the machine's `config.toml` lives: `ZAF_HOME`, else `~/.zaf`, else
/// none (neither `ZAF_HOME` nor `HOME` set), and then the machine layer is absent.
pub fn zaf_home() -> Option<PathBuf> {
    zaf_home_from(std::env::var_os(ZAF_HOME_VAR), std::env::var_os(HOME_VAR))
}

/// [`zaf_home`] from the two variables' values; an empty value counts as unset.
pub fn zaf_home_from(zaf_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    set(zaf_home)
        .map(PathBuf::from)
        .or_else(|| set(home).map(|home| PathBuf::from(home).join(STATE_DIR)))
}

impl Composition {
    /// Loads the manifest of the project at `project_root` from its manifest files and the
    /// machine's `config.toml`.
    pub fn load(project_root: &Path) -> Result<Self, ManifestError> {
        Self::load_with_home(project_root, zaf_home().as_deref())
    }

    /// [`load`](Self::load) with the zaf home given instead of read from the environment.
    pub fn load_with_home(project_root: &Path, home: Option<&Path>) -> Result<Self, ManifestError> {
        let manifest = Manifest::load_layers(project_root, home)?;
        Ok(Self {
            root: project_root.to_owned(),
            manifest,
        })
    }

    /// Finds the project root by walking up from `start`.
    ///
    /// Every directory on the way up with a manifest file (`.zaf.toml`, `.zaf/config.toml`
    /// or the retired `ZAF.toml`) is a candidate; the zaf home's own `config.toml` is not a
    /// project's. The root is the topmost candidate whose `[workspaces]` path holds every
    /// candidate below it, so a manifest copied into a slice workspace still resolves to
    /// its project; failing that, the nearest candidate is the root.
    pub fn discover(start: &Path) -> Result<Self, CompositionError> {
        Self::discover_with_home(start, zaf_home().as_deref())
    }

    /// [`discover`](Self::discover) with the zaf home given instead of read from the environment.
    pub fn discover_with_home(start: &Path, home: Option<&Path>) -> Result<Self, CompositionError> {
        let mut candidates: Vec<&Path> = Vec::new();
        for dir in start.ancestors() {
            let files = ProjectFiles::find(dir, home);
            if files.is_empty() {
                continue;
            }
            // The retired file beside a new one is an error wherever it is found.
            files.refuse_legacy_beside_new()?;
            candidates.push(dir);
        }
        // Nearest first; the loop below tries the topmost first.
        for (index, candidate) in candidates.iter().enumerate().rev() {
            let Ok(composition) = Self::load_with_home(candidate, home) else {
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
            Some(nearest) => Ok(Self::load_with_home(nearest, home)?),
            None => Err(CompositionError::NoProject {
                start: start.to_owned(),
            }),
        }
    }

    /// Finds the project root from the current directory, as every verb but `setup` does.
    /// A project still on the retired `ZAF.toml` loads as it is, with one line on stderr
    /// ([`legacy_notice`](Self::legacy_notice)); nothing is moved and stdout is untouched.
    pub fn discover_here() -> Result<Self, CompositionError> {
        let composition = Self::discover_here_quiet()?;
        if let Some(notice) = composition.legacy_notice() {
            eprintln!("{notice}");
        }
        Ok(composition)
    }

    /// [`discover_here`](Self::discover_here) without the legacy warning: `setup`'s, which
    /// is the verb that moves the file.
    pub fn discover_here_quiet() -> Result<Self, CompositionError> {
        let here = std::env::current_dir().map_err(|source| {
            CompositionError::Manifest(ManifestError::Read {
                path: PathBuf::from("."),
                source,
            })
        })?;
        Self::discover(&here)
    }

    /// The project root: the directory holding the manifest.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The warning for a project that still has the retired root `ZAF.toml`: the line that
    /// says to run `zaf setup`; `None` once it is `.zaf.toml`.
    pub fn legacy_notice(&self) -> Option<String> {
        self.root.join(LEGACY_FILE).is_file().then(|| {
            format!(
                "zaf: {LEGACY_FILE} is the retired name; run `zaf setup` to move it to {ROOT_FILE}"
            )
        })
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

    /// The directory of a slice's workspace, open or not: the project root, the
    /// `[workspaces]` path and the slice name. The manifest alone answers it, so no port is called.
    pub fn slice_dir(&self, slice: &SliceName) -> PathBuf {
        self.root
            .join(&self.manifest.workspaces.path)
            .join(slice.as_ref())
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

    /// [`gate_in`](Self::gate_in) with `env` set on every run, on top of the inherited
    /// environment (see [`project_env`](Self::project_env)).
    pub fn gate_in_with_env(
        &self,
        dir: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Box<dyn Gate>, CompositionError> {
        let section = self
            .manifest
            .gate
            .as_ref()
            .ok_or(CompositionError::Absent {
                port: "Gate",
                section: "gate",
            })?;
        Ok(Box::new(ShellGate::new(dir, section).with_env(env.clone())))
    }

    /// The manifest's `[env]` table with `{root}` the absolute project root and `{version}`
    /// the given version. A value that uses `{version}` with none given is a
    /// [`ManifestError`] naming the variable.
    pub fn project_env(
        &self,
        version: Option<&str>,
    ) -> Result<BTreeMap<String, String>, ManifestError> {
        self.manifest.env.resolve(&self.root, version)
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

    /// The runner's command with `{root}` set to the project root (single-quoted for the
    /// shell) and `{model}` and `{effort}` filled where given; a placeholder left unfilled
    /// stays as written. `{prompt}` and `{dir}` are the runner's own, filled per run.
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
        command = command.replace("{root}", &single_quote(&self.root.to_string_lossy()));
        let timeout = Duration::from_secs(section.timeout_minutes * 60);
        Ok(ProcessRunner::new(CommandLine::from(command), timeout))
    }

    /// A shell that runs command lines at the project root.
    pub fn shell(&self) -> Box<dyn Shell> {
        Box::new(SystemShell::new(&self.root, SHELL_TIMEOUT))
    }

    /// A shell that runs command lines in `dir` with `env` set on every line, on top of the
    /// inherited environment.
    pub fn shell_in_with_env(&self, dir: &Path, env: &BTreeMap<String, String>) -> Box<dyn Shell> {
        Box::new(SystemShell::new(dir, SHELL_TIMEOUT).with_env(env.clone()))
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
