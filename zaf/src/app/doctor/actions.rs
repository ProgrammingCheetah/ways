//! The actions part of `zaf doctor` (docs/zaf-actions.md §3, §4, §5): every action either tier
//! provides, with its validity and its mode.
//!
//! A file is invalid when its metadata does not hold (a bad `mode`, `target` or `phases`, a
//! declared code `125`, a missing description or code `1`), when it is not executable, or when its
//! `group` names a role the manifest does not have. An invalid file fails the report and the
//! problem names the file, and the line when there is one. Needed names, shadowing and machine
//! dependencies are not here.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::app::actions::{ActionMetadata, ResolveError, Resolved, list, parse};
use crate::composition::STATE_DIR;
use crate::manifest::Manifest;

/// Any of the three execute bits.
const EXECUTE_BITS: u32 = 0o111;

/// One action, checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionCheck {
    /// The action's name.
    pub name: String,
    /// The tier its file is in, `project` or `machine`.
    pub tier: String,
    /// The file.
    pub path: PathBuf,
    /// The mode it declares, three octal digits; none when its metadata does not hold, so no mode
    /// can be read.
    pub mode: Option<String>,
    /// Whether the file is a valid action.
    pub ok: bool,
    /// What is wrong with it, one entry per problem, each naming the file.
    pub problems: Vec<String>,
}

impl ActionCheck {
    /// `action <name> (<tier>): ok, mode 755`, or `invalid` with a line per problem under it.
    pub(super) fn lines(&self) -> Vec<String> {
        let state = if self.ok { "ok" } else { "invalid" };
        let mode = self
            .mode
            .as_deref()
            .map_or_else(|| "mode unknown".to_owned(), |mode| format!("mode {mode}"));
        let head = format!("action {} ({}): {state}, {mode}", self.name, self.tier);
        let mut lines = vec![head];
        lines.extend(self.problems.iter().map(|problem| format!("  {problem}")));
        lines
    }
}

/// Every action of the project's tier and the machine's, checked against the manifest's roles.
///
/// Without a zaf home only the project's tier is read. An actions folder that cannot be read is
/// the second part of the answer, its error.
pub(super) fn check_actions(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
) -> (Vec<ActionCheck>, Option<String>) {
    // With no home there is no machine tier. `list` always takes one, so it is given the project's
    // own state folder, whose `actions` the project tier already covers.
    let state_dir = root.join(STATE_DIR);
    let machine_home = zaf_home.unwrap_or(&state_dir);
    match list(root, machine_home) {
        Ok(found) => (
            found
                .iter()
                .map(|found| check_action(found, manifest))
                .collect(),
            None,
        ),
        Err(error) => (Vec::new(), Some(unreadable(&error))),
    }
}

fn unreadable(error: &ResolveError) -> String {
    format!("actions: {error}")
}

fn check_action(found: &Resolved, manifest: &Manifest) -> ActionCheck {
    let file = found.path.as_path();
    let mut problems = Vec::new();
    let mut mode = None;
    match fs::read_to_string(file) {
        Err(error) => problems.push(format!("{}: cannot read: {error}", file.display())),
        Ok(text) => match parse(file, &text) {
            Err(error) => problems.push(error.to_string()),
            Ok(parsed) => {
                mode = Some(parsed.metadata.mode.to_string());
                problems.extend(unknown_roles(file, &parsed.metadata, manifest));
            }
        },
    }
    if let Some(problem) = not_executable(file) {
        problems.push(problem);
    }
    ActionCheck {
        name: found.name.clone(),
        tier: found.tier.to_string(),
        path: found.path.clone(),
        mode,
        ok: problems.is_empty(),
        problems,
    }
}

/// One problem when `group` names roles the manifest does not have.
fn unknown_roles(file: &Path, metadata: &ActionMetadata, manifest: &Manifest) -> Option<String> {
    let roles: Vec<&str> = manifest
        .roles
        .keys()
        .map(|role| -> &str { role.as_ref() })
        .collect();
    let unknown: Vec<&str> = metadata
        .group
        .iter()
        .map(String::as_str)
        .filter(|named| !roles.contains(named))
        .collect();
    if unknown.is_empty() {
        return None;
    }
    Some(format!(
        "{}: group names no role: {} (the roles are {})",
        file.display(),
        unknown.join(", "),
        roles.join(", ")
    ))
}

/// A problem when the file has no execute bit. A file as a whole has no line of its own, so the
/// problem is put on line 1.
fn not_executable(file: &Path) -> Option<String> {
    let mode = fs::metadata(file).ok()?.permissions().mode();
    (mode & EXECUTE_BITS == 0).then(|| format!("{}:1: not executable", file.display()))
}
