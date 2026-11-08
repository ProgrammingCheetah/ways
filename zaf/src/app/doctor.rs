//! `zaf doctor`: verify the tools the manifest requires and the folders it names.
//!
//! The manifest itself is validated when it loads, so a report exists only for a
//! manifest that already parsed. Tools are asked for `<tool> --version` through
//! [`Commands`], which tests fake and the command line backs with real processes.
//! Each generated profile is asked whether it carries a login, by `claude auth status`
//! run with `CLAUDE_CONFIG_DIR` set to it; a missing login is reported with its fix,
//! never fixed, and does not fail the doctor. Its line also says what its `.credentials.json`
//! is: shared with the Claude home, or a copy that goes stale; the token is never printed.
//! Each profile that carries the pack reports the version [`profile::installed_version`] reads from
//! its `plugins/installed_plugins.json` (the reader `zaf profile init` uses), or that the pack is missing; a missing pack, or a version other than the source
//! pack's `plugin.json` declares, fails the report, and `zaf profile init` installs or updates it. Every action either tier provides is
//! reported with its validity and its mode; an invalid file fails the report (see [`ActionCheck`]).
//! Every action name the project needs is reported with the tier that provides it; a needed name
//! with no provider fails the report (see [`NeededCheck`]).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::actions::ACTIONS_DIR;
use crate::app::profile::{self, CONFIG_DIR_VAR, CREDENTIALS_FILE};
use crate::composition::STATE_DIR;
use crate::manifest::{DesignSection, Manifest, Requirement, ToolName};

mod actions;
pub mod fake;
mod needed;

pub use actions::ActionCheck;
pub use needed::NeededCheck;

/// Where a credentials file holds its access token's expiry, in milliseconds since the epoch.
const EXPIRES_AT: &str = "/claudeAiOauth/expiresAt";
/// A plugin's own manifest, under its source folder.
const PLUGIN_FILE: &str = ".claude-plugin/plugin.json";
/// What an installed pack that records no version is reported as.
const UNKNOWN_VERSION: &str = profile::UNKNOWN_VERSION;

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

/// What a profile's `.credentials.json` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Credentials {
    /// There is none: the login, if any, comes from elsewhere, such as `[auth] token_env`.
    Absent,
    /// A symlink to the file of the Claude home: the one live token.
    Shared,
    /// A symlink to anything else, which is never called shared.
    Foreign,
    /// A regular file, not past its `expiresAt` as far as it can be read.
    Copy,
    /// A regular file whose access token is past its `expiresAt`.
    Stale,
}

impl Credentials {
    /// The words the profile's line adds; `None` when there is no file to speak of.
    fn words(self) -> Option<&'static str> {
        match self {
            Self::Absent => None,
            Self::Shared => Some("shared"),
            Self::Foreign => Some("links outside the Claude home"),
            Self::Copy => Some("copy"),
            Self::Stale => Some("copy, stale"),
        }
    }
}

/// Whether a profile carries a login, as `claude auth status` inside it says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LoginCheck {
    /// The profile's name.
    pub profile: String,
    /// Its home, `CLAUDE_CONFIG_DIR` for its runs.
    pub path: PathBuf,
    /// Whether it is logged in; a profile not generated yet is not.
    pub ok: bool,
    /// What its credentials file is.
    pub credentials: Credentials,
}

impl LoginCheck {
    /// `profile <name>: login ok`, or `login missing`, then in brackets what its credentials
    /// file is and the fix as a worksheet line, whichever apply.
    pub fn line(&self) -> String {
        let state = if self.ok { "login ok" } else { "login missing" };
        let details = match (self.credentials.words(), self.fix()) {
            (None, None) => return format!("profile {}: {state}", self.profile),
            (Some(words), None) => words.to_owned(),
            (None, Some(fix)) => format!("fix: {fix}"),
            (Some(words), Some(fix)) => format!("{words}; fix: {fix}"),
        };
        format!("profile {}: {state} ({details})", self.profile)
    }

    /// What the Engineer does about this login; `None` when nothing is wrong yet.
    fn fix(&self) -> Option<String> {
        let relink = || {
            format!(
                "remove {} and run `zaf profile init`",
                self.path.join(CREDENTIALS_FILE).display()
            )
        };
        match (self.ok, self.credentials) {
            (true, Credentials::Absent | Credentials::Shared | Credentials::Copy) => None,
            (_, Credentials::Foreign | Credentials::Stale) | (false, Credentials::Copy) => {
                Some(relink())
            }
            (false, Credentials::Shared) => Some("log in to your Claude home".to_owned()),
            (false, Credentials::Absent) => Some(format!(
                "log in to your Claude home and run `zaf profile init`, which links its \
                 {CREDENTIALS_FILE} into {}, \
                 or set [auth] token_env = \"<VAR>\" in .zaf/local.toml and export that token",
                self.path.display()
            )),
        }
    }
}

/// Which pack version a profile has installed, as its `plugins/installed_plugins.json` says,
/// against the version the source pack declares.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackCheck {
    /// The profile's name.
    pub profile: String,
    /// The pack as `<plugin>@<marketplace>`, when the manifest names one Claude can know.
    pub pack: Option<String>,
    /// The version the profile has installed, when the pack is installed there.
    pub version: Option<String>,
    /// The version the source pack's `.claude-plugin/plugin.json` declares, when it declares one.
    pub source_version: Option<String>,
    /// Whether the pack is installed at the source's version (or the source declares none).
    pub ok: bool,
}

impl PackCheck {
    /// Whether the profile holds a version other than the one the source declares.
    ///
    /// An installed pack that records no version is never stale: there is nothing to compare.
    pub fn stale(&self) -> bool {
        match (&self.version, &self.source_version) {
            (Some(have), Some(wanted)) => have != UNKNOWN_VERSION && have != wanted,
            _ => false,
        }
    }

    /// `profile <name>: pack <pack>@<marketplace> <version> ok`; a stale pack names both versions
    /// and its fix; a missing one says `pack missing` and that `zaf profile init` installs it.
    pub fn line(&self) -> String {
        match (&self.pack, &self.version) {
            (Some(pack), Some(have)) if self.stale() => format!(
                "profile {}: pack {pack} {have} is stale: the source pack is {} \
                 (fix: zaf profile init updates it)",
                self.profile,
                self.source_version.as_deref().unwrap_or(UNKNOWN_VERSION)
            ),
            (Some(pack), Some(version)) => {
                format!("profile {}: pack {pack} {version} ok", self.profile)
            }
            _ => format!(
                "profile {}: pack missing (fix: zaf profile init installs it)",
                self.profile
            ),
        }
    }
}

/// Everything `doctor` found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    /// One entry per `[requires]` tool, in name order.
    pub tools: Vec<ToolCheck>,
    /// The design folder, when the design kind is not `none`.
    pub design: Option<PathCheck>,
    /// The design index file, when the design kind is not `none`.
    pub design_index: Option<PathCheck>,
    /// The workspaces folder, which must be creatable.
    pub workspaces: PathCheck,
    /// The project's own actions folder, `<root>/.zaf/actions`; `zaf setup` makes it.
    pub actions_dir: PathCheck,
    /// One entry per generated profile; informational, it never fails the report.
    pub logins: Vec<LoginCheck>,
    /// One entry per generated profile that carries the pack, when the manifest has a `[pack]`;
    /// a stale or a missing pack fails the report.
    pub packs: Vec<PackCheck>,
    /// One entry per action either tier provides, in name order; an invalid one fails the report.
    pub actions: Vec<ActionCheck>,
    /// Why the actions folders could not be read, when they could not; it fails the report.
    pub actions_error: Option<String>,
    /// One entry per action name the project needs, in name order, with the tier that provides it;
    /// a name with no provider fails the report.
    pub needed: Vec<NeededCheck>,
}

impl Report {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.tools.iter().all(|check| check.status.is_ok())
            && self.packs.iter().all(|check| check.ok)
            && self.actions.iter().all(|check| check.ok)
            && self.actions_error.is_none()
            && self.needed.iter().all(|check| check.ok)
            && self.design.as_ref().is_none_or(|check| check.ok)
            && self.design_index.as_ref().is_none_or(|check| check.ok)
            && self.workspaces.ok
            && self.actions_dir.ok
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
        lines.extend(self.design_index.iter().map(PathCheck::line));
        lines.push(self.actions_dir.line());
        lines.extend(self.logins.iter().map(LoginCheck::line));
        lines.extend(self.packs.iter().map(PackCheck::line));
        lines.extend(self.actions.iter().flat_map(ActionCheck::lines));
        lines.extend(self.actions_error.clone());
        lines.extend(self.needed.iter().flat_map(NeededCheck::lines));
        lines.push(self.workspaces.line());
        lines
    }
}

impl PathCheck {
    /// `<what>: ok (<path>)`, or the failure with the absolute path tried:
    /// `<what> not found: <path>`, and `workspaces dir not creatable: <path>`.
    fn line(&self) -> String {
        let path = self.path.display();
        match (self.what, self.ok) {
            (what, true) => format!("{what}: ok ({path})"),
            ("workspaces dir", false) => format!("workspaces dir not creatable: {path}"),
            (what, false) => format!("{what} not found: {path}"),
        }
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

/// Asks each generated profile under `<root>/.zaf/profiles/` whether it carries a login.
///
/// A profile whose folder is not there yet is missing without asking; one that is
/// there runs `env CLAUDE_CONFIG_DIR=<home> claude auth status`, which exits 0 only
/// when logged in. Each one's credentials file is judged against `claude_home` at `now`.
pub fn logins(
    root: &Path,
    manifest: &Manifest,
    commands: &dyn Commands,
    claude_home: Option<&Path>,
    now: SystemTime,
) -> Vec<LoginCheck> {
    let state_dir = root.join(STATE_DIR);
    profile::generated(manifest)
        .into_iter()
        .map(|name| {
            let path = profile::home(&state_dir, name);
            let ok = path.is_dir() && {
                let setting = format!("{CONFIG_DIR_VAR}={}", path.display());
                commands
                    .run("env", &[&setting, "claude", "auth", "status"])
                    .is_ok_and(|output| output.success)
            };
            let credentials = credentials(&path, claude_home, now);
            LoginCheck {
                profile: name.to_string(),
                path,
                ok,
                credentials,
            }
        })
        .collect()
}

/// What the `.credentials.json` of the profile at `home` is; it looks at the link itself,
/// so a link is never mistaken for the file it leads to.
fn credentials(home: &Path, claude_home: Option<&Path>, now: SystemTime) -> Credentials {
    let file = home.join(CREDENTIALS_FILE);
    let Ok(meta) = fs::symlink_metadata(&file) else {
        return Credentials::Absent;
    };
    if meta.file_type().is_symlink() {
        let shared = claude_home.is_some_and(|home| profile::links_to_claude_home(&file, home));
        return if shared {
            Credentials::Shared
        } else {
            Credentials::Foreign
        };
    }
    if !meta.is_file() {
        return Credentials::Absent;
    }
    if expiry(&file).is_some_and(|expires_at| is_past(expires_at, now)) {
        Credentials::Stale
    } else {
        Credentials::Copy
    }
}

/// The expiry a credentials file holds, in milliseconds since the epoch; `None` when it
/// cannot be read. Nothing else of the file leaves this function.
fn expiry(file: &Path) -> Option<u64> {
    let text = fs::read_to_string(file).ok()?;
    let credentials: serde_json::Value = serde_json::from_str(&text).ok()?;
    credentials.pointer(EXPIRES_AT)?.as_u64()
}

/// Whether `now` is past `expires_at`, in milliseconds since the epoch.
fn is_past(expires_at: u64, now: SystemTime) -> bool {
    now.duration_since(UNIX_EPOCH)
        .is_ok_and(|since| since.as_millis() > u128::from(expires_at))
}

/// Reads, for each generated profile that carries the pack, the version its
/// `plugins/installed_plugins.json` records, and compares it with the source pack's version.
///
/// Reads only: a profile not generated yet, or a pack the project's marketplace file cannot
/// name, is missing and fails the report; the fix is `zaf profile init`, which doctor never runs. Only a `[pack] path`
/// has a source version to compare with.
pub fn packs(root: &Path, manifest: &Manifest) -> Vec<PackCheck> {
    let Some(pack) = &manifest.pack else {
        return Vec::new();
    };
    let pack_ref = profile::pack_ref(root, pack).ok().flatten();
    let source_version = pack
        .path
        .as_deref()
        .and_then(|path| declared_version(&root.join(path)));
    let state_dir = root.join(STATE_DIR);
    profile::generated(manifest)
        .into_iter()
        .filter(|name| profile::carries_pack(*name))
        .map(|name| {
            let home = profile::home(&state_dir, name);
            let version = pack_ref
                .as_ref()
                .and_then(|pack_ref| profile::installed_version(&home, &pack_ref.to_string()));
            let mut check = PackCheck {
                profile: name.to_string(),
                pack: pack_ref.as_ref().map(ToString::to_string),
                version,
                source_version: source_version.clone(),
                ok: false,
            };
            check.ok = check.version.is_some() && !check.stale();
            check
        })
        .collect()
}

/// The version the pack's own `.claude-plugin/plugin.json` declares under `source`; `None` when
/// the file is missing, unreadable or declares none (the pack is not versioned).
fn declared_version(source: &Path) -> Option<String> {
    let text = fs::read_to_string(source.join(PLUGIN_FILE)).ok()?;
    let plugin: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = plugin.get("version")?.as_str()?.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// Checks the required tools, the design folder, the workspaces folder, the profiles' logins and
/// packs and every action of a loaded manifest; `zaf_home` is the machine's tier of actions.
pub fn doctor(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
) -> Report {
    doctor_with(
        root,
        zaf_home,
        manifest,
        commands,
        profile::claude_home().as_deref(),
        SystemTime::now(),
    )
}

/// [`doctor`] against a given Claude home and clock.
pub fn doctor_with(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
    claude_home: Option<&Path>,
    now: SystemTime,
) -> Report {
    let tools = manifest
        .requires
        .iter()
        .map(|(tool, requirement)| check_tool(commands, tool, requirement))
        .collect();
    // Both paths come from the manifest's own functions, as for `ask`, `brief` and `setup`.
    let design_index = manifest.design.index_path(root).map(|path| PathCheck {
        what: "design index",
        ok: path.is_file(),
        path,
    });
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
    let actions_path = root.join(STATE_DIR).join(ACTIONS_DIR);
    let actions_dir = PathCheck {
        what: "actions dir",
        ok: actions_path.is_dir(),
        path: actions_path,
    };
    let (actions, actions_error) = actions::check_actions(root, zaf_home, manifest);
    let needed = needed::check_needed(root, zaf_home, manifest);
    Report {
        tools,
        design,
        design_index,
        workspaces,
        actions_dir,
        logins: logins(root, manifest, commands, claude_home, now),
        packs: packs(root, manifest),
        actions,
        actions_error,
        needed,
    }
}

#[cfg(test)]
mod tests;
