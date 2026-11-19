//! The build start (design 0.5.2, M20, I9 and I10): the one step that opens a version's build.
//!
//! [`start`] checks first and acts after. It refuses, changing nothing, when the release folder's
//! objectives (or scope) or design is not signed or was edited since, and when the repository has no
//! remote. Then it runs the actions `base-open` and `papers-commit` through the one actions runner,
//! with `ZAF_VERSION`, `ZAF_BASE` and `ZAF_RELEASE_DIR` set, and reads [`Started`] from what they
//! printed. The board, the signatures, the scheduler and its lock are not touched here, and nobody
//! calling it is told anything but the result.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::actions::{
    Caller, MINUTE, NO_SUCH_ACTION, Nesting, PhaseEnv, RunOutcome, RunRequest, RunResult,
    SliceContext, Terminal, run_action,
};
use super::release::{Phase, ReleaseFolder, phase_due};
use crate::composition::{Composition, STATE_DIR, zaf_home};

/// The action that opens the integration branch on the remote.
const OPEN_ACTION: &str = "base-open";

/// The action that opens the release pull request.
const RELEASE_OPEN_ACTION: &str = "release-open";

/// The action that commits the release folder to the integration branch.
const COMMIT_ACTION: &str = "papers-commit";

/// What the actions run as: the phase name they carry in `ZAF_PHASE`.
const PHASE: &str = "build";

/// What `base-open` prints when it makes the branch.
const MADE_PREFIX: &str = "made ";

/// What `papers-commit` prints when it commits.
const COMMITTED_PREFIX: &str = "committed ";

/// The placeholder for the version in the integration branch pattern.
const VERSION_PLACEHOLDER: &str = "{version}";

/// What the start did, for the one line each caller prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The integration branch (`ZAF_BASE`), made now or handed back.
    pub branch: String,
    /// Whether the branch was made by this start (`false`: it was on the remote already).
    pub made: bool,
    /// The commit the papers went in, `None` when the branch held them as they stand.
    pub commit: Option<String>,
}

/// The release pull request `release-open` handed back (design 0.5.2, I12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePullRequest {
    /// The pull request's number.
    pub number: String,
    /// Its link, when the action gave one.
    pub link: Option<String>,
}

/// Why the build did not start.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    /// A document is missing, unsigned, malformed, or edited since it was signed. Nothing was
    /// changed.
    #[error(
        "{document} is not signed, or was edited after signing; run `zaf begin --version <version>` to sign it again"
    )]
    Unsigned {
        /// The document's file name: `objectives.md`, `scope.md` or `design.md`.
        document: String,
    },
    /// The repository has no remote to open the integration branch on. Nothing was changed.
    #[error("the repository has no remote: the integration branch is opened on one")]
    NoRemote,
    /// An action refused (exit 1), in its own sentence.
    #[error("{0}")]
    Refused(String),
    /// A backend failed, an action failed (any other exit) or never ran.
    #[error("{0}")]
    Backend(String),
}

impl BuildError {
    /// The sentence a caller prints for `version`: for a document not signed it names the version in
    /// the command to run, as [`Display`](std::fmt::Display) cannot.
    pub fn sentence(&self, version: &str) -> String {
        match self {
            Self::Unsigned { document } => format!(
                "{document} is not signed, or was edited after signing; run `zaf begin --version {version}` to sign it again"
            ),
            other => other.to_string(),
        }
    }
}

/// `start(composition, version)`: opens `version`'s build from signed papers.
///
/// Refuses with [`BuildError::Unsigned`] naming the objectives or the design document that is not
/// signed, and with [`BuildError::NoRemote`] when the repository has none, before any action runs.
/// Then runs `base-open` and `papers-commit`, the actions of the project's or the machine's tier,
/// as the Engineer would. An action's exit 1 is [`BuildError::Refused`] with the sentence it ended
/// on, and so is an action found in neither tier (the sentence names it and says to run `zaf update`
/// again); any other failure, and an action that never ran for another reason, is
/// [`BuildError::Backend`].
pub fn start(composition: &Composition, version: &str) -> Result<Started, BuildError> {
    let root = composition.root();
    // With no home there is no machine tier, so the project's actions alone are looked for.
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    start_with_home(composition, version, &home)
}

/// [`start`] with the zaf home given instead of read from the environment.
pub(crate) fn start_with_home(
    composition: &Composition,
    version: &str,
    home: &Path,
) -> Result<Started, BuildError> {
    let root = composition.root();
    let folder = ReleaseFolder::of(root, composition.manifest(), version);
    check_signed(&folder, version)?;

    let vcs = composition
        .vcs()
        .map_err(|error| BuildError::Backend(error.to_string()))?;
    let remote = vcs.has_remote(root).map_err(|error| {
        BuildError::Backend(format!("cannot tell if there is a remote: {error}"))
    })?;
    if !remote {
        return Err(BuildError::NoRemote);
    }

    let branch = integration_branch(composition, version)?;
    let run = Runs {
        composition,
        home,
        version,
        folder: folder.path(),
        vcs: vcs.as_ref(),
    };
    let opened = run.action(OPEN_ACTION)?;
    let committed = run.action(COMMIT_ACTION)?;
    Ok(Started {
        made: line_after(&opened, MADE_PREFIX).is_some(),
        commit: line_after(&committed, COMMITTED_PREFIX),
        branch,
    })
}

/// `open_release(composition, version)`: runs the action `release-open` for `version` and hands
/// back the pull request it names, the one already open when there is one. An action's exit 1 is
/// [`BuildError::Refused`] with the sentence it ended on; any other failure, an action that never
/// ran, and an answer that holds no pull request number are [`BuildError::Backend`].
pub fn open_release(
    composition: &Composition,
    version: &str,
) -> Result<ReleasePullRequest, BuildError> {
    let root = composition.root();
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    open_release_with_home(composition, version, &home)
}

/// [`open_release`] with the zaf home given instead of read from the environment.
pub fn open_release_with_home(
    composition: &Composition,
    version: &str,
    home: &Path,
) -> Result<ReleasePullRequest, BuildError> {
    let folder = ReleaseFolder::of(composition.root(), composition.manifest(), version);
    let vcs = composition
        .vcs()
        .map_err(|error| BuildError::Backend(error.to_string()))?;
    let run = Runs {
        composition,
        home,
        version,
        folder: folder.path(),
        vcs: vcs.as_ref(),
    };
    let printed = run.action_with(RELEASE_OPEN_ACTION, &[OsString::from(version)])?;
    pull_request_of(&printed)
}

/// The pull request `release-open` printed: its number, then its link, as the last two non-empty
/// lines of its log (I12). The log also holds what the action said on stderr, before them.
fn pull_request_of(printed: &str) -> Result<ReleasePullRequest, BuildError> {
    let mut lines = printed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev();
    let link = lines.next().unwrap_or_default();
    let number = lines.next().unwrap_or_default();
    let is_number = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !is_number(number) || is_number(link) {
        return Err(BuildError::Backend(format!(
            "{RELEASE_OPEN_ACTION} gave no pull request number: {number:?}"
        )));
    }
    Ok(ReleasePullRequest {
        number: number.to_owned(),
        link: Some(link.to_owned()),
    })
}

/// Refuses unless the objectives and the design are both signed.
fn check_signed(folder: &ReleaseFolder, version: &str) -> Result<(), BuildError> {
    let document = match phase_due(folder, &[], version).phase {
        Phase::Nothing => return Ok(()),
        Phase::Objectives => folder
            .objectives_document()
            .unwrap_or_else(|| folder.path().join("objectives.md")),
        Phase::Design => folder.design_document(),
    };
    let name = document.file_name().map_or_else(
        || document.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Err(BuildError::Unsigned { document: name })
}

/// The integration branch of `version`, from `[branches] integration`.
fn integration_branch(composition: &Composition, version: &str) -> Result<String, BuildError> {
    let pattern: &str = composition.manifest().branches.integration.as_ref();
    let branch = pattern.replace(VERSION_PLACEHOLDER, version);
    if branch.contains("{task}") || branch.contains("{epic}") {
        return Err(BuildError::Backend(format!(
            "[branches] integration {pattern} names a ticket: there is none to build a version from"
        )));
    }
    Ok(branch)
}

/// What one action run needs, shared by both.
struct Runs<'a> {
    composition: &'a Composition,
    home: &'a Path,
    version: &'a str,
    folder: &'a Path,
    vcs: &'a dyn crate::ports::Vcs,
}

impl Runs<'_> {
    /// Runs the action `name` and hands back what it printed, or why it did not end well.
    fn action(&self, name: &str) -> Result<String, BuildError> {
        self.action_with(name, &[])
    }

    /// [`Runs::action`] with arguments.
    fn action_with(&self, name: &str, args: &[OsString]) -> Result<String, BuildError> {
        let root = self.composition.root();
        let manifest = self.composition.manifest();
        let env = manifest
            .env
            .resolve(root, Some(self.version))
            .map_err(|error| BuildError::Backend(error.to_string()))?;
        let slice = SliceContext {
            board: Err("the build start reads no board".to_owned()),
            forge: Err("the build start reads no forge".to_owned()),
            vcs: Ok(self.vcs),
            branches: &manifest.branches,
            version: Some(self.version),
        };
        let request = RunRequest {
            name,
            args,
            cwd: root,
            project_root: Some(root),
            home: self.home,
            timeout_minutes: manifest.actions.timeout_minutes,
            minute: MINUTE,
            nesting: Nesting::default(),
            caller: Caller::Engineer,
            only: None,
            slice: Some(slice),
            env,
            terminal: Terminal::Absent,
            run: None,
            phase: None,
        }
        .with_phase(PhaseEnv {
            version: self.version.to_owned(),
            phase: PHASE.to_owned(),
            release_dir: PathBuf::from(self.folder),
            input: None,
        });
        judge(&run_action(&request))
    }
}

/// What a run printed, or the error its end is.
fn judge(outcome: &RunOutcome) -> Result<String, BuildError> {
    let printed = outcome
        .log
        .as_deref()
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|error| {
            BuildError::Backend(format!("cannot read the log of {}: {error}", outcome.name))
        })?
        .unwrap_or_default();
    match outcome.result {
        RunResult::Ok => Ok(printed),
        RunResult::NotRun if outcome.meaning.as_deref() == Some(NO_SUCH_ACTION) => {
            Err(BuildError::Refused(format!(
                "{} is missing: run `zaf update` again; it installs the new actions",
                outcome.name
            )))
        }
        RunResult::NotRun => Err(BuildError::Backend(
            outcome
                .headline()
                .unwrap_or_else(|| format!("NOT_RUN {}", outcome.name)),
        )),
        RunResult::Fail if outcome.exit == 1 => Err(BuildError::Refused(
            last_sentence(&printed)
                .or_else(|| outcome.meaning.clone())
                .unwrap_or_else(|| format!("{} refused", outcome.name)),
        )),
        RunResult::Fail => Err(BuildError::Backend(
            outcome
                .headline()
                .unwrap_or_else(|| format!("FAIL {}", outcome.name)),
        )),
    }
}

/// The last line that says anything: what a refusing action ends on.
fn last_sentence(printed: &str) -> Option<String> {
    printed
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
}

/// What follows `prefix` on the first line of `printed` that starts with it.
fn line_after(printed: &str, prefix: &str) -> Option<String> {
    printed
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(|rest| rest.trim().to_owned())
        .filter(|rest| !rest.is_empty())
}

#[cfg(test)]
mod tests;
