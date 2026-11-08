//! The manifest: everything zaf knows about a project.
//!
//! It is read from `.zaf.toml`, `.zaf/config.toml` and the `config.toml` of the zaf home,
//! combined property by property (a root `ZAF.toml` standing alone still loads).
//!
//! zaf is project-blind: no project word in the binary, no assumed language,
//! VCS, forge or design source. Absent sections take the pack's defaults;
//! an external thing left undeclared is absent, and the checks that need it are dropped.

mod document;
mod errors;
mod layers;
mod requirement;
mod sections;

pub use document::Manifest;
pub use errors::ManifestError;
pub use layers::{
    FOLDER_FILE, HOME_FILE, LEGACY_FILE, ProjectFiles, ROOT_FILE, workspaces_path_lenient,
};
pub use requirement::{Requirement, RequirementError};
pub use sections::{
    ActionsSection, AskSection, BoardKind, BoardSection, BranchPattern, BranchesSection,
    CheckSection, CheckTarget, ChecksSection, CommandLine, DEFAULT_DESIGN_INDEX, DesignKind,
    DesignSection, Effort, EnvSection, ForbiddenSection, ForgeKind, ForgeSection, GateSection,
    IsolationKind, IsolationSection, ModelName, PackSection, ProjectName, ProjectSection, RepoName,
    RoleSection, Rung, RunnerName, RunnerSection, SpawnSection, StyleSection, ToolName, VcsKind,
    VcsSection, WorkspacesSection,
};
