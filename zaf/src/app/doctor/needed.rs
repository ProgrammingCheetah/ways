//! The needed names of `zaf doctor` (docs/zaf-actions.md §2 "No built-ins", §4 doctor row, ruling
//! 28): every action name the project needs, with the tier that provides it.
//!
//! zaf ships no actions, so a name nothing provides fails the report, naming what needs it. A
//! project needs:
//!
//! - the four `slice-*` names, always;
//! - each `action:<name>` step of a `[check.<target>]` table;
//! - each `zaf do <name>` in a `[gate]` line;
//! - each literal `zaf do <name>` in the body of an action. A `zaf do --tier machine <name>` call
//!   needs the machine's file, whatever the project has.
//!
//! The scan is textual and reads every line of a body, a comment line too (the design: "doctor
//! finds needed names by scanning bodies for a literal `zaf do`"). A name made at run time
//! (`zaf do "$name"`) is not literal and is not found. Invalid files stay
//! with [`super::ActionCheck`], shadowing with its own task. The project's steps that resolve to a machine action are the
//! machine dependents of [`super::ActionCheck`].

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::app::actions::{ResolveError, Tier, list, parse, resolve_in};
use crate::app::check::step::FLAGGED_SUFFIX;
use crate::composition::STATE_DIR;
use crate::manifest::{CommandLine, Manifest};

/// The slice family: action language, written by the project (§1 principle 9).
const SLICE_NAMES: [&str; 4] = ["slice-open", "slice-pr", "slice-merge", "slice-drop"];

/// What needs the four `slice-*` names.
const SLICE_NEEDER: &str = "the slice verbs";

/// The prefix of a check step that runs an action.
const ACTION_STEP: &str = "action:";

/// Characters that end a shell command, and so end the words of a `zaf do` call.
const COMMAND_ENDS: [char; 7] = [';', '&', '|', '(', ')', '<', '>'];

/// One needed name, with who needs it and the tier that provides it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NeededCheck {
    /// The action name.
    pub name: String,
    /// The tier `zaf do <name>` resolves to, `project` or `machine`; none when no tier has it.
    pub tier: Option<String>,
    /// Who needs the name, one entry each, in the order they were found.
    pub needed_by: Vec<String>,
    /// Whether the name is provided as its needers call it.
    pub ok: bool,
    /// What is wrong, one entry per problem; empty when `ok`.
    pub problems: Vec<String>,
}

impl NeededCheck {
    /// `needed <name> (<tier>): ok, needed by <who>`, or the failure and its who, with a line per
    /// problem under it.
    pub(super) fn lines(&self) -> Vec<String> {
        let who = self.needed_by.join(", ");
        let head = match (&self.tier, self.ok) {
            (Some(tier), true) => format!("needed {} ({tier}): ok, needed by {who}", self.name),
            (Some(tier), false) => {
                format!("needed {} ({tier}): missing, needed by {who}", self.name)
            }
            (None, _) => format!("needed {}: no provider, needed by {who}", self.name),
        };
        let mut lines = vec![head];
        lines.extend(self.problems.iter().map(|problem| format!("  {problem}")));
        lines
    }
}

/// How the callers of one name call it.
#[derive(Debug, Default)]
struct Demand {
    needed_by: Vec<String>,
    /// Some caller named `--tier machine`.
    machine_call: bool,
    /// The callers that are the project's own steps (the manifest, the slice verbs, the body of a
    /// project action), each with whether it named `--tier machine`.
    project_calls: Vec<(String, bool)>,
}

/// The needed names in name order.
#[derive(Debug, Default)]
struct Wanted(BTreeMap<String, Demand>);

impl Wanted {
    fn add(&mut self, name: &str, by: String, machine_call: bool, project: bool) {
        let demand = self.0.entry(name.to_owned()).or_default();
        if project && !demand.project_calls.contains(&(by.clone(), machine_call)) {
            demand.project_calls.push((by.clone(), machine_call));
        }
        if !demand.needed_by.contains(&by) {
            demand.needed_by.push(by);
        }
        demand.machine_call |= machine_call;
    }
}

/// Every needed name of the manifest and of the action bodies in both tiers, each with the tier
/// that provides it.
///
/// Without a zaf home there is no machine tier. A body that cannot be read or is not a valid
/// action is skipped here; its file is reported as invalid.
pub(super) fn check_needed(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
) -> Vec<NeededCheck> {
    wanted_names(root, zaf_home, manifest)
        .0
        .into_iter()
        .map(|(name, demand)| check_name(root, zaf_home, name, demand))
        .collect()
}

/// Every needed name, with its callers.
fn wanted_names(root: &Path, zaf_home: Option<&Path>, manifest: &Manifest) -> Wanted {
    let mut wanted = Wanted::default();
    for name in SLICE_NAMES {
        wanted.add(name, SLICE_NEEDER.to_owned(), false, true);
    }
    manifest_names(manifest, &mut wanted);
    body_names(root, zaf_home, &mut wanted);
    wanted
}

/// The project's own steps that resolve to a machine action, by action name: each entry is one
/// step, as [`NeededCheck::needed_by`] words it.
///
/// A step resolves to the machine's file when the project has none of that name, and always for a
/// `zaf do --tier machine` call; either way the machine must have the file. The steps of the
/// machine's own actions are not the project's and are left out. Without a zaf home there is no
/// machine tier and no dependency.
pub(super) fn machine_dependents(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
) -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    let Some(home) = zaf_home else {
        return found;
    };
    for (name, demand) in wanted_names(root, zaf_home, manifest).0 {
        let has = |tier: Tier| matches!(resolve_in(tier, root, home, &name), Ok(Some(_)));
        if !has(Tier::Machine) {
            continue;
        }
        let project_has = has(Tier::Project);
        let steps: Vec<String> = demand
            .project_calls
            .into_iter()
            .filter(|(_, machine_call)| *machine_call || !project_has)
            .map(|(by, _)| by)
            .collect();
        if !steps.is_empty() {
            found.insert(name, steps);
        }
    }
    found
}

/// The names the manifest's check steps and gate lines call.
fn manifest_names(manifest: &Manifest, wanted: &mut Wanted) {
    for (target, section) in &manifest.check.targets {
        for step in &section.steps {
            let step = step.trim();
            let step = step.strip_suffix(FLAGGED_SUFFIX).unwrap_or(step);
            if let Some(name) = step.strip_prefix(ACTION_STEP) {
                wanted.add(
                    name.trim(),
                    format!("[check.{target}] step `{ACTION_STEP}{}`", name.trim()),
                    false,
                    true,
                );
            }
        }
    }
    let Some(gate) = &manifest.gate else {
        return;
    };
    let lines: [(&str, Option<&CommandLine>); 3] = [
        ("full", Some(&gate.full)),
        ("light", gate.light.as_ref()),
        ("tests", gate.tests.as_ref()),
    ];
    for (key, line) in lines {
        let Some(line) = line else { continue };
        for call in calls_in(line.as_ref()) {
            wanted.add(
                &call.name,
                format!("[gate] {key} `{}`", call.shown()),
                call.machine,
                true,
            );
        }
    }
}

/// The names the bodies of the resolved actions call.
fn body_names(root: &Path, zaf_home: Option<&Path>, wanted: &mut Wanted) {
    // With no home there is no machine tier; `list` always takes one, so it is given the project's
    // own state folder, whose `actions` the project tier already covers.
    let state_dir = root.join(STATE_DIR);
    let Ok(found) = list(root, zaf_home.unwrap_or(&state_dir)) else {
        return;
    };
    for action in found {
        let Ok(text) = fs::read_to_string(&action.path) else {
            continue;
        };
        let Ok(parsed) = parse(&action.path, &text) else {
            continue;
        };
        let by = format!("action {} ({})", action.name, action.tier);
        for line in text.lines().skip(parsed.body_line.saturating_sub(1)) {
            for call in calls_in(line) {
                wanted.add(
                    &call.name,
                    format!("{by}: `{}`", call.shown()),
                    call.machine,
                    action.tier == Tier::Project,
                );
            }
        }
    }
}

/// Looks one name up in the tiers.
fn check_name(root: &Path, zaf_home: Option<&Path>, name: String, demand: Demand) -> NeededCheck {
    let state_dir = root.join(STATE_DIR);
    let in_tier = |tier: Tier| -> Result<bool, ResolveError> {
        let home = match (tier, zaf_home) {
            (Tier::Machine, None) => return Ok(false),
            (_, Some(home)) => home,
            (Tier::Project, None) => state_dir.as_path(),
        };
        Ok(resolve_in(tier, root, home, &name)?.is_some())
    };
    let mut problems = Vec::new();
    let mut found = |tier: Tier| match in_tier(tier) {
        Ok(found) => found,
        Err(error) => {
            let problem = error.to_string();
            if !problems.contains(&problem) {
                problems.push(problem);
            }
            false
        }
    };
    let project = found(Tier::Project);
    let machine = found(Tier::Machine);
    let tier = match (project, machine) {
        (true, _) => Some(Tier::Project),
        (false, true) => Some(Tier::Machine),
        (false, false) => None,
    };
    if tier.is_some() && demand.machine_call && !machine {
        problems.push(format!(
            "`zaf do --tier machine {name}` needs the machine's file, and the machine has none"
        ));
    }
    let ok = tier.is_some() && problems.is_empty();
    NeededCheck {
        name,
        tier: tier.map(|tier| tier.to_string()),
        needed_by: demand.needed_by,
        ok,
        problems,
    }
}

/// One literal `zaf do` call found in a line of text.
#[derive(Debug, PartialEq, Eq)]
struct Call {
    name: String,
    machine: bool,
}

impl Call {
    fn shown(&self) -> String {
        if self.machine {
            format!("zaf do --tier machine {}", self.name)
        } else {
            format!("zaf do {}", self.name)
        }
    }
}

/// Every `zaf do <name>` in a line, in order. The name is the first word after `do` and its
/// flags that is not a flag (`--tier` takes a value); a name that is not literal (a variable, a
/// command substitution, a placeholder) is skipped, as is a name that is no action name.
fn calls_in(line: &str) -> Vec<Call> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut calls = Vec::new();
    let mut at = 0;
    while at < words.len() {
        if !is_zaf(words[at]) || words.get(at + 1) != Some(&"do") {
            at += 1;
            continue;
        }
        let mut machine = false;
        let mut next = at + 2;
        let mut name = None;
        while let Some(&word) = words.get(next) {
            next += 1;
            if let Some(value) = word.strip_prefix("--tier=") {
                machine |= value == "machine";
            } else if word == "--tier" {
                machine |= words.get(next).is_some_and(|value| *value == "machine");
                next += 1;
            } else if word.starts_with('-') {
                // `--show`, or a flag zaf do does not have: neither is the name.
            } else {
                name = Some(word);
                break;
            }
        }
        if let Some(name) = name.and_then(literal) {
            calls.push(Call { name, machine });
        }
        at = next.max(at + 1);
    }
    calls
}

/// Whether a word is the `zaf` binary: bare, or by a path, possibly quoted or opening a group.
fn is_zaf(word: &str) -> bool {
    let word = word.trim_start_matches(['"', '\'', '(', '{']);
    word == "zaf" || word.ends_with("/zaf")
}

/// The action name a shell word stands for, when it is literal.
fn literal(word: &str) -> Option<String> {
    let word = word.split(COMMAND_ENDS).next().unwrap_or_default();
    let word = word.trim_matches(['"', '\'']);
    let dynamic = word.contains(['$', '`', '{', '}', '*', '?', '\\', '"', '\'']);
    (!word.is_empty() && !dynamic && crate::app::actions::check_name(word).is_ok())
        .then(|| word.to_owned())
}

#[cfg(test)]
mod tests;
