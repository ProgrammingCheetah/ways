//! The sections of `ZAF.toml`, with the pack's defaults for the ones a project leaves out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// The design index's file name when `[design] index` names none.
pub const DEFAULT_DESIGN_INDEX: &str = "index.md";

impl DesignSection {
    /// Where the design pages are on disk, for a project rooted at `root`; `None` for kind `none`.
    ///
    /// `path` is relative to the project root, so `path = "../x-design"` lands beside the
    /// project. A `git-sibling` without `path` defaults to `<parent of root>/<repo name>`.
    /// Every reader of the design folder goes through this, so none can disagree.
    pub fn dir(&self, root: &Path) -> Option<PathBuf> {
        match self.kind {
            DesignKind::None => None,
            DesignKind::RepoDir => self.path.as_ref().map(|path| root.join(path)),
            DesignKind::GitSibling => match &self.path {
                Some(path) => Some(root.join(path)),
                None => {
                    let repo = self.repo.as_ref()?.as_ref();
                    let name = repo.rsplit('/').next().unwrap_or(repo);
                    let name = name.strip_suffix(".git").unwrap_or(name);
                    Some(root.parent().unwrap_or(root).join(name))
                }
            },
        }
    }

    /// Where the design index is on disk, for a project rooted at `root`: `[design] index`
    /// (default [`DEFAULT_DESIGN_INDEX`]) inside [`dir`](Self::dir); `None` for kind `none`.
    /// `doctor`, the design adapter, `ask` and `brief` all take the index from here.
    pub fn index_path(&self, root: &Path) -> Option<PathBuf> {
        let index = self
            .index
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_DESIGN_INDEX));
        self.dir(root).map(|dir| dir.join(index))
    }
}

/// `[style]`: the project's style file. Absent means the project declares none.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleSection {
    /// The style file, relative to the project root.
    pub path: PathBuf,
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
    /// The command prefix of the machine's build slot: with it a gate line runs as
    /// `<slot> sh -c '<line>'`, so every gate level waits for and holds the slot.
    #[serde(default)]
    pub slot: Option<CommandLine>,
}

/// `[runners.<name>]`: a command that runs a role, with `{prompt}`, `{dir}` and `{root}` placeholders (`{model}` and `{effort}` for a role).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSection {
    /// The command line.
    pub command: CommandLine,
    /// Minutes before a run is killed. Default: 45.
    #[serde(default = "default_runner_timeout_minutes")]
    pub timeout_minutes: u64,
}

/// The runner timeout when `[runners.<name>]` gives none.
fn default_runner_timeout_minutes() -> u64 {
    45
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

/// How long after the `ci` step starts an empty check list still counts as pending, in seconds.
pub const DEFAULT_CI_GRACE_SECONDS: u64 = 120;

/// `[check]`: the `ci` grace and, beside it, one `[check.<target>]` table per target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct ChecksSection {
    /// Seconds after the `ci` step starts during which an empty check list is pending, because
    /// the forge lists no checks for a while after a push. Default: 120.
    pub ci_grace_seconds: u64,
    /// The targets; one left out runs no steps.
    #[serde(flatten)]
    pub targets: BTreeMap<CheckTarget, CheckSection>,
}

impl Default for ChecksSection {
    fn default() -> Self {
        Self {
            ci_grace_seconds: DEFAULT_CI_GRACE_SECONDS,
            targets: BTreeMap::new(),
        }
    }
}

/// `[check.<target>]`: the steps run for that target, in the step syntax of the check verb.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckSection {
    /// The steps.
    pub steps: Vec<String>,
}

/// The action timeout when `[actions]` gives none, in minutes.
pub const DEFAULT_ACTION_TIMEOUT_MINUTES: u64 = 30;

/// `[actions]`: how long an action may run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionsSection {
    /// Minutes before an action run is killed, at least 1. Default: 30.
    pub timeout_minutes: u64,
}

impl Default for ActionsSection {
    fn default() -> Self {
        Self {
            timeout_minutes: DEFAULT_ACTION_TIMEOUT_MINUTES,
        }
    }
}

/// `[ask]`: the ladder of models a question climbs when a quote check fails.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AskSection {
    /// The rungs, in order. Default: `haiku`, then `sonnet`.
    pub ladder: Vec<Rung>,
    /// The most questions per ask, 1 to 5. Default: 5.
    pub max: usize,
    /// Hours an answered question is served from the log. Default: 24.
    pub cache_hours: u64,
}

impl Default for AskSection {
    fn default() -> Self {
        Self {
            ladder: vec![Rung::from("haiku"), Rung::from("sonnet")],
            max: crate::domain::MAX_QUESTIONS,
            cache_hours: 24,
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

/// `[pack]`: where the pack a profile carries comes from: a local checkout or a release tag.
///
/// Exactly one key: `path` (the pack's folder, relative to the project root) or `tag`
/// (a release installed from the marketplace). Absent means the profiles carry no pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackSection {
    /// The pack's folder, relative to the project root.
    pub path: Option<PathBuf>,
    /// The release tag installed from the marketplace.
    pub tag: Option<String>,
}

impl PackSection {
    /// The pack's folder on disk for a project rooted at `root`, when `path` is given.
    pub fn dir(&self, root: &Path) -> Option<PathBuf> {
        self.path.as_ref().map(|path| root.join(path))
    }
}

/// `[forbidden]`: commands no profile may run. Default: none.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForbiddenSection {
    /// The commands.
    pub commands: Vec<CommandLine>,
}

/// The placeholder in an `[env]` value for the absolute project root.
const ROOT_PLACEHOLDER: &str = "{root}";

/// The placeholder in an `[env]` value for the version of the epic.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The prefix of the variables zaf sets itself; no `[env]` key may start with it.
const RESERVED_PREFIX: &str = "ZAF_";

/// The variable that points a Claude process at its profile's home; zaf sets it for each
/// profile, so no `[env]` key may be it.
const PROFILE_HOME_VAR: &str = "CLAUDE_CONFIG_DIR";

/// `[env]`: variables every child zaf starts gets, as `NAME = "value"` pairs.
///
/// A value may use `{root}` for the absolute project root and `{version}` for the version
/// of the epic. No name may start with `ZAF_` (whatever its case): those are zaf's own, and a
/// manifest setting `ZAF_INSTANCE` would dodge the modes. `CLAUDE_CONFIG_DIR` is refused too:
/// zaf sets it for each profile, and a manifest pointing it elsewhere would take an instance
/// out of its profile's denies. The check runs when the manifest
/// loads, so a bad table never reaches a child. Default: no variables.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "BTreeMap<String, toml::Value>")]
pub struct EnvSection {
    vars: BTreeMap<String, String>,
}

impl TryFrom<BTreeMap<String, toml::Value>> for EnvSection {
    type Error = String;

    fn try_from(table: BTreeMap<String, toml::Value>) -> Result<Self, Self::Error> {
        let mut vars = BTreeMap::new();
        for (name, value) in table {
            check_env_name(&name)?;
            let toml::Value::String(text) = value else {
                return Err(format!(
                    "[env] `{name}` must be a string, got {}",
                    value.type_str()
                ));
            };
            if text.contains('\0') {
                return Err(format!("[env] `{name}` must not hold a NUL character"));
            }
            vars.insert(name, text);
        }
        Ok(Self { vars })
    }
}

impl EnvSection {
    /// The variables as written, placeholders unresolved, by name.
    pub fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }

    /// Whether the table sets nothing.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The variables with `{root}` and `{version}` filled in, by name.
    ///
    /// `root` is the absolute project root; `version` is the epic's version, which a value
    /// that uses `{version}` needs: without one the error names that variable. Any other
    /// text, braces included, is kept as written.
    pub fn resolve(
        &self,
        root: &Path,
        version: Option<&str>,
    ) -> Result<BTreeMap<String, String>, super::ManifestError> {
        let root = root.to_string_lossy();
        let mut resolved = BTreeMap::new();
        for (name, value) in &self.vars {
            let mut text = String::with_capacity(value.len());
            let mut rest = value.as_str();
            while let Some(at) = rest.find('{') {
                text.push_str(&rest[..at]);
                rest = &rest[at..];
                if let Some(after) = rest.strip_prefix(ROOT_PLACEHOLDER) {
                    text.push_str(&root);
                    rest = after;
                } else if let Some(after) = rest.strip_prefix(VERSION_PLACEHOLDER) {
                    let Some(version) = version else {
                        return Err(super::ManifestError::EnvNeedsVersion { name: name.clone() });
                    };
                    text.push_str(version);
                    rest = after;
                } else {
                    text.push('{');
                    rest = &rest[1..];
                }
            }
            text.push_str(rest);
            resolved.insert(name.clone(), text);
        }
        Ok(resolved)
    }
}

/// Refuses a name a child process cannot carry or zaf reserves.
fn check_env_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    if !starts_well || !chars.all(|rest| rest.is_ascii_alphanumeric() || rest == '_') {
        return Err(format!(
            "[env] `{name}` is not a variable name: use letters, digits and `_`, not starting with a digit"
        ));
    }
    if name.to_ascii_uppercase().starts_with(RESERVED_PREFIX) {
        return Err(format!(
            "[env] `{name}` starts with `{RESERVED_PREFIX}`: those variables are zaf's own and no manifest sets them"
        ));
    }
    if name.eq_ignore_ascii_case(PROFILE_HOME_VAR) {
        return Err(format!(
            "[env] `{name}` is not allowed: zaf sets it for each profile, and no manifest may point an instance away from its profile's denies"
        ));
    }
    Ok(())
}
