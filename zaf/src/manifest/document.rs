//! The whole manifest: read, defaulted and validated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::sections::{
    ActionName, ActionsSection, AskSection, BoardKind, BoardSection, BranchesSection,
    ChecksSection, DesignKind, DesignSection, EnvSection, ForbiddenSection, ForgeKind,
    ForgeSection, GateSection, IsolationKind, IsolationSection, PackSection, ProjectSection,
    RoleSection, RunnerName, RunnerSection, SpawnSection, StyleSection, ToolName, VcsSection,
    WorkspacesSection,
};
use super::{ManifestError, Requirement, layers};
use crate::domain::{MAX_QUESTIONS, RoleName};

/// A project's `ZAF.toml`. Only `[project]` and `[board]` are required.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[project]`.
    pub project: ProjectSection,
    /// `[requires]`: tool name to version requirement or release tag.
    #[serde(default)]
    pub requires: BTreeMap<ToolName, Requirement>,
    /// `[board]`.
    pub board: BoardSection,
    /// `[vcs]`; absent means no VCS port.
    #[serde(default)]
    pub vcs: Option<VcsSection>,
    /// `[forge]`; absent means no forge port.
    #[serde(default)]
    pub forge: Option<ForgeSection>,
    /// `[design]`; absent means kind `none`.
    #[serde(default)]
    pub design: DesignSection,
    /// `[style]`; absent means the project declares no style file.
    #[serde(default)]
    pub style: Option<StyleSection>,
    /// `[workspaces]`.
    #[serde(default)]
    pub workspaces: WorkspacesSection,
    /// `[branches]`.
    #[serde(default)]
    pub branches: BranchesSection,
    /// `[gate]`; absent means no gate port.
    #[serde(default)]
    pub gate: Option<GateSection>,
    /// `[runners.<name>]`.
    #[serde(default)]
    pub runners: BTreeMap<RunnerName, RunnerSection>,
    /// `[roles.<name>]`, the pack's roles filled in where the project leaves them out.
    #[serde(default)]
    pub roles: BTreeMap<RoleName, RoleSection>,
    /// `[check]` and `[check.<target>]`; a target left out runs no steps.
    #[serde(default)]
    pub check: ChecksSection,
    /// `[actions]`.
    #[serde(default)]
    pub actions: ActionsSection,
    /// `[ask]`.
    #[serde(default)]
    pub ask: AskSection,
    /// `[spawn]`.
    #[serde(default)]
    pub spawn: SpawnSection,
    /// `[isolation]`.
    #[serde(default)]
    pub isolation: IsolationSection,
    /// `[forbidden]`.
    #[serde(default)]
    pub forbidden: ForbiddenSection,
    /// `[pack]`; absent means the profiles carry no pack.
    #[serde(default)]
    pub pack: Option<PackSection>,
    /// `[env]`: variables for every child, merged key by key across the layers.
    #[serde(default)]
    pub env: EnvSection,
    /// `[phases]`: a task type's name (the forge's issue type, such as `Task` or `Bug`) to its
    /// ordered list of action names, as in `Task = ["implement", "ci", "merge"]`. A higher layer
    /// replaces a type's whole list. Read as `Manifest.phases: BTreeMap<String, Vec<ActionName>>`.
    #[serde(default)]
    pub phases: BTreeMap<String, Vec<ActionName>>,
}

impl Manifest {
    /// The ordered phase actions of the task type `task_type`; none when `[phases]` has no list
    /// for it: `Manifest::phase_list(type) -> Option<&[ActionName]>`.
    pub fn phase_list(&self, task_type: &str) -> Option<&[ActionName]> {
        self.phases.get(task_type).map(Vec::as_slice)
    }

    /// Reads, defaults and validates the one manifest file at `path`; an error names it.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let table = layers::read_table(path)?;
        Self::from_table(table).map_err(|error| error.in_files(&[path]))
    }

    /// Reads the manifest of the project at `root` from the files it holds (`.zaf.toml`,
    /// `.zaf/config.toml` or a lone legacy `ZAF.toml`), with the `config.toml` of the zaf
    /// `home` below them, combined property by property. Every error names its file.
    pub fn load_layers(root: &Path, home: Option<&Path>) -> Result<Self, ManifestError> {
        layers::load(root, home)
    }

    /// Parses, defaults and validates a merged table.
    pub(super) fn from_table(table: toml::Table) -> Result<Self, ManifestError> {
        let mut manifest: Self = table.try_into()?;
        manifest.fill_pack_defaults();
        manifest.validate()?;
        Ok(manifest)
    }

    /// Checks the rules serde cannot express: keys a kind needs, relative paths, ranges.
    pub fn validate(&self) -> Result<(), ManifestError> {
        self.validate_ports()?;
        self.validate_paths()?;
        self.validate_limits()?;
        self.validate_phases()
    }

    /// A phase list zaf could not run: empty, or naming an action twice.
    fn validate_phases(&self) -> Result<(), ManifestError> {
        for (task_type, list) in &self.phases {
            if list.is_empty() {
                return Err(ManifestError::EmptyPhaseList {
                    task_type: task_type.clone(),
                });
            }
            for (at, name) in list.iter().enumerate() {
                if list[..at].contains(name) {
                    return Err(ManifestError::PhaseListedTwice {
                        task_type: task_type.clone(),
                        name: name.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_ports(&self) -> Result<(), ManifestError> {
        match self.board.kind {
            BoardKind::GithubIssues => {
                require_key("board", "repo", self.board.kind, self.board.repo.is_some())?
            }
            BoardKind::GithubProjects => {
                require_key("board", "repo", self.board.kind, self.board.repo.is_some())?;
                require_key(
                    "board",
                    "project",
                    self.board.kind,
                    self.board.project.is_some(),
                )?;
            }
        }
        if let Some(forge) = &self.forge {
            match forge.kind {
                ForgeKind::Github => {
                    require_key("forge", "repo", forge.kind, forge.repo.is_some())?
                }
            }
        }
        match self.design.kind {
            DesignKind::GitSibling => {
                require_key(
                    "design",
                    "repo",
                    self.design.kind,
                    self.design.repo.is_some(),
                )?;
            }
            DesignKind::RepoDir => require_key(
                "design",
                "path",
                self.design.kind,
                self.design.path.is_some(),
            )?,
            DesignKind::None => {}
        }
        match self.isolation.kind {
            IsolationKind::Container => {
                require_key(
                    "isolation",
                    "image",
                    self.isolation.kind,
                    self.isolation.image.is_some(),
                )?;
            }
            IsolationKind::Process => {}
        }
        Ok(())
    }

    fn validate_paths(&self) -> Result<(), ManifestError> {
        require_relative("workspaces", "path", Some(&self.workspaces.path))?;
        require_relative("design", "path", self.design.path.as_ref())?;
        require_relative("design", "index", self.design.index.as_ref())?;
        require_relative(
            "style",
            "path",
            self.style.as_ref().map(|style| &style.path),
        )?;
        require_relative(
            "pack",
            "path",
            self.pack.as_ref().and_then(|pack| pack.path.as_ref()),
        )
    }

    fn validate_limits(&self) -> Result<(), ManifestError> {
        if !self.branches.task.as_ref().contains("{task}") {
            return Err(invalid(
                "branches",
                "task",
                "must hold `{task}`, or every task shares one branch",
            ));
        }
        if self.spawn.slots == 0 {
            return Err(invalid("spawn", "slots", "must be at least 1"));
        }
        let guard = &self.spawn.guard;
        let ascending = guard.windows(2).all(|pair| pair[0] < pair[1]);
        let in_range = guard.iter().all(|percent| (1..=100).contains(percent));
        if !ascending || !in_range {
            return Err(invalid(
                "spawn",
                "guard",
                "must be strictly ascending percentages from 1 to 100",
            ));
        }
        if self.actions.timeout_minutes == 0 {
            return Err(invalid("actions", "timeout_minutes", "must be at least 1"));
        }
        if self.ask.ladder.is_empty() {
            return Err(invalid("ask", "ladder", "must name at least one rung"));
        }
        if !(1..=MAX_QUESTIONS).contains(&self.ask.max) {
            return Err(invalid(
                "ask",
                "max",
                &format!("must be from 1 to {MAX_QUESTIONS}"),
            ));
        }
        if let Some(pack) = &self.pack
            && pack.path.is_some() == pack.tag.is_some()
        {
            return Err(invalid("pack", "path", "or `tag`: give exactly one"));
        }
        for (name, runner) in &self.runners {
            if runner.timeout_minutes == 0 {
                return Err(invalid(
                    "runners",
                    "timeout_minutes",
                    &format!("must be at least 1 (runner `{name}`)"),
                ));
            }
        }
        Ok(())
    }

    fn fill_pack_defaults(&mut self) {
        for (name, role) in RoleSection::pack_defaults() {
            self.roles.entry(RoleName::from(name)).or_insert(role);
        }
    }
}

impl FromStr for Manifest {
    type Err = ManifestError;

    /// Parses the manifest, fills the pack's defaults and validates the result.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::from_table(text.parse::<toml::Table>()?)
    }
}

fn require_key(
    section: &'static str,
    key: &'static str,
    kind: impl ToString,
    present: bool,
) -> Result<(), ManifestError> {
    if present {
        return Ok(());
    }
    let missing_key_error = ManifestError::MissingKey {
        section,
        key,
        kind: kind.to_string(),
    };
    Err(missing_key_error)
}

fn require_relative(
    section: &'static str,
    key: &'static str,
    path: Option<&PathBuf>,
) -> Result<(), ManifestError> {
    let Some(path) = path.filter(|path| path.is_absolute() || path.has_root()) else {
        return Ok(());
    };
    let absolute_path_error = ManifestError::AbsolutePath {
        section,
        key,
        path: path.clone(),
    };
    Err(absolute_path_error)
}

fn invalid(section: &'static str, key: &'static str, reason: &str) -> ManifestError {
    ManifestError::Invalid {
        section,
        key,
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests;
