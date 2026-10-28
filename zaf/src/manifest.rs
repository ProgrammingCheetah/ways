//! `ZAF.toml`: everything zaf knows about a project.
//!
//! zaf is project-blind: no project word in the binary, no assumed language,
//! VCS, forge or design source. Absent sections take the pack's defaults;
//! an external thing left undeclared is absent, and the checks that need it are dropped.

mod document;
mod errors;
mod requirement;
mod sections;

pub use document::Manifest;
pub use errors::ManifestError;
pub use requirement::{Requirement, RequirementError};
pub use sections::{
    AskSection, BoardKind, BoardSection, BranchPattern, BranchesSection, CheckSection, CheckTarget,
    CommandLine, DesignKind, DesignSection, Effort, ForbiddenSection, ForgeKind, ForgeSection,
    GateSection, IsolationKind, IsolationSection, ModelName, ProjectName, ProjectSection, RepoName,
    RoleSection, Rung, RunnerName, RunnerSection, SpawnSection, ToolName, VcsKind, VcsSection,
    WorkspacesSection,
};
