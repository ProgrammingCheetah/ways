//! The actions part of `zaf doctor` (docs/zaf-actions.md §3, §4, §5): every action either tier
//! provides, with its validity and its mode.
//!
//! A file is invalid when its metadata does not hold (a bad `mode`, `target` or `phases`, a
//! declared code `125`, a missing description or code `1`), when it is not executable, or when its
//! `group` names a role the manifest does not have. An invalid file fails the report and the
//! problem names the file, and the line when there is one.
//!
//! A project file that shadows a machine file is listed with the file it shadows. That does not
//! fail the report; a shadow whose declared run bits are wider than the shadowed file's gets a note
//! that the extra bits have no effect. A shadow of a file whose mode has no owner `w`, or whose
//! mode cannot be read, fails it, since a run would refuse it. Needed names and machine
//! dependencies are not here.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::app::actions::{
    ActionMetadata, ActionMode, ResolveError, Resolved, Tier, can_shadow, list, parse, resolve_in,
    same_file,
};
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
    /// The machine file this project file shadows; none when it shadows nothing.
    pub shadows: Option<PathBuf>,
    /// What is worth knowing and does not fail the report, such as a shadow's run bits that have no
    /// effect.
    pub notes: Vec<String>,
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
        lines.extend(
            self.shadows
                .iter()
                .map(|over| format!("  shadows the machine file {}", over.display())),
        );
        lines.extend(self.notes.iter().map(|note| format!("  {note}")));
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
                .map(|found| check_action(found, manifest, root, machine_home))
                .collect(),
            None,
        ),
        Err(error) => (Vec::new(), Some(unreadable(&error))),
    }
}

fn unreadable(error: &ResolveError) -> String {
    format!("actions: {error}")
}

fn check_action(
    found: &Resolved,
    manifest: &Manifest,
    root: &Path,
    machine_home: &Path,
) -> ActionCheck {
    let file = found.path.as_path();
    let mut problems = Vec::new();
    let mut mode = None;
    let mut own = None;
    match fs::read_to_string(file) {
        Err(error) => problems.push(format!("{}: cannot read: {error}", file.display())),
        Ok(text) => match parse(file, &text) {
            Err(error) => problems.push(error.to_string()),
            Ok(parsed) => {
                mode = Some(parsed.metadata.mode.to_string());
                problems.extend(unknown_roles(file, &parsed.metadata, manifest));
                own = Some(parsed.metadata);
            }
        },
    }
    if let Some(problem) = not_executable(file) {
        problems.push(problem);
    }
    let shadows = shadowed_file(found, root, machine_home);
    let mut notes = Vec::new();
    if let Some(over) = &shadows {
        match shadow_verdict(file, own.as_ref(), over) {
            Ok(note) => notes.extend(note),
            Err(problem) => problems.push(problem),
        }
    }
    ActionCheck {
        name: found.name.clone(),
        tier: found.tier.to_string(),
        path: found.path.clone(),
        mode,
        ok: problems.is_empty(),
        problems,
        shadows,
        notes,
    }
}

/// The machine file a project file shadows; none for a file that shadows nothing, which includes a
/// project and a machine whose two tiers are one folder.
fn shadowed_file(found: &Resolved, root: &Path, machine_home: &Path) -> Option<PathBuf> {
    found.shadows?;
    let over = resolve_in(Tier::Machine, root, machine_home, &found.name)
        .ok()??
        .path;
    (!same_file(&found.path, &over)).then_some(over)
}

/// What a shadow of `over` comes to: a note when its run bits are wider than those of `over`, a
/// problem when `over` may not be shadowed. `own` is none when the shadow's own metadata does not
/// hold, which its own problems already say.
fn shadow_verdict(
    file: &Path,
    own: Option<&ActionMetadata>,
    over: &Path,
) -> Result<Option<String>, String> {
    let (file, over_name) = (file.display(), over.display());
    let theirs = fs::read_to_string(over)
        .map_err(|error| format!("cannot read {over_name}: {error}"))
        .and_then(|text| parse(over, &text).map_err(|error| error.to_string()))
        .map_err(|reason| format!("{file} may not shadow {over_name}: it has no mode: {reason}"))?
        .metadata;
    if !can_shadow(theirs.mode) {
        let mode = theirs.mode;
        return Err(format!(
            "{file} may not shadow {over_name}: its mode {mode} has no owner w"
        ));
    }
    Ok(own.filter(|own| widens(own, &theirs)).map(|own| {
        format!(
            "{file}: mode {} has run bits wider than mode {} of {over_name}: the extra bits have no effect",
            own.mode, theirs.mode
        )
    }))
}

/// Whether `own` lets any caller run what `theirs` does not: the Engineer, a role either names in
/// its group, or any other role. A role is read by the group digit of the file that names it and by
/// the others digit of the file that does not, as a run does.
fn widens(own: &ActionMetadata, theirs: &ActionMetadata) -> bool {
    if own.mode.owner().run() && !theirs.mode.owner().run() {
        return true;
    }
    let named = own.group.iter().chain(&theirs.group).map(Some);
    std::iter::once(None).chain(named).any(|role| {
        run_bit(own.mode, &own.group, role) && !run_bit(theirs.mode, &theirs.group, role)
    })
}

/// Whether `mode` lets a role run: the group digit when `group` names it, the others digit when it
/// does not. No role is one that neither file names.
fn run_bit(mode: ActionMode, group: &[String], role: Option<&String>) -> bool {
    match role {
        Some(role) if group.contains(role) => mode.group().run(),
        _ => mode.others().run(),
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
