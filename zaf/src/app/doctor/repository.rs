//! The repository rows of `zaf doctor` (design D27): the folder is a repository of the kind
//! `[vcs]` names, and, for a git project, git is new enough.
//!
//! A manifest with no `[vcs]` has neither row. `kind = "jj"` needs a `.jj` folder; `kind = "git"`
//! needs a `.git` (folder or file, as a worktree has) and no `.jj`, since a colocated repository
//! is one jj runs. Each failure names its fix. A git project also fails below [`GIT_FLOOR`], the
//! oldest git whose `merge-tree --write-tree` the lane relies on (docs/git-lane-spike.md §2).

use std::path::Path;

use super::Commands;
use super::paint::{bold, piece, with_commands};
use crate::domain::screen::{Line, Meaning};
use crate::manifest::VcsKind;

/// The oldest git the git lane works with: `merge-tree --write-tree --name-only` arrived in it.
pub const GIT_FLOOR: semver::Version = semver::Version::new(2, 38, 0);

/// What a git project needs of zaf: older binaries refuse `kind = "git"`.
const ZAF_FLOOR: &str = "0.5.3";

/// The folder, checked against the repository kind `[vcs]` names.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RepositoryCheck {
    /// The kind `[vcs]` names, `jj` or `git`.
    pub kind: String,
    /// Whether the folder is a repository of that kind.
    pub ok: bool,
    /// The failure and its fix, or what a passing git project needs of zaf; empty for a passing
    /// jj project.
    pub detail: String,
}

impl RepositoryCheck {
    /// `repository: ok (<kind>)`, or `repository: <detail>`; a passing git project says it needs
    /// zaf 0.5.3 or newer. As pieces: `ok` done, a failure red, a command in it cyan.
    pub(super) fn painted(&self) -> Line {
        let mut line = vec![bold("repository: ")];
        match (self.ok, self.detail.is_empty()) {
            (true, true) => line.extend([
                piece(Meaning::Done, "ok"),
                bold(format!(" ({})", self.kind)),
            ]),
            (true, false) => line.extend([
                piece(Meaning::Done, "ok"),
                bold(format!(" ({}), {}", self.kind, self.detail)),
            ]),
            (false, _) => line.extend(with_commands(Meaning::Failed, &self.detail)),
        }
        line
    }
}

/// `git --version`, checked against [`GIT_FLOOR`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GitVersionCheck {
    /// The installed version, when `git --version` printed one.
    pub found: Option<String>,
    /// The floor, as a version.
    pub floor: String,
    /// Whether git is at the floor or above.
    pub ok: bool,
    /// What is wrong, empty when `ok`.
    pub detail: String,
}

impl GitVersionCheck {
    /// `git version: ok (<found>)`, or `git version: <detail>`. As pieces: `ok` done, a failure
    /// red, a command in it cyan.
    pub(super) fn painted(&self) -> Line {
        let mut line = vec![bold("git version: ")];
        match (&self.found, self.ok) {
            (Some(found), true) => line.extend([
                piece(Meaning::Done, "ok"),
                bold(" ("),
                piece(Meaning::Noun, found),
                bold(")"),
            ]),
            _ => line.extend(with_commands(Meaning::Failed, &self.detail)),
        }
        line
    }
}

/// Checks that `root` is a repository of `kind`.
pub(super) fn check_repository(root: &Path, kind: VcsKind) -> RepositoryCheck {
    let jj = root.join(".jj").is_dir();
    let git = root.join(".git").exists();
    let (ok, detail) = match (kind, jj, git) {
        (VcsKind::Jj, true, _) => (true, String::new()),
        (VcsKind::Jj, false, true) => (
            false,
            "[vcs] kind = \"jj\" but the folder is a git repository: write kind = \"git\", or run `jj git init --colocate`".to_owned(),
        ),
        (VcsKind::Jj, false, false) => (
            false,
            "[vcs] kind = \"jj\" but the folder is not a repository: run `jj git init --colocate`"
                .to_owned(),
        ),
        (VcsKind::Git, true, _) => (
            false,
            "[vcs] kind = \"git\" but the folder is a jj repository: write kind = \"jj\""
                .to_owned(),
        ),
        (VcsKind::Git, false, true) => (
            true,
            format!(
                "a git project needs zaf {ZAF_FLOOR} or newer, older binaries refuse kind = \"git\""
            ),
        ),
        (VcsKind::Git, false, false) => (
            false,
            "[vcs] kind = \"git\" but the folder is not a repository: run `git init`".to_owned(),
        ),
    };
    RepositoryCheck {
        kind: kind.to_string(),
        ok,
        detail,
    }
}

/// Runs `git --version` and checks it against [`GIT_FLOOR`].
pub(super) fn check_git_version(commands: &dyn Commands) -> GitVersionCheck {
    let floor = GIT_FLOOR.to_string();
    let (found, ok, detail) = match commands.run("git", &["--version"]) {
        Err(_) => (
            None,
            false,
            format!("git is not installed: install git {floor} or newer"),
        ),
        Ok(output) => match super::first_semver(&output.text) {
            None => (
                None,
                false,
                format!("no version in `git --version`: the floor is git {floor}"),
            ),
            Some(version) if version >= GIT_FLOOR => {
                (Some(version.to_string()), true, String::new())
            }
            Some(version) => (
                Some(version.to_string()),
                false,
                format!("git {version} is older than the floor, git {floor}: upgrade git"),
            ),
        },
    };
    GitVersionCheck {
        found,
        floor,
        ok,
        detail,
    }
}
