//! Resolving an action name through the two tiers (docs/zaf-actions.md §2, §3).
//!
//! The project's `<root>/.zaf/actions/<name>` beats the machine's
//! `<home>/actions/<name>`. There is no built-in table: a name in no tier
//! resolves to nothing, and no name is special here. The project tier is read
//! from the project root alone, so an action file inside a slice workspace is
//! never found: an edit to an action runs only once it has merged and reached
//! the root.
//!
//! A file in a tier's `hooks` folder is a hook, never an action: a name under `hooks/` resolves to
//! nothing and is refused by name ([`ResolveError::Hook`]; `zaf do hooks/pre-tick` exits 125 and
//! its body never runs), the listing leaves the folder out, and
//! [`hook`] finds one by its event under the same precedence as an action.
//!
//! Finding the file reads no metadata: [`can_shadow`] is what a run holds the shadowed file's mode
//! against.

use std::path::{Path, PathBuf};

use super::metadata::{ActionMode, marks_jj};
use crate::composition::STATE_DIR;
use crate::manifest::VcsKind;

/// The suffix of an action's git version: on a git project `push.git` stands for `push`.
pub const GIT_SUFFIX: &str = ".git";

/// The folder under a tier's home that holds its actions.
pub const ACTIONS_DIR: &str = "actions";

/// The folder under a tier's actions folder that holds its hooks.
pub const HOOKS_DIR: &str = "hooks";

/// The events a hook runs on, which name its file.
pub const HOOK_EVENTS: [&str; 3] = ["pre-tick", "post-tick", "post-phase"];

/// Where an action file lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum Tier {
    /// The project's committed `.zaf/actions/`.
    Project,
    /// The machine's `~/.zaf/actions/` (or `$ZAF_HOME/actions/`).
    Machine,
}

/// A name that resolved to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The action's name: the file name, as is.
    pub name: String,
    /// The tier the file is in.
    pub tier: Tier,
    /// The file.
    pub path: PathBuf,
    /// The tier of the file this one shadows, when both tiers have the name.
    pub shadows: Option<Tier>,
}

/// Why a name could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The name is empty, has whitespace or a path separator, or climbs out of
    /// the actions folder.
    #[error("bad action name {name:?}: {reason}")]
    BadName {
        /// The name as given.
        name: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// The name is under the `hooks` folder: a hook runs on its event, not as an action.
    #[error("a hook runs on its event, not as an action")]
    Hook,
    /// A git project has the name only as files marked `vcs: jj`, and no `<name>.git`.
    #[error("no git version of `{0}`")]
    NoGitVersion(String),
    /// An actions folder or a file in it could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The folder or file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// Checks a name: no whitespace or control characters, no path separator, not
/// `.` or `..`. Hyphens, dots and underscores are fine and no extension is
/// dropped.
pub fn check_name(name: &str) -> Result<(), ResolveError> {
    if is_hook_name(name) {
        return Err(ResolveError::Hook);
    }
    let bad = |reason| ResolveError::BadName {
        name: name.to_owned(),
        reason,
    };
    if name.is_empty() {
        return Err(bad("it is empty"));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("it has whitespace"));
    }
    if name.contains(['/', '\\']) {
        return Err(bad("it has a path separator"));
    }
    if name == "." || name == ".." {
        return Err(bad("it is a directory reference"));
    }
    Ok(())
}

/// Whether `name` is under the `hooks` folder, `hooks/<event>`.
fn is_hook_name(name: &str) -> bool {
    name.strip_prefix(HOOKS_DIR)
        .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// The actions folder of a tier: the project root's `.zaf/actions` or the
/// machine home's `actions`.
fn tier_dir(tier: Tier, project_root: &Path, machine_home: &Path) -> PathBuf {
    match tier {
        Tier::Project => project_root.join(STATE_DIR).join(ACTIONS_DIR),
        Tier::Machine => machine_home.join(ACTIONS_DIR),
    }
}

/// Whether `path` is a file (a symlink to one counts); an absent path is not.
fn is_file(path: &Path) -> Result<bool, ResolveError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(ResolveError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// The file at `relative` under one tier's actions folder; none when that tier has no such file.
/// The name it carries is `relative` as given.
fn file_in(
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    relative: &str,
) -> Result<Option<Resolved>, ResolveError> {
    let path = tier_dir(tier, project_root, machine_home).join(relative);
    Ok(is_file(&path)?.then(|| Resolved {
        name: relative.to_owned(),
        tier,
        path,
        shadows: None,
    }))
}

/// The file at `relative` as the two tiers have it: the project's, which shadows the machine's.
fn file_in_tiers(
    project_root: &Path,
    machine_home: &Path,
    relative: &str,
) -> Result<Option<Resolved>, ResolveError> {
    let project = file_in(Tier::Project, project_root, machine_home, relative)?;
    let machine = file_in(Tier::Machine, project_root, machine_home, relative)?;
    Ok(match (project, machine) {
        (Some(project), Some(_)) => Some(Resolved {
            shadows: Some(Tier::Machine),
            ..project
        }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    })
}

/// The file `name` is in one tier; none when that tier has no such file.
pub fn resolve_in(
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    resolve_in_for(None, tier, project_root, machine_home, name)
}

/// The file `name` resolves to: the project's, else the machine's; none when
/// neither tier has it.
pub fn resolve(
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    resolve_for(None, project_root, machine_home, name)
}

/// Whether a project of `vcs` takes the git version of an action: only a git project does. A jj
/// project, and one with no `[vcs]`, look up a name as the file of that name alone.
fn takes_git(vcs: Option<VcsKind>) -> bool {
    vcs == Some(VcsKind::Git)
}

/// Whether the file at `path` carries the metadata line `vcs: jj`. A file that cannot be read is
/// not marked: it is found, and the run says what is wrong with it.
fn marked_jj(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| marks_jj(&String::from_utf8_lossy(&bytes)))
}

/// What `name` is in one tier for a project of `vcs`. On a git project that is the tier's
/// `<name>.git` when it has one, else its `<name>` unless that is marked `vcs: jj`; otherwise
/// the file of that name alone. The second field is whether the tier holds a `vcs: jj` file that
/// was passed over. The file's name is `name`, as asked, whichever file stands for it.
fn pick_in(
    vcs: Option<VcsKind>,
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<(Option<Resolved>, bool), ResolveError> {
    if !takes_git(vcs) {
        return Ok((file_in(tier, project_root, machine_home, name)?, false));
    }
    let git = format!("{name}{GIT_SUFFIX}");
    if let Some(found) = file_in(tier, project_root, machine_home, &git)? {
        return Ok((
            Some(Resolved {
                name: name.to_owned(),
                ..found
            }),
            false,
        ));
    }
    Ok(match file_in(tier, project_root, machine_home, name)? {
        Some(found) if marked_jj(&found.path) => (None, true),
        found => (found, false),
    })
}

/// [`resolve_in`] for a project of `vcs`. A name every file of which is marked `vcs: jj`, on a
/// git project, is [`ResolveError::NoGitVersion`], not none: the tier has the action and not a
/// version of it that runs on git.
pub fn resolve_in_for(
    vcs: Option<VcsKind>,
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    check_name(name)?;
    match pick_in(vcs, tier, project_root, machine_home, name)? {
        (Some(found), _) => Ok(Some(found)),
        (None, true) => Err(ResolveError::NoGitVersion(name.to_owned())),
        (None, false) => Ok(None),
    }
}

/// [`resolve`] for a project of `vcs`: on a git project, for each tier in order the `<name>.git`,
/// else the `<name>` unless it is marked `vcs: jj`, else the next tier; with no tier left, the
/// name is [`ResolveError::NoGitVersion`] when some tier had a marked file, else none. The file it
/// shadows is the one the other tier would give.
pub fn resolve_for(
    vcs: Option<VcsKind>,
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    check_name(name)?;
    if !takes_git(vcs) {
        return file_in_tiers(project_root, machine_home, name);
    }
    let (project, skipped_project) = pick_in(vcs, Tier::Project, project_root, machine_home, name)?;
    let (machine, skipped_machine) = pick_in(vcs, Tier::Machine, project_root, machine_home, name)?;
    match (project, machine) {
        (Some(project), machine) => Ok(Some(Resolved {
            shadows: machine.map(|_| Tier::Machine),
            ..project
        })),
        (None, Some(machine)) => Ok(Some(machine)),
        (None, None) if skipped_project || skipped_machine => {
            Err(ResolveError::NoGitVersion(name.to_owned()))
        }
        (None, None) => Ok(None),
    }
}

/// `hook(root, home, event) -> Option<ResolvedAction>`, where the ticket's `ResolvedAction` is
/// [`Resolved`]: the hook of `event`, the project's `hooks/<event>`, else the machine's, under the precedence and
/// the shadowing of an action. None when neither tier has one, when `event` is not one of
/// [`HOOK_EVENTS`], and when a folder cannot be read: a hook that cannot be found is not run. The
/// tick and the launcher find a hook through this alone.
pub fn hook(project_root: &Path, machine_home: &Path, event: &str) -> Option<Resolved> {
    if !HOOK_EVENTS.contains(&event) {
        return None;
    }
    hook_file(project_root, machine_home, event).ok().flatten()
}

/// The hook file `file` as the two tiers have it, whatever its name: [`hook`] for an event, and the
/// doctor for a name that is none. The name it carries is `hooks/<file>`.
pub fn hook_file(
    project_root: &Path,
    machine_home: &Path,
    file: &str,
) -> Result<Option<Resolved>, ResolveError> {
    check_name(file)?;
    file_in_tiers(project_root, machine_home, &format!("{HOOKS_DIR}/{file}"))
}

/// The hook file `file` in one tier alone, the way [`resolve_in`] reads an action's.
pub fn hook_file_in(
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    file: &str,
) -> Result<Option<Resolved>, ResolveError> {
    check_name(file)?;
    file_in(
        tier,
        project_root,
        machine_home,
        &format!("{HOOKS_DIR}/{file}"),
    )
}

/// Whether a file in a higher tier may override a file of `mode`: the `w` of the owner digit, the
/// only digit that can say, since the Engineer writes those files.
pub fn can_shadow(mode: ActionMode) -> bool {
    mode.owner().write()
}

/// Whether two paths are one file, which then shadows nothing: a project at the parent of the zaf
/// home has one folder for both tiers, and a link to the machine's file runs that very file.
pub fn same_file(one: &Path, other: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(one), std::fs::canonicalize(other)),
        (Ok(one), Ok(other)) if one == other
    )
}

/// Every name once, sorted, with the tier it resolves to and the tier it
/// shadows. Entries whose names `check_name` refuses, and anything that is not
/// a file, are skipped: they cannot be named.
pub fn list(project_root: &Path, machine_home: &Path) -> Result<Vec<Resolved>, ResolveError> {
    list_for(None, project_root, machine_home)
}

/// [`list`] for a project of `vcs`. A `<name>.git` file is never an action of its own: on a git
/// project it stands for `<name>`, which is listed with the tier and file the lookup gives, and a
/// name with no version that runs on git is left out; on any other project it is skipped.
pub fn list_for(
    vcs: Option<VcsKind>,
    project_root: &Path,
    machine_home: &Path,
) -> Result<Vec<Resolved>, ResolveError> {
    let git = takes_git(vcs);
    let mut names = std::collections::BTreeSet::new();
    for entry in entry_names(project_root, machine_home, None)? {
        match entry.strip_suffix(GIT_SUFFIX) {
            Some(stem) if git && !stem.is_empty() => {
                names.insert(stem.to_owned());
            }
            Some(_) => {}
            None => {
                names.insert(entry);
            }
        }
    }
    let mut resolved = Vec::new();
    for name in names {
        match resolve_for(vcs, project_root, machine_home, &name) {
            Ok(Some(found)) => resolved.push(found),
            Ok(None) | Err(ResolveError::NoGitVersion(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(resolved)
}

/// Every file in the tiers' `hooks` folders once, sorted, named `hooks/<file>`, whatever the file's
/// name: a name that is no event is for the doctor to say. An entry whose name `check_name`
/// refuses, and anything that is not a file, is skipped.
pub fn list_hooks(project_root: &Path, machine_home: &Path) -> Result<Vec<Resolved>, ResolveError> {
    let mut resolved = Vec::new();
    for file in entry_names(project_root, machine_home, Some(HOOKS_DIR))? {
        if let Some(found) = hook_file(project_root, machine_home, &file)? {
            resolved.push(found);
        }
    }
    Ok(resolved)
}

/// The names of the entries of both tiers' actions folders, or of the folder `sub` in them, once and
/// sorted. A name `check_name` refuses is left out.
fn entry_names(
    project_root: &Path,
    machine_home: &Path,
    sub: Option<&str>,
) -> Result<std::collections::BTreeSet<String>, ResolveError> {
    let mut names = std::collections::BTreeSet::new();
    for tier in [Tier::Project, Tier::Machine] {
        let mut dir = tier_dir(tier, project_root, machine_home);
        dir.extend(sub);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source)
                if matches!(
                    source.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(source) => return Err(ResolveError::Read { path: dir, source }),
        };
        for entry in entries {
            let entry = entry.map_err(|source| ResolveError::Read {
                path: dir.clone(),
                source,
            })?;
            if let Ok(name) = entry.file_name().into_string()
                && check_name(&name).is_ok()
            {
                names.insert(name);
            }
        }
    }
    Ok(names)
}
