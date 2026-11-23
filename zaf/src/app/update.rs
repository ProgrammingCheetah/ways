//! `zaf update`: replace this zaf with the newest release.
//!
//! Nothing is remembered between runs: the newest release is read from the
//! releases each time ([`newest`]), and the running version is the binary's own.
//! An update downloads this platform's archive and its `.sha256` beside the
//! binary, checks the sum, unpacks it, runs the new binary's `--version` to see it
//! is the release it claims to be, and only then renames it over the running one,
//! so a failure at any step leaves the old binary as it was.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::app::actions::ACTIONS_DIR;
use crate::app::doctor::Commands;
use crate::app::profile;
use crate::app::setup::{PackActions, changed_actions, machine_edits};
use crate::composition::{STATE_DIR, zaf_home};
use crate::domain::screen::{PlanRow, Text, Verb};
use crate::manifest::{FOLDER_FILE, LEGACY_FILE, Manifest, ROOT_FILE};
use crate::ports::{PortError, Releases, Shell};

/// The repository zaf is released from.
pub const HOME_REPO: &str = "ProgrammingCheetah/ways";

/// The prefix of every zaf release tag: `zaf-v0.2.1`.
pub const TAG_PREFIX: &str = "zaf-v";

/// The release archive's target for this build, or none where zaf publishes no archive.
pub fn target() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else {
        None
    }
}

/// The newest stable release among `tags`: the highest `zaf-v<semver>` with no pre-release
/// part, with its tag. Other tags are passed by.
pub fn newest(tags: &[String]) -> Option<(Version, String)> {
    tags.iter()
        .filter_map(|tag| {
            let version = Version::parse(tag.strip_prefix(TAG_PREFIX)?).ok()?;
            version.pre.is_empty().then(|| (version, tag.clone()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
}

/// Every stable release among `tags` newer than `current`, newest first, each with its tag
/// (the same parse as [`newest`]).
pub fn skipped(tags: &[String], current: &Version) -> Vec<(Version, String)> {
    let mut found: Vec<(Version, String)> = tags
        .iter()
        .filter_map(|tag| {
            let version = Version::parse(tag.strip_prefix(TAG_PREFIX)?).ok()?;
            (version.pre.is_empty() && version > *current).then(|| (version, tag.clone()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
}

/// The most lines `what_is_new` gives for a release.
const WHAT_IS_NEW_LINES: usize = 5;

/// What a release's notes say is new: the lines under a line reading `What is new` (a Markdown
/// heading of any level, or the bare words), past any blank lines after it, up to the next blank
/// line, each with a leading `- `, `* ` or `• ` taken off, at most five. Notes with no such section give their first
/// non-blank line; empty notes give nothing.
pub fn what_is_new(notes: &str) -> Vec<Text> {
    let mut lines = notes.lines();
    let found = lines.by_ref().any(|line| {
        line.trim()
            .trim_start_matches('#')
            .trim()
            .eq_ignore_ascii_case("what is new")
    });
    if found {
        return lines
            .skip_while(|line| line.trim().is_empty())
            .take_while(|line| !line.trim().is_empty())
            .take(WHAT_IS_NEW_LINES)
            .map(|line| Text::from(unbullet(line.trim())))
            .collect();
    }
    notes
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| vec![Text::from(line)])
        .unwrap_or_default()
}

/// `line` without a leading `- `, `* ` or `• `.
fn unbullet(line: &str) -> &str {
    ["- ", "* ", "• "]
        .iter()
        .find_map(|bullet| line.strip_prefix(bullet))
        .map_or(line, str::trim_start)
}

/// The release archive for `tag` and `target`, as the release workflow names it.
pub fn archive(tag: &str, target: &str) -> String {
    format!("{tag}-{target}.tar.gz")
}

/// What `zaf update` found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This zaf is the newest release, or newer.
    Current {
        /// The running version.
        version: Version,
    },
    /// A newer release is out; nothing was changed (a check).
    Behind {
        /// The running version.
        current: Version,
        /// The newest release.
        newest: Version,
    },
    /// The binary was replaced.
    Updated {
        /// The version that ran.
        from: Version,
        /// The version now installed.
        to: Version,
        /// The binary replaced.
        path: PathBuf,
    },
}

/// Why an update could not be made.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The releases could not be read or fetched.
    #[error(transparent)]
    Port(#[from] PortError),
    /// No stable `zaf-v*` release is published.
    #[error("no zaf release is published at {HOME_REPO}")]
    NoRelease,
    /// zaf publishes no archive for this platform.
    #[error(
        "no release archive for {platform}; build {tag} instead: cargo install --git https://github.com/{HOME_REPO} --tag {tag} --locked --force zaf"
    )]
    NoArchive {
        /// The OS and architecture.
        platform: String,
        /// The newest release's tag.
        tag: String,
    },
    /// A step of the install failed; the old binary is untouched.
    #[error("cannot install {tag}: {reason}")]
    Install {
        /// The release.
        tag: String,
        /// What failed.
        reason: String,
    },
}

/// Says whether a release newer than `current` is out, changing nothing.
pub fn check(releases: &dyn Releases, current: &Version) -> Result<Outcome, UpdateError> {
    let (newest, _) = newest(&releases.tags()?).ok_or(UpdateError::NoRelease)?;
    Ok(if &newest > current {
        Outcome::Behind {
            current: current.clone(),
            newest,
        }
    } else {
        Outcome::Current {
            version: current.clone(),
        }
    })
}

/// Replaces the binary at `exe` (version `current`) with the newest release for `target`,
/// when one is newer. `target` is none where zaf publishes no archive.
pub fn update(
    releases: &dyn Releases,
    current: &Version,
    exe: &Path,
    target: Option<&str>,
) -> Result<Outcome, UpdateError> {
    let (newest, tag) = newest(&releases.tags()?).ok_or(UpdateError::NoRelease)?;
    if &newest <= current {
        return Ok(Outcome::Current {
            version: current.clone(),
        });
    }
    let Some(target) = target else {
        return Err(UpdateError::NoArchive {
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            tag,
        });
    };
    let install = |reason: String| UpdateError::Install {
        tag: tag.clone(),
        reason,
    };
    let parent = exe
        .parent()
        .ok_or_else(|| install(format!("{} has no folder", exe.display())))?;
    // Beside the binary, so the final rename stays on one file system.
    let work = WorkDir::create(parent)
        .map_err(|error| install(format!("cannot write in {}: {error}", parent.display())))?;
    let name = archive(&tag, target);
    let sum = format!("{name}.sha256");
    releases.download(&tag, &[name.clone(), sum.clone()], work.path())?;
    verify_sum(&work.path().join(&name), &work.path().join(&sum)).map_err(install)?;
    let fresh = unpack(&work.path().join(&name), work.path(), &tag, target).map_err(install)?;
    probe(&fresh, &newest).map_err(install)?;
    std::fs::rename(&fresh, exe)
        .map_err(|error| install(format!("cannot replace {}: {error}", exe.display())))?;
    Ok(Outcome::Updated {
        from: current.clone(),
        to: newest,
        path: exe.to_owned(),
    })
}

/// A folder for one update's downloads, removed with everything in it when dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn create(parent: &Path) -> std::io::Result<Self> {
        let dir = parent.join(format!(".zaf-update-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Checks the archive against the first word of its `.sha256` file.
fn verify_sum(archive: &Path, sum_file: &Path) -> Result<(), String> {
    let expected = std::fs::read_to_string(sum_file)
        .map_err(|error| format!("cannot read {}: {error}", sum_file.display()))?;
    let expected = expected
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("{} is empty", sum_file.display()))?
        .to_lowercase();
    let actual = sha256(archive)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "the archive's sha256 is {actual}, but the release says {expected}"
        ))
    }
}

/// The file's sha256 in hex, from `sha256sum`, else `shasum -a 256`.
fn sha256(path: &Path) -> Result<String, String> {
    let tries: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];
    for (program, args) in tries {
        let Ok(output) = Command::new(program).args(args).arg(path).output() else {
            continue;
        };
        if output.status.success()
            && let Some(sum) = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
        {
            return Ok(sum.to_lowercase());
        }
    }
    Err("neither sha256sum nor shasum could hash the archive".to_owned())
}

/// Unpacks the archive into `dir` and returns its `zaf`, made executable.
fn unpack(archive: &Path, dir: &Path, tag: &str, target: &str) -> Result<PathBuf, String> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .status()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    if !status.success() {
        return Err(format!("tar could not unpack {}", archive.display()));
    }
    let fresh = dir.join(format!("{tag}-{target}")).join("zaf");
    if !fresh.is_file() {
        return Err(format!("the archive holds no {tag}-{target}/zaf"));
    }
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make {} executable: {error}", fresh.display()))?;
    Ok(fresh)
}

/// Runs the new binary's `--version` and checks it names `version`.
fn probe(fresh: &Path, version: &Version) -> Result<(), String> {
    let output = Command::new(fresh)
        .arg("--version")
        .output()
        .map_err(|error| format!("the new binary does not run: {error}"))?;
    let said = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && said == format!("zaf {version}") {
        Ok(())
    } else {
        Err(format!("the new binary says `{said}`, not `zaf {version}`"))
    }
}

/// The marketplace the teams plugin comes from.
const MARKETPLACE: &str = "ways";

/// The teams plugin as Claude Code names it.
pub const PLUGIN: &str = "teams@ways";

/// One step of `zaf update`, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The binary is replaced.
    Binary,
    /// The teams plugin in the Engineer's Claude home is brought to the marketplace's newest.
    Plugin,
    /// The machine's actions are brought to the installed plugin's.
    Actions,
    /// The project's `[pack] tag` is raised and its profiles made again.
    Pack,
}

/// The plugin row: what is installed and what the marketplace declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRow {
    /// The installed version, `None` when the plugin is not installed.
    pub installed: Option<Version>,
    /// The version `main` declares.
    pub to: Version,
}

/// The actions row: what differs between the installed plugin's actions and the machine's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsRow {
    /// The names new to the machine or different from its copy; empty when only the plugin's
    /// own update may bring new ones.
    pub changed: Vec<String>,
    /// How many actions the installed plugin carries.
    pub total: usize,
    /// The machine's actions equal to the old pack's copy, read before the plugin step replaces
    /// the pack: an `Assumed` run replaces each of these the new pack differs on.
    pub replaced: Vec<String>,
    /// The machine's actions that differ from the old pack's copy or have none to read: an
    /// `Assumed` run keeps each of these, by name, whatever the new pack holds.
    pub kept: Vec<String>,
}

/// What one release says is new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct News {
    /// The release.
    pub version: Version,
    /// Its lines, as [`what_is_new`] reads them.
    pub lines: Vec<Text>,
}

/// Where `zaf update` looks, for its plan.
#[derive(Debug, Clone, Copy)]
pub struct Places<'a> {
    /// The binary to replace.
    pub exe: &'a Path,
    /// The Engineer's Claude home (`~/.claude`).
    pub claude_home: Option<&'a Path>,
    /// The zaf home (`~/.zaf`).
    pub zaf_home: Option<&'a Path>,
    /// The project's root and manifest, inside one.
    pub project: Option<(&'a Path, &'a Manifest)>,
}

/// What `zaf update` would do, worked out first; making it changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The running version.
    pub current: Version,
    /// The newest release.
    pub newest: Version,
    /// The newest release's tag.
    pub tag: String,
    /// The binary to replace, when a newer release is out.
    pub binary: Option<PathBuf>,
    /// The plugin row, when the plugin is not at the marketplace's version.
    pub plugin: Option<PluginRow>,
    /// The actions row, with the zaf home it concerns.
    pub actions: Option<(ActionsRow, PathBuf)>,
    /// The line of the project's manifest the pack row raises.
    pub pack: Option<PackChange>,
    /// What each release newer than the running one says is new, newest first.
    pub news: Vec<News>,
}

impl Plan {
    /// The steps to run, in order, each with its row.
    pub fn rows(&self) -> Vec<(Step, PlanRow)> {
        let mut rows = Vec::new();
        if let Some(exe) = &self.binary {
            let folder = exe.parent().map(tilde).unwrap_or_default();
            rows.push((
                Step::Binary,
                row(
                    "Replace",
                    format!("zaf {} → {}", self.current, self.newest),
                    format!("the program itself, in {folder}"),
                ),
            ));
        }
        if let Some(plugin) = &self.plugin {
            let (verb, noun) = match &plugin.installed {
                Some(installed) => (
                    "Update",
                    format!("teams plugin {installed} → {}", plugin.to),
                ),
                None => ("Install", format!("teams plugin {}", plugin.to)),
            };
            rows.push((
                Step::Plugin,
                row(
                    verb,
                    noun,
                    "the roles zaf starts inside Claude Code".to_owned(),
                ),
            ));
        }
        if let Some((actions, home)) = &self.actions {
            let home = tilde(home);
            let noun = if actions.changed.is_empty() {
                format!("actions in {home}")
            } else {
                format!(
                    "{} of {} actions in {home}",
                    actions.changed.len(),
                    actions.total
                )
            };
            rows.push((
                Step::Actions,
                row(
                    "Update",
                    noun,
                    "the steps zaf runs; each change is shown first".to_owned(),
                ),
            ));
        }
        if let Some(change) = &self.pack {
            let file = change
                .file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            rows.push((
                Step::Pack,
                row(
                    "Update",
                    format!("pack tag in {file}"),
                    "the project's pack; its profiles are made again".to_owned(),
                ),
            ));
        }
        rows
    }
}

fn row(verb: &str, noun: String, why: String) -> PlanRow {
    PlanRow {
        verb: Verb::from(verb),
        noun: Text::from(noun),
        why: Text::from(why),
    }
}

/// `path` with the Engineer's home folder written `~`.
pub fn tilde(path: &Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && home != "/" => match shown.strip_prefix(&home) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => shown,
        },
        _ => shown,
    }
}

/// The `actions/` folder of the teams plugin installed in `claude_home`.
pub fn pack_actions_dir(claude_home: &Path) -> Option<PathBuf> {
    profile::installed_path(claude_home, PLUGIN).map(|path| path.join(ACTIONS_DIR))
}

/// Works out what `zaf update` would do at `places`, changing nothing: the binary when a newer
/// release is out for `target`, the plugin when it is not at the version `main` declares, the
/// machine's actions that differ from the installed plugin's, and inside a project its stale pack.
/// A row with nothing to do is left out. A plugin version that cannot be read leaves out the
/// plugin and pack rows; a release's notes that cannot be read leave out its lines.
pub fn plan(
    releases: &dyn Releases,
    current: &Version,
    target: Option<&str>,
    places: &Places<'_>,
) -> Result<Plan, UpdateError> {
    let tags = releases.tags()?;
    let (newest, tag) = newest(&tags).ok_or(UpdateError::NoRelease)?;
    let newer = newest > *current;
    if newer && target.is_none() {
        return Err(UpdateError::NoArchive {
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            tag,
        });
    }
    let news = if newer {
        skipped(&tags, current)
            .into_iter()
            .map(|(version, tag)| News {
                version,
                lines: releases
                    .notes(&tag)
                    .ok()
                    .flatten()
                    .map(|notes| what_is_new(&notes))
                    .unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let declared = releases.plugin_version().ok();
    let plugin = declared.as_ref().and_then(|to| {
        let installed = places
            .claude_home
            .and_then(|home| profile::installed_version(home, PLUGIN))
            .and_then(|text| Version::parse(&text).ok());
        (installed.as_ref() != Some(to)).then(|| PluginRow {
            installed,
            to: to.clone(),
        })
    });
    let actions = places.zaf_home.and_then(|home| {
        // The old pack copy, read here: the plugin step replaces it.
        let old_pack = places.claude_home.and_then(pack_actions_dir);
        let (changed, total) = old_pack
            .as_deref()
            .map(|folder| changed_actions(home, folder))
            .unwrap_or_default();
        let edits = machine_edits(home, old_pack.as_deref());
        (!changed.is_empty() || plugin.is_some()).then(|| {
            let row = ActionsRow {
                changed,
                total,
                replaced: edits.unedited,
                kept: edits.edited,
            };
            (row, home.to_owned())
        })
    });
    let pack = match (&declared, places.project) {
        (Some(to), Some((root, manifest))) => stale_pack(root, manifest, to),
        _ => None,
    };
    Ok(Plan {
        current: current.clone(),
        newest,
        tag,
        binary: newer.then(|| places.exe.to_owned()),
        plugin,
        actions,
        pack,
        news,
    })
}

/// A word on what the actions step did.
pub fn actions_summary(done: &PackActions) -> String {
    let mut parts = Vec::new();
    if !done.installed.is_empty() {
        parts.push(format!("{} new", done.installed.len()));
    }
    if !done.replaced.is_empty() {
        parts.push(format!("{} replaced", done.replaced.len()));
    }
    if parts.is_empty() {
        "actions as they were".to_owned()
    } else {
        format!("actions: {}", parts.join(", "))
    }
}

/// How the plugin step of `zaf update` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// The step did its work.
    Done(String),
    /// The step had nothing to do, or left the plugin as it is.
    Skipped(String),
    /// The step failed; the text names the command and its first error line.
    Failed(String),
}

/// Where `ways` stands in `claude plugin marketplace list`.
#[derive(Debug, PartialEq, Eq)]
enum Marketplace {
    Missing,
    Remote,
    Local,
}

/// Brings the teams plugin in the Engineer's own Claude home to the marketplace's newest.
///
/// The commands run in the environment as it is (no profile `CLAUDE_CONFIG_DIR`). A
/// marketplace that is not added yet is added and the plugin installed; one added from a
/// local folder is left as it is. No `claude`, or a command that fails, is a [`StepOutcome::Failed`]
/// naming the command and its first error line; it never stops the caller.
pub fn update_plugin(commands: &dyn Commands) -> StepOutcome {
    let listing = match claude(commands, &["plugin", "marketplace", "list"]) {
        Ok(text) => text,
        Err(failed) => return StepOutcome::Failed(failed),
    };
    let steps: (&[&[&str]], &str) = match marketplace_state(&listing) {
        Marketplace::Local => {
            return StepOutcome::Skipped(format!(
                "{MARKETPLACE} was added from a local folder; left as it is"
            ));
        }
        Marketplace::Remote => (
            &[
                &["plugin", "marketplace", "update", MARKETPLACE],
                &["plugin", "update", PLUGIN],
            ],
            "teams plugin updated",
        ),
        Marketplace::Missing => (
            &[
                &["plugin", "marketplace", "add", HOME_REPO],
                &["plugin", "install", PLUGIN],
            ],
            "teams plugin installed",
        ),
    };
    for args in steps.0 {
        if let Err(failed) = claude(commands, args) {
            return StepOutcome::Failed(failed);
        }
    }
    StepOutcome::Done(steps.1.to_owned())
}

/// Why a project with `[pack] path` keeps its pack as it is.
pub const PACK_FROM_FOLDER: &str = "pack from a folder; left as it is";

/// The one line of a manifest that raises a project's `[pack] tag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackChange {
    /// The manifest file holding the line.
    pub file: PathBuf,
    /// The line as it is, without its line end.
    pub old: String,
    /// The line as it would be.
    pub new: String,
}

/// One machine action the plugin's copy would replace: what the Engineer is shown to decide on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDiff {
    /// The action's name.
    pub name: String,
    /// Why the plugin's copy differs: the pack's `# changed:` line, when it says.
    pub why: Option<Text>,
    /// The two copies' difference as `git diff` prints it.
    pub difference: String,
}

/// Who gives the update plan its yes-or-no answers: `proceed` once, `replace` for each machine
/// action that differs and `raise_pack` for the project's pack line. A `true` goes ahead.
pub trait UpdateAnswers {
    /// Whether the plan as shown is carried out.
    fn proceed(&mut self) -> bool;
    /// Whether the plugin's copy of an action replaces the machine's.
    fn replace(&mut self, diff: &ActionDiff) -> bool;
    /// Whether the project's pack line is raised.
    fn raise_pack(&mut self, change: &PackChange) -> bool;
    /// Hands over the plan, once it is worked out and before the first answer. Answers that decide
    /// from what the plan read take it; the others leave it.
    fn planned(&mut self, _plan: &Plan) {}
}

/// The answers of a run no one is asked in: yes to `proceed` and `raise_pack`, and for `replace`
/// the edited-actions rule: once the plan is handed over, an action is replaced when the machine's
/// copy equals the old pack's, and kept when it differs or there is no old copy to read. Before a
/// plan, or with a hook, `replace` is the hook's answer, yes for every diff when there is none. It
/// reads nothing from standard input and writes nothing.
#[derive(Default)]
pub struct Assumed {
    replace: Option<ReplaceHook>,
    /// The names the plan lets replace; `None` before a plan is handed over.
    unedited: Option<Vec<String>>,
    /// The names answered no, in the order asked.
    kept: Vec<String>,
}

/// Answers whether one differing action is replaced.
type ReplaceHook = Box<dyn Fn(&ActionDiff) -> bool>;

impl Assumed {
    /// Answers `replace` with `hook`, the edited-actions rule.
    pub fn with_replace(hook: impl Fn(&ActionDiff) -> bool + 'static) -> Self {
        Self {
            replace: Some(Box::new(hook)),
            ..Self::default()
        }
    }

    /// The names of the actions it answered no on, in the order asked: what the last line names.
    pub fn kept(&self) -> &[String] {
        &self.kept
    }
}

impl UpdateAnswers for Assumed {
    fn proceed(&mut self) -> bool {
        true
    }

    fn replace(&mut self, diff: &ActionDiff) -> bool {
        let yes = match (&self.replace, &self.unedited) {
            (Some(hook), _) => hook(diff),
            (None, Some(unedited)) => unedited.contains(&diff.name),
            (None, None) => true,
        };
        if !yes {
            self.kept.push(diff.name.clone());
        }
        yes
    }

    fn raise_pack(&mut self, _change: &PackChange) -> bool {
        true
    }

    fn planned(&mut self, plan: &Plan) {
        self.unedited = plan.actions.as_ref().map(|(row, _)| row.replaced.clone());
    }
}

/// The reason a project's pack is left as it is: `[pack] path` names a folder.
pub fn skip_reason(manifest: &Manifest) -> Option<&'static str> {
    manifest
        .pack
        .as_ref()
        .is_some_and(|pack| pack.path.is_some())
        .then_some(PACK_FROM_FOLDER)
}

/// The change that raises the `[pack] tag` of the project at `root` to `newest`, keeping the
/// tag's prefix form and the line's quoting and spacing; changes nothing. `None` when the
/// project has no `[pack] tag` (none, or a `path`), or its tag is at or past `newest`.
pub fn stale_pack(root: &Path, manifest: &Manifest, newest: &Version) -> Option<PackChange> {
    let pack = manifest.pack.as_ref()?;
    if pack.path.is_some() {
        return None;
    }
    let (_, version) = split_tag(pack.tag.as_deref()?)?;
    if version >= *newest {
        return None;
    }
    [ROOT_FILE, FOLDER_FILE, LEGACY_FILE]
        .iter()
        .map(|name| root.join(name))
        .find_map(|file| {
            let text = std::fs::read_to_string(&file).ok()?;
            let (_, old) = tag_line(&text)?;
            let new = raised_line(old, newest)?;
            Some(PackChange {
                file,
                old: old.to_owned(),
                new,
            })
        })
}

/// A tag's prefix (everything before its first digit, as in `teams-v`) and its version.
fn split_tag(tag: &str) -> Option<(&str, Version)> {
    let tag = tag.trim();
    let at = tag.find(|c: char| c.is_ascii_digit())?;
    let version = Version::parse(&tag[at..]).ok()?;
    Some((&tag[..at], version))
}

/// The `tag = "..."` line of the `[pack]` table with its byte offset in `text`.
fn tag_line(text: &str) -> Option<(usize, &str)> {
    let mut in_pack = false;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_pack = trimmed == "[pack]"
                || trimmed
                    .strip_prefix("[pack]")
                    .is_some_and(|rest| rest.trim_start().starts_with('#'));
        } else if in_pack && quoted_tag(line).is_some() {
            return Some((start, line));
        }
    }
    None
}

/// The span of the tag's text between its quotes on a `tag = "..."` line.
fn quoted_tag(line: &str) -> Option<std::ops::Range<usize>> {
    let after_key = line.trim_start().strip_prefix("tag")?;
    let value = after_key.trim_start().strip_prefix('=')?.trim_start();
    let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let start = line.len() - value.len() + 1;
    let length = line[start..].find(quote)?;
    Some(start..start + length)
}

/// `line` with its tag's version replaced by `newest`, the prefix kept.
fn raised_line(line: &str, newest: &Version) -> Option<String> {
    let span = quoted_tag(line)?;
    let (prefix, _) = split_tag(&line[span.clone()])?;
    Some(format!(
        "{}{prefix}{newest}{}",
        &line[..span.start],
        &line[span.end..]
    ))
}

/// Rewrites the one line of `change` (every other byte of the file stays) and regenerates the
/// profiles from the manifest as it then reads. A refused generation is a failure naming it;
/// the rewritten line stays.
pub fn apply_pack(root: &Path, change: &PackChange, shell: &dyn Shell) -> StepOutcome {
    apply_pack_with(
        root,
        change,
        shell,
        zaf_home().as_deref(),
        profile::claude_home().as_deref(),
    )
}

/// [`apply_pack`] with the zaf home and the Claude home given.
fn apply_pack_with(
    root: &Path,
    change: &PackChange,
    shell: &dyn Shell,
    home: Option<&Path>,
    claude_home: Option<&Path>,
) -> StepOutcome {
    if let Err(failed) = rewrite_line(change) {
        return StepOutcome::Failed(failed);
    }
    let manifest = match Manifest::load_layers(root, home) {
        Ok(manifest) => manifest,
        Err(error) => {
            return StepOutcome::Failed(format!(
                "{} after the rewrite: {error}",
                change.file.display()
            ));
        }
    };
    match profile::init_with(root, &root.join(STATE_DIR), &manifest, shell, claude_home) {
        Ok(_) => StepOutcome::Done(format!("{}; profiles regenerated", change.new.trim())),
        Err(error) => StepOutcome::Failed(format!(
            "{} is rewritten, but the profiles were refused: {error}",
            change.file.display()
        )),
    }
}

/// Replaces the line `change.old` of its file with `change.new`, through a temporary file
/// renamed over it; nothing else in the file moves.
fn rewrite_line(change: &PackChange) -> Result<(), String> {
    let file = &change.file;
    let shown = file.display();
    let text = std::fs::read_to_string(file).map_err(|error| format!("{shown}: {error}"))?;
    let (start, line) = tag_line(&text)
        .filter(|(_, line)| *line == change.old)
        .ok_or_else(|| format!("{shown}: the line `{}` is no longer there", change.old))?;
    let end = start + line.len();
    let raised = format!("{}{}{}", &text[..start], change.new, &text[end..]);
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let temp = file.with_file_name(format!(".{name}.zaf-new"));
    let written = std::fs::write(&temp, raised)
        .and_then(|()| std::fs::set_permissions(&temp, std::fs::metadata(file)?.permissions()))
        .and_then(|()| std::fs::rename(&temp, file));
    written.map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("{shown}: {error}")
    })
}

/// Runs `claude` with `args`; an error is the command and its first error line.
fn claude(commands: &dyn Commands, args: &[&str]) -> Result<String, String> {
    let shown = format!("claude {}", args.join(" "));
    match commands.run("claude", args) {
        Err(error) => Err(format!("{shown}: {error}")),
        Ok(output) if output.success => Ok(output.text),
        Ok(output) => {
            let line = output
                .text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("failed without a message");
            Err(format!("{shown}: {line}"))
        }
    }
}

/// Reads `ways` out of the marketplace listing: a name line, then an indented
/// `Source: <kind> (<where>)` line; a `Directory` or `File` source is a local folder.
fn marketplace_state(listing: &str) -> Marketplace {
    let mut lines = listing.lines();
    let named = lines.by_ref().any(|line| {
        line.trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim_end()
            == MARKETPLACE
    });
    if !named {
        return Marketplace::Missing;
    }
    for line in lines {
        let line = line.trim();
        let Some(source) = line.strip_prefix("Source:") else {
            if line.is_empty() || !line.contains(':') {
                break; // the next marketplace's name, or the end of this entry
            }
            continue;
        };
        let source = source.trim().to_lowercase();
        let local = source.starts_with("directory") || source.starts_with("file");
        return if local {
            Marketplace::Local
        } else {
            Marketplace::Remote
        };
    }
    Marketplace::Remote
}

#[cfg(test)]
mod tests;
