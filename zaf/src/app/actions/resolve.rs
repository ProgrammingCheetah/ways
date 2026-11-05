//! Resolving an action name through the two tiers (docs/zaf-actions.md §2, §3).
//!
//! The project's `<root>/.zaf/actions/<name>` beats the machine's
//! `<home>/actions/<name>`. There is no built-in table: a name in no tier
//! resolves to nothing, and no name is special here. The project tier is read
//! from the project root alone, so an action file inside a slice workspace is
//! never found: an edit to an action runs only once it has merged and reached
//! the root.

use std::path::{Path, PathBuf};

use crate::composition::STATE_DIR;

/// The folder under a tier's home that holds its actions.
pub const ACTIONS_DIR: &str = "actions";

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

/// The file `name` is in one tier; none when that tier has no such file.
pub fn resolve_in(
    tier: Tier,
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    check_name(name)?;
    let path = tier_dir(tier, project_root, machine_home).join(name);
    Ok(is_file(&path)?.then(|| Resolved {
        name: name.to_owned(),
        tier,
        path,
        shadows: None,
    }))
}

/// The file `name` resolves to: the project's, else the machine's; none when
/// neither tier has it.
pub fn resolve(
    project_root: &Path,
    machine_home: &Path,
    name: &str,
) -> Result<Option<Resolved>, ResolveError> {
    let project = resolve_in(Tier::Project, project_root, machine_home, name)?;
    let machine = resolve_in(Tier::Machine, project_root, machine_home, name)?;
    Ok(match (project, machine) {
        (Some(project), Some(_)) => Some(Resolved {
            shadows: Some(Tier::Machine),
            ..project
        }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    })
}

/// Every name once, sorted, with the tier it resolves to and the tier it
/// shadows. Entries whose names `check_name` refuses, and anything that is not
/// a file, are skipped: they cannot be named.
pub fn list(project_root: &Path, machine_home: &Path) -> Result<Vec<Resolved>, ResolveError> {
    let mut names = std::collections::BTreeSet::new();
    for tier in [Tier::Project, Tier::Machine] {
        let dir = tier_dir(tier, project_root, machine_home);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
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
    let mut resolved = Vec::new();
    for name in names {
        if let Some(found) = resolve(project_root, machine_home, &name)? {
            resolved.push(found);
        }
    }
    Ok(resolved)
}
