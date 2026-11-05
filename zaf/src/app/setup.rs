//! `zaf setup`: make the two homes, fetch the design repo, run doctor, hand over what is left.
//!
//! It never installs a tool: every tool doctor reports as missing, too old or
//! unknown becomes a step of a worksheet for the Engineer. Running it again
//! changes nothing that is already in place.
//!
//! The folders are [`ensure_homes`]: the project's `.zaf/` and the zaf home, created when
//! missing and never rewritten. Every other verb runs it first; only an explicit `setup`
//! goes further and replaces an old `.zaf/.gitignore` that holds just `*`.
//!
//! A root `ZAF.toml` is renamed to `.zaf.toml` by [`move_legacy_manifest`], and only here:
//! `setup` outside an instance. Every other verb reads the old name and warns.
//!
//! A profile already generated gets its login linked to the Claude home's before doctor asks
//! about it. `setup` generates no profile: that takes the pack's commands, `zaf profile init`'s.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::doctor::{Commands, Report, ToolStatus, design_dir, doctor_with};
use super::profile;
use crate::composition::STATE_DIR;
use crate::manifest::{DesignKind, HOME_FILE, LEGACY_FILE, Manifest, ProjectFiles, ROOT_FILE};

/// The folders under the project's `.zaf/` that `setup` makes.
pub const PROJECT_DIRS: [&str; 3] = ["actions", "roles", "logs"];
/// The folders under the zaf home that `setup` makes.
pub const HOME_DIRS: [&str; 1] = ["actions"];
/// The file in `.zaf/` that keeps the machine's state out of version control.
pub const IGNORE_FILE: &str = ".gitignore";
/// The lines of `.zaf/.gitignore`: an allow-list, so actions and roles stay tracked.
pub const IGNORE_LINES: [&str; 7] = [
    "*",
    "!.gitignore",
    "!config.toml",
    "!actions/",
    "!actions/**",
    "!roles/",
    "!roles/**",
];
/// The zaf home's `config.toml` when `setup` makes it: every line a comment.
pub const HOME_CONFIG: &str = "\
# zaf machine config: defaults every project on this machine inherits.
# A project's .zaf.toml and .zaf/config.toml sit above this file and win, property by
# property; an array is replaced whole, never merged. Everything here is commented out.
#
# [actions]
# timeout_minutes = 30
";

/// What `.zaf/.gitignore` held when the homes were ensured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreState {
    /// It was absent and the allow-list was written.
    Written,
    /// It already is the allow-list.
    Current,
    /// It holds only `*`, the old file: [`ensure_homes`] leaves it, `setup` replaces it.
    Star,
    /// It holds anything else: kept, and the Engineer merges the allow-list by hand.
    Foreign,
    /// It held only `*` and `setup` replaced it with the allow-list.
    Replaced,
}

/// What [`ensure_homes`] found and made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Homes {
    /// Every folder and file it created, in order; empty when both homes were complete.
    pub made: Vec<PathBuf>,
    /// The homes where something was made, for the one line a verb prints.
    pub touched: Vec<PathBuf>,
    /// The state of the project's `.zaf/.gitignore`; `None` outside a project.
    pub ignore: Option<IgnoreState>,
}

impl Homes {
    /// The one line a verb prints on stderr when it made something; `None` when it made nothing.
    pub fn notice(&self) -> Option<String> {
        if self.made.is_empty() {
            return None;
        }
        let places: Vec<String> = self
            .touched
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        Some(format!(
            "zaf: made what was missing in {}",
            places.join(", ")
        ))
    }
}

/// Creates what is missing of the project's `.zaf/` (under `root`, when in a project) and of the
/// zaf `home` (when there is one): the folders, the commented `config.toml` and the
/// allow-list `.gitignore`. Anything already there is left exactly as it is, an old
/// `.gitignore` included; no doctor, no clone, no worksheet.
pub fn ensure_homes(root: Option<&Path>, home: Option<&Path>) -> io::Result<Homes> {
    let mut homes = Homes::default();
    if let Some(root) = root {
        let state = root.join(STATE_DIR);
        let before = homes.made.len();
        for dir in PROJECT_DIRS {
            make_dir(&state.join(dir), &mut homes.made)?;
        }
        let ignore = state.join(IGNORE_FILE);
        homes.ignore = Some(if make_file(&ignore, &ignore_text(), &mut homes.made)? {
            IgnoreState::Written
        } else {
            classify_ignore(&ignore)?
        });
        if homes.made.len() > before {
            homes.touched.push(state);
        }
    }
    if let Some(home) = home {
        let before = homes.made.len();
        make_dir(home, &mut homes.made)?;
        for dir in HOME_DIRS {
            make_dir(&home.join(dir), &mut homes.made)?;
        }
        make_file(&home.join(HOME_FILE), HOME_CONFIG, &mut homes.made)?;
        if homes.made.len() > before {
            homes.touched.push(home.to_owned());
        }
    }
    Ok(homes)
}

fn ignore_text() -> String {
    let mut text = IGNORE_LINES.join("\n");
    text.push('\n');
    text
}

fn make_dir(path: &Path, made: &mut Vec<PathBuf>) -> io::Result<()> {
    if !path.is_dir() {
        fs::create_dir_all(path)?;
        made.push(path.to_owned());
    }
    Ok(())
}

/// Writes `text` to `path` unless a file is there; whether it wrote.
fn make_file(path: &Path, text: &str, made: &mut Vec<PathBuf>) -> io::Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            made.push(path.to_owned());
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

fn classify_ignore(path: &Path) -> io::Result<IgnoreState> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return Ok(IgnoreState::Foreign);
        }
        Err(error) => return Err(error),
    };
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    Ok(if lines == IGNORE_LINES {
        IgnoreState::Current
    } else if lines == ["*"] {
        IgnoreState::Star
    } else {
        IgnoreState::Foreign
    })
}

/// Whether the project's own `.gitignore` ignores the `.zaf/` folder itself; git cannot
/// re-include a file under an ignored folder, so the allow-list would do nothing.
pub fn root_ignores_state_dir(root: &Path) -> bool {
    let Ok(text) = fs::read_to_string(root.join(".gitignore")) else {
        return false;
    };
    text.lines().map(str::trim).any(|line| {
        matches!(
            line,
            ".zaf" | ".zaf/" | "/.zaf" | "/.zaf/" | "**/.zaf" | "**/.zaf/"
        )
    })
}

/// What [`move_legacy_manifest`] did with a root `ZAF.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyMove {
    /// There is no root `ZAF.toml`: nothing to move.
    Absent,
    /// It was renamed to `.zaf.toml`.
    Moved,
    /// It is there, and an instance does not move it: the move is the Engineer's.
    ForEngineer,
}

impl LegacyMove {
    /// The one line `setup` prints about the move; `None` when there was nothing to say.
    pub fn line(self) -> Option<String> {
        match self {
            Self::Absent => None,
            Self::Moved => Some(format!("moved {LEGACY_FILE} to {ROOT_FILE}")),
            Self::ForEngineer => Some(format!(
                "{LEGACY_FILE} stays: moving it to {ROOT_FILE} is for the Engineer, who runs `zaf setup` outside an instance"
            )),
        }
    }
}

/// Renames the project's root `ZAF.toml` to `.zaf.toml`, but only for the Engineer: with
/// `as_instance` set (a `ZAF_INSTANCE` run) it moves nothing and says so. The only place
/// that moves the file; every other verb reads it. A `ZAF.toml` beside `.zaf.toml` or
/// `.zaf/config.toml` is refused as the manifest layers refuse it, naming both, and nothing
/// moves. A second call finds nothing to move.
pub fn move_legacy_manifest(
    root: &Path,
    home: Option<&Path>,
    as_instance: bool,
) -> io::Result<LegacyMove> {
    let files = ProjectFiles::find(root, home);
    let Some(legacy) = &files.legacy else {
        return Ok(LegacyMove::Absent);
    };
    files
        .refuse_legacy_beside_new()
        .map_err(|error| io::Error::new(io::ErrorKind::AlreadyExists, error.to_string()))?;
    if as_instance {
        return Ok(LegacyMove::ForEngineer);
    }
    fs::rename(legacy, root.join(ROOT_FILE))?;
    Ok(LegacyMove::Moved)
}

/// What happened to the design repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOutcome {
    /// `gh repo clone` ran and succeeded.
    Cloned,
    /// The folder was already there.
    Present,
    /// The clone failed; the message is what it said.
    Failed(String),
}

/// One thing the Engineer must do that `setup` cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The action, one sentence.
    pub action: String,
    /// How the Engineer knows it is done.
    pub done_when: String,
}

/// What `setup` did and what remains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    /// What the homes held and what was made.
    pub homes: Homes,
    /// The design repo, when the manifest declares one to clone.
    pub clone: Option<CloneOutcome>,
    /// Doctor, run after the folders and the clone.
    pub doctor: Report,
    /// The Engineer's steps; empty when nothing is left.
    pub worksheet: Vec<Step>,
}

impl SetupReport {
    /// Whether nothing is left for the Engineer.
    pub fn is_ready(&self) -> bool {
        self.worksheet.is_empty() && self.doctor.passed()
    }

    /// The worksheet as numbered lines, each step with its "Done when", and a closing "Tell me".
    pub fn worksheet_lines(&self) -> Vec<String> {
        if self.worksheet.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for (number, step) in self.worksheet.iter().enumerate() {
            lines.push(format!("{}. {}", number + 1, step.action));
            lines.push(format!("   Done when: {}", step.done_when));
        }
        lines.push("Tell me: the output of `zaf doctor`.".to_owned());
        lines
    }
}

fn clone_design(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Option<CloneOutcome> {
    if manifest.design.kind != DesignKind::GitSibling {
        return None;
    }
    let repo = manifest.design.repo.as_ref()?;
    let dir = design_dir(root, &manifest.design)?;
    if dir.is_dir() {
        return Some(CloneOutcome::Present);
    }
    let target = dir.to_string_lossy();
    let outcome = match commands.run("gh", &["repo", "clone", repo.as_ref(), &target]) {
        Ok(output) if output.success => CloneOutcome::Cloned,
        Ok(output) => CloneOutcome::Failed(output.text.trim().to_owned()),
        Err(error) => CloneOutcome::Failed(error.to_string()),
    };
    Some(outcome)
}

fn worksheet(
    manifest: &Manifest,
    root: &Path,
    clone: Option<&CloneOutcome>,
    report: &Report,
    homes: &Homes,
) -> Vec<Step> {
    let mut steps = Vec::new();
    for check in report.failing_tools() {
        let tool = &check.tool;
        let wanted = &check.wanted;
        let action = match &check.status {
            ToolStatus::Missing => format!("Install `{tool}` (the manifest requires `{wanted}`)."),
            ToolStatus::TooOld { found, .. } => {
                format!("Upgrade `{tool}` from {found} to a version satisfying `{wanted}`.")
            }
            ToolStatus::Unknown | ToolStatus::Ok { .. } => {
                format!("Make `{tool} --version` print a semantic version satisfying `{wanted}`.")
            }
        };
        steps.push(Step {
            action,
            done_when: format!("`{tool} --version` prints a version satisfying `{wanted}`."),
        });
    }
    if let (Some(CloneOutcome::Failed(why)), Some(repo), Some(dir)) = (
        clone,
        manifest.design.repo.as_ref(),
        design_dir(root, &manifest.design),
    ) {
        steps.push(Step {
            action: format!(
                "Clone the design repo: `gh repo clone {repo} {}` (it said: {why}).",
                dir.display()
            ),
            done_when: format!("`{}` exists.", dir.display()),
        });
    }
    if !report.workspaces.ok {
        steps.push(Step {
            action: format!(
                "Make `{}` creatable, or point `[workspaces] path` at a folder that is.",
                report.workspaces.path.display()
            ),
            done_when: "`zaf doctor` prints `workspaces dir: ok`.".to_owned(),
        });
    }
    if homes.ignore == Some(IgnoreState::Foreign) {
        steps.push(Step {
            action: format!(
                "Merge these lines into `{STATE_DIR}/{IGNORE_FILE}` by hand, keeping your own: {}.",
                IGNORE_LINES.join(", ")
            ),
            done_when: format!("`{STATE_DIR}/{IGNORE_FILE}` holds every one of those lines."),
        });
    }
    if root_ignores_state_dir(root) {
        steps.push(Step {
            action: format!(
                "In the root `.gitignore`, change the `{STATE_DIR}/` line to `{STATE_DIR}/*`: git cannot track \
                 actions and roles under an ignored folder."
            ),
            done_when: format!(
                "`git check-ignore {STATE_DIR}/actions/x` prints nothing."
            ),
        });
    }
    steps
}

/// Links the login of every generated profile whose folder is already there to `claude_home`.
fn link_logins(root: &Path, manifest: &Manifest, claude_home: &Path) -> io::Result<()> {
    let state_dir = root.join(STATE_DIR);
    for name in profile::generated(manifest) {
        let home = profile::home(&state_dir, name);
        if home.is_dir() {
            profile::link_credentials(&home, claude_home)?;
        }
    }
    Ok(())
}

/// Ensures both homes, replaces an old `.zaf/.gitignore` that holds only `*`, clones a declared
/// design repo that is missing, links the logins of the profiles already there, runs doctor
/// and builds the worksheet. Idempotent.
pub fn setup(
    root: &Path,
    home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
) -> io::Result<SetupReport> {
    let claude_home = profile::claude_home();
    setup_with(root, home, manifest, commands, claude_home.as_deref())
}

/// [`setup`] against a given Claude home; `None`, when there is none, links nothing.
fn setup_with(
    root: &Path,
    home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
    claude_home: Option<&Path>,
) -> io::Result<SetupReport> {
    let mut homes = ensure_homes(Some(root), home)?;
    if homes.ignore == Some(IgnoreState::Star) {
        fs::write(root.join(STATE_DIR).join(IGNORE_FILE), ignore_text())?;
        homes.ignore = Some(IgnoreState::Replaced);
    }
    let clone = clone_design(root, manifest, commands);
    if let Some(claude_home) = claude_home {
        link_logins(root, manifest, claude_home)?;
    }
    let report = doctor_with(
        root,
        home,
        manifest,
        commands,
        claude_home,
        SystemTime::now(),
    );
    let worksheet = worksheet(manifest, root, clone.as_ref(), &report, &homes);
    Ok(SetupReport {
        homes,
        clone,
        doctor: report,
        worksheet,
    })
}

#[cfg(test)]
mod tests;
