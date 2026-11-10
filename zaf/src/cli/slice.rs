//! `zaf slice open|pr|merge|drop`: aliases of `zaf do slice-open|slice-pr|slice-merge|slice-drop`
//! (docs/zaf-actions.md §4), and what `zaf changes` and `zaf check` share with them: the release
//! version from the environment, the target resolved from the board, the exit code of a
//! [`SliceError`], and the line that names a missing `slice-open`.
//!
//! The alias runs the action through the one runner, so the modes, the phases, the slice records
//! and the log are the action's own. zaf ships no actions: a project with none gets
//! `missing action <name>`, exit 2.

use std::ffi::OsString;
use std::fmt::Display;
use std::path::Path;

use serde_json::{Value, json};

use super::do_action::{print_outcome, run_named};
use super::exit::{BACKEND, OK, REFUSED, code_for, fail_composition};
use super::{Cli, SliceAction};
use crate::app::actions::{self, NO_SUCH_ACTION, RunOutcome, RunResult, Tier, hand_back};
use crate::app::slice::{self, SliceError, SliceQuery, SliceTarget};
use crate::composition::{Composition, zaf_home};
use crate::domain::IssueNumber;

/// The environment variable that names the release a slice's branches build toward,
/// for branch patterns that hold `{version}`.
pub const VERSION_VAR: &str = "ZAF_VERSION";

/// The action that opens a task's slice.
const SLICE_OPEN: &str = "slice-open";

/// Runs the interface `zaf slice <x> <task>` (x is open, pr, merge or drop) as
/// `zaf do slice-<x> <task>`, with the action's exit. A missing action is
/// exit 2 rather than the 125 of `zaf do`. On exit 0 the value `slice-open` or `slice-pr` handed
/// back (the workspace, the pull request number) is printed on one line.
pub fn run(cli: &Cli, action: &SliceAction) -> u8 {
    let (name, task, hands_back) = match action {
        SliceAction::Open { task } => (SLICE_OPEN, *task, true),
        SliceAction::Pr { task } => ("slice-pr", *task, true),
        SliceAction::Merge { task } => ("slice-merge", *task, false),
        SliceAction::Drop { task } => ("slice-drop", *task, false),
    };
    let args = [OsString::from(task.get().to_string())];
    let outcome = run_named(name, &args, None);
    if is_missing(&outcome) {
        return fail_missing(cli, &cli.verb.path(), name);
    }
    let code = print_outcome(cli, &outcome);
    if code == OK && hands_back && !cli.json {
        print_hand_back(&outcome);
    }
    code
}

/// Prints the last line of the run's log, the value the action handed back.
fn print_hand_back(outcome: &RunOutcome) {
    let Some(log) = outcome.log.as_deref() else {
        return;
    };
    match hand_back(log) {
        Ok(Some(value)) => println!("{value}"),
        Ok(None) => {}
        Err(error) => eprintln!("zaf slice: cannot read {}: {error}", log.display()),
    }
}

/// Whether the run never started because no tier provides the name.
fn is_missing(outcome: &RunOutcome) -> bool {
    outcome.result == RunResult::NotRun
        && outcome.tier.is_none()
        && outcome.meaning.as_deref() == Some(NO_SUCH_ACTION)
}

/// What every verb that needs an action says when no tier provides it.
fn missing_action(name: &str) -> String {
    format!(
        "missing action {name}: no {name} action in .zaf/actions or the machine's actions \
         (zaf ships none; see docs/zaf-actions.md §4)"
    )
}

/// Prints a missing action, as JSON too on `--json`, and returns its code.
fn fail_missing(cli: &Cli, path: &str, name: &str) -> u8 {
    let code = report_missing(path, name);
    if cli.json {
        println!("{}", json!({ "error": missing_action(name) }));
    }
    code
}

/// Prints a missing action and returns its code.
fn report_missing(path: &str, name: &str) -> u8 {
    eprintln!("zaf {path}: {}", missing_action(name));
    BACKEND
}

/// Why a task with no slice record is refused.
fn not_open(task: IssueNumber) -> String {
    format!(
        "slice for {task} is not open; run zaf do slice-open {}",
        task.get()
    )
}

/// Refuses a task with no slice record: names the missing `slice-open` action when no tier
/// provides it (exit 2), else says the slice is not open (exit 1). Prints and returns the code.
pub(super) fn refuse_not_open(path: &str, composition: &Composition, task: IssueNumber) -> u8 {
    refuse(path, composition.root(), zaf_home().as_deref(), task)
}

/// [`refuse_not_open`] for a project root and a zaf home.
fn refuse(path: &str, root: &Path, home: Option<&Path>, task: IssueNumber) -> u8 {
    if provides(root, home, SLICE_OPEN) {
        eprintln!("zaf {path}: {}", not_open(task));
        REFUSED
    } else {
        report_missing(path, SLICE_OPEN)
    }
}

/// Whether a tier has a file of that name. A tier that cannot be read counts as providing it:
/// the run itself says what is wrong with it.
fn provides(root: &Path, home: Option<&Path>, name: &str) -> bool {
    let found = match home {
        Some(home) => {
            let home = std::path::absolute(home).unwrap_or_else(|_| home.to_owned());
            actions::resolve(root, &home, name)
        }
        None => actions::resolve_in(Tier::Project, root, root, name),
    };
    found.map_or(true, |found| found.is_some())
}

/// Finds the project and resolves the task's slice; on failure, prints it and returns the code.
pub(super) fn prepare(
    path: &str,
    cli: &Cli,
    task: IssueNumber,
) -> Result<(Composition, SliceTarget), u8> {
    let composition =
        Composition::discover_here().map_err(|error| fail_composition(path, &error))?;
    let board = composition
        .board()
        .map_err(|error| fail_composition(path, &error))?;
    let query = SliceQuery {
        task,
        version: std::env::var(VERSION_VAR)
            .ok()
            .filter(|version| !version.is_empty()),
    };
    let branches = &composition.manifest().branches;
    let target = slice::resolve(board.as_ref(), branches, &query)
        .map_err(|error| fail_slice(path, cli, &error))?;
    Ok((composition, target))
}

/// The exit code a slice error maps to.
fn code_for_slice(error: &SliceError) -> u8 {
    match error {
        SliceError::NoEpic { .. } => REFUSED,
        SliceError::NoVersion => BACKEND,
        SliceError::Port { error, .. } => code_for(error),
    }
}

/// Prints a slice error, naming its step, and returns its code.
pub(super) fn fail_slice(path: &str, cli: &Cli, error: &SliceError) -> u8 {
    let hint = match error {
        SliceError::NoVersion => format!("; set {VERSION_VAR}"),
        _ => String::new(),
    };
    eprintln!("zaf {path}: {error}{hint}");
    if cli.json {
        let step: Option<&'static str> = error.step().map(Into::into);
        println!("{}", json!({ "error": error.to_string(), "step": step }));
    }
    code_for_slice(error)
}

/// Prints a verb's result: the JSON for the spawner on `--json`, the terse line otherwise.
pub(super) fn print(cli: &Cli, json: &Value, terse: impl Display) -> u8 {
    if cli.json {
        println!("{json}");
    } else {
        println!("{terse}");
    }
    OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_action_line_names_the_action_and_the_page() {
        assert_eq!(
            missing_action("slice-drop"),
            "missing action slice-drop: no slice-drop action in .zaf/actions or the machine's \
             actions (zaf ships none; see docs/zaf-actions.md §4)"
        );
    }

    #[test]
    fn a_task_with_no_record_is_refused_by_what_the_tiers_hold() {
        let dir = tempfile::tempdir().expect("tempdir");
        let task = IssueNumber::from(46);
        assert_eq!(
            not_open(task),
            "slice for #46 is not open; run zaf do slice-open 46"
        );
        assert_eq!(refuse("check", dir.path(), None, task), BACKEND);
        let actions = dir.path().join(".zaf/actions");
        std::fs::create_dir_all(&actions).expect("mkdir");
        std::fs::write(actions.join(SLICE_OPEN), "#!/bin/sh\n").expect("write");
        assert_eq!(refuse("check", dir.path(), None, task), REFUSED);
    }

    #[test]
    fn the_machines_slice_open_counts_as_provided() {
        let project = tempfile::tempdir().expect("tempdir");
        let home = tempfile::tempdir().expect("tempdir");
        let task = IssueNumber::from(46);
        assert_eq!(
            refuse("check", project.path(), Some(home.path()), task),
            BACKEND
        );
        let actions = home.path().join("actions");
        std::fs::create_dir_all(&actions).expect("mkdir");
        std::fs::write(actions.join(SLICE_OPEN), "#!/bin/sh\n").expect("write");
        assert_eq!(
            refuse("check", project.path(), Some(home.path()), task),
            REFUSED
        );
    }

    #[test]
    fn only_a_nameless_no_such_action_is_missing() {
        let missing = RunOutcome::not_run(SLICE_OPEN, None, NO_SUCH_ACTION.to_owned());
        assert!(is_missing(&missing));
        let refused = RunOutcome::not_run(SLICE_OPEN, None, "a phase refused it".to_owned());
        assert!(!is_missing(&refused));
        let shadowed =
            RunOutcome::not_run(SLICE_OPEN, Some(Tier::Project), NO_SUCH_ACTION.to_owned());
        assert!(!is_missing(&shadowed));
    }
}
