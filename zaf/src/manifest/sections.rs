//! The sections of `ZAF.toml`, with the pack's defaults for the ones a project leaves out.

use std::path::PathBuf;

use crate::domain::Profile;
use crate::newtype::text_newtype;

text_newtype! {
    /// The project's name, only for logs and ticket prefixes.
    ProjectName
}

text_newtype! {
    /// A repository on a forge or board, as `owner/name`.
    RepoName
}

text_newtype! {
    /// A tool named under `[requires]`.
    ToolName
}

text_newtype! {
    /// A runner named under `[runners.<name>]`.
    RunnerName
}

text_newtype! {
    /// A model a role runs on.
    ModelName
}

text_newtype! {
    /// A rung of the ask ladder: a runner or a model, tried in order.
    Rung
}

text_newtype! {
    /// A branch name pattern with `{version}`, `{epic}` and `{task}` placeholders.
    BranchPattern
}

text_newtype! {
    /// A shell command line, possibly with `{placeholders}`.
    CommandLine
}

/// `[project]`: who the manifest is for.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    /// The project's name.
    pub name: ProjectName,
}

/// The board backends zaf knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum BoardKind {
    /// Issues of a forge repository, with sub-issues and labels.
    GithubIssues,
}

/// `[board]`: required; the one port every project has.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardSection {
    /// The backend.
    pub kind: BoardKind,
    /// The repository holding the issues.
    #[serde(default)]
    pub repo: Option<RepoName>,
}

/// The VCS backends zaf knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum VcsKind {
    /// Jujutsu.
    Jj,
}

/// `[vcs]`: absent means no VCS port.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VcsSection {
    /// The backend.
    pub kind: VcsKind,
}

/// The forge backends zaf knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ForgeKind {
    /// A GitHub repository.
    Github,
}

/// `[forge]`: absent means no forge port and no PR checks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeSection {
    /// The backend.
    pub kind: ForgeKind,
    /// The code repository.
    #[serde(default)]
    pub repo: Option<RepoName>,
}

/// The design sources zaf knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum DesignKind {
    /// A separate repository cloned beside the code.
    GitSibling,
    /// A folder committed in the code repository.
    RepoDir,
    /// No design source: the tickets, epics and rulings are the design.
    #[default]
    None,
}

/// `[design]`: where the design pages live. Default: none.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DesignSection {
    /// The source.
    pub kind: DesignKind,
    /// The repository, for `git-sibling`.
    pub repo: Option<RepoName>,
    /// Where the pages are, relative to the project.
    pub path: Option<PathBuf>,
    /// The page index, relative to the design path.
    pub index: Option<PathBuf>,
}

/// `[workspaces]`: where slice workspaces go.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacesSection {
    /// A path relative to the project. Default: `.zaf/workspaces`.
    pub path: PathBuf,
}

impl Default for WorkspacesSection {
    fn default() -> Self {
        Self {
            path: PathBuf::from(".zaf/workspaces"),
        }
    }
}

/// `[branches]`: how integration and task branches are named.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BranchesSection {
    /// One per release. Default: `feature/{version}`.
    pub integration: BranchPattern,
    /// One per task; must hold `{task}`. Default: `feature/{version}-{epic}-{task}`.
    pub task: BranchPattern,
}

impl Default for BranchesSection {
    fn default() -> Self {
        Self {
            integration: BranchPattern::from("feature/{version}"),
            task: BranchPattern::from("feature/{version}-{epic}-{task}"),
        }
    }
}

/// `[gate]`: the project's own checks. Absent means no gate port.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSection {
    /// Everything CI runs.
    pub full: CommandLine,
    /// The quick subset.
    #[serde(default)]
    pub light: Option<CommandLine>,
    /// One named test, with `{name}` and other placeholders.
    #[serde(default)]
    pub tests: Option<CommandLine>,
}

/// `[runners.<name>]`: a command that runs a role, with `{prompt}`, `{dir}` placeholders.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSection {
    /// The command line.
    pub command: CommandLine,
}

/// How hard a model works on a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Effort {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
    /// Extra high.
    Xhigh,
    /// Maximum.
    Max,
}

/// `[roles.<name>]`: how a role runs. Roles the project leaves out take the pack's defaults.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSection {
    /// The model.
    pub model: ModelName,
    /// The effort.
    pub effort: Effort,
    /// The context profile.
    pub profile: Profile,
    /// The turn budget.
    pub turns: u32,
    /// How many tasks one instance takes in a batch, for roles that batch.
    #[serde(default)]
    pub batch: Option<u32>,
}

impl RoleSection {
    /// The pack's roles: a programmer and a team instance.
    pub(crate) fn pack_defaults() -> [(&'static str, Self); 2] {
        let programmer = Self {
            model: ModelName::from("sonnet"),
            effort: Effort::High,
            profile: Profile::Lean,
            turns: 60,
            batch: None,
        };
        let team = Self {
            model: ModelName::from("opus"),
            effort: Effort::Xhigh,
            profile: Profile::Build,
            turns: 200,
            batch: Some(4),
        };
        [("programmer", programmer), ("team", team)]
    }
}

/// What `zaf check` can be pointed at.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize, strum::Display,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CheckTarget {
    /// A task: its acceptance checks.
    Task,
    /// A pull request: CI, bots, runner reviews.
    Pr,
    /// A milestone: objectives and release reviews.
    Milestone,
}

/// `[check.<target>]`: the steps run for that target, in the step syntax of the check verb.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSection {
    /// The steps.
    pub steps: Vec<String>,
}

/// `[ask]`: the ladder of models a question climbs when a quote check fails.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AskSection {
    /// The rungs, in order. Default: `haiku`, then `sonnet`.
    pub ladder: Vec<Rung>,
    /// The most questions per ask, 1 to 5. Default: 5.
    pub max: usize,
}

impl Default for AskSection {
    fn default() -> Self {
        Self {
            ladder: vec![Rung::from("haiku"), Rung::from("sonnet")],
            max: crate::domain::MAX_QUESTIONS,
        }
    }
}

/// `[spawn]`: how many instances run at once and when the budget guard steps in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpawnSection {
    /// Concurrent instances, at least 1. Default: 1.
    pub slots: u32,
    /// Budget percentages at which the guard tightens, ascending. Default: 70, 85, 95.
    pub guard: Vec<u8>,
}

impl Default for SpawnSection {
    fn default() -> Self {
        Self {
            slots: 1,
            guard: vec![70, 85, 95],
        }
    }
}

/// How instances are kept apart from the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IsolationKind {
    /// Hermetic profiles and lock dirs in the project.
    #[default]
    Process,
    /// Every instance in a declared image.
    Container,
}

/// `[isolation]`: default `process`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IsolationSection {
    /// The level.
    pub kind: IsolationKind,
    /// The image, for `container`.
    pub image: Option<String>,
}

/// `[forbidden]`: commands no profile may run. Default: none.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForbiddenSection {
    /// The commands.
    pub commands: Vec<CommandLine>,
}
