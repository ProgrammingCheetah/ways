//! The phase lists part of `zaf doctor` (docs/zaf-actions.md §5.1): every action a `[phases]`
//! list names, with the tier that provides it.
//!
//! The load has already refused an empty list and a name listed twice. Here a listed action is
//! invalid when no tier provides it, when its file is not a valid action, when it declares no
//! `kind` or when it declares no `ends`, since zaf runs a phase by what its action declares and
//! guesses nothing. An invalid file fails the report.

use std::fs;
use std::path::Path;

use crate::app::actions::{parse, resolve};
use crate::composition::STATE_DIR;
use crate::manifest::Manifest;

/// One action of one task type's phase list, checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PhaseCheck {
    /// The task type whose list names the action.
    pub task_type: String,
    /// The action's name.
    pub name: String,
    /// The tier `zaf do <name>` resolves to, `project` or `machine`; none when no tier has it.
    pub tier: Option<String>,
    /// Whether the action can stand for a phase.
    pub ok: bool,
    /// What is wrong, one entry per problem; empty when `ok`.
    pub problems: Vec<String>,
}

impl PhaseCheck {
    /// `phase <type> <name> (<tier>): ok`, or the failure with a line per problem under it.
    pub(super) fn lines(&self) -> Vec<String> {
        let head = match (&self.tier, self.ok) {
            (Some(tier), true) => format!("phase {} {} ({tier}): ok", self.task_type, self.name),
            (Some(tier), false) => {
                format!("phase {} {} ({tier}): invalid", self.task_type, self.name)
            }
            (None, _) => format!("phase {} {}: no provider", self.task_type, self.name),
        };
        let mut lines = vec![head];
        lines.extend(self.problems.iter().map(|problem| format!("  {problem}")));
        lines
    }
}

/// Every action of every `[phases]` list, in task type order and then list order.
///
/// Without a zaf home there is no machine tier.
pub(super) fn check_phases(
    root: &Path,
    zaf_home: Option<&Path>,
    manifest: &Manifest,
) -> Vec<PhaseCheck> {
    // With no home there is no machine tier; `resolve` always takes one, so it is given the
    // project's own state folder, whose `actions` the project tier already covers.
    let state_dir = root.join(STATE_DIR);
    let machine_home = zaf_home.unwrap_or(&state_dir);
    manifest
        .phases
        .iter()
        .flat_map(|(task_type, list)| {
            list.iter()
                .map(move |name| check_phase(root, machine_home, task_type, name.as_str()))
        })
        .collect()
}

fn check_phase(root: &Path, machine_home: &Path, task_type: &str, name: &str) -> PhaseCheck {
    let head = format!("[phases] {task_type}: action `{name}`");
    let mut problems = Vec::new();
    let mut tier = None;
    match resolve(root, machine_home, name) {
        Ok(None) => problems.push(format!("{head} is provided by no tier")),
        Err(error) => problems.push(format!("{head} is provided by no tier: {error}")),
        Ok(Some(found)) => {
            tier = Some(found.tier.to_string());
            match fs::read_to_string(&found.path)
                .map_err(|error| error.to_string())
                .and_then(|text| parse(&found.path, &text).map_err(|error| error.to_string()))
            {
                Err(reason) => problems.push(format!("{head} is invalid: {reason}")),
                Ok(parsed) => {
                    if parsed.metadata.kind.is_none() {
                        problems.push(format!("{head} has no `kind`"));
                    }
                    if parsed.metadata.ends.is_none() {
                        problems.push(format!("{head} has no `ends`"));
                    }
                }
            }
        }
    }
    PhaseCheck {
        task_type: task_type.to_owned(),
        name: name.to_owned(),
        tier,
        ok: problems.is_empty(),
        problems,
    }
}
