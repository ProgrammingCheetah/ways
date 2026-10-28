//! `zaf slice open|pr|merge|drop`, and what `zaf changes` shares with them:
//! the release version from the environment, the target resolved from the board,
//! the exit code of a [`SliceError`], and the pull request `slice pr` records.

use std::fmt::Display;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::exit::{BACKEND, OK, REFUSED, code_for, fail_composition};
use super::{Cli, SliceAction};
use crate::app::slice::{
    self, MergeCommand, MergeOutcome, MergePorts, SliceError, SliceQuery, SliceTarget,
};
use crate::composition::{Composition, CompositionError};
use crate::domain::IssueNumber;
use crate::ports::{Gate, PrNumber, Publication, SliceOpening};

/// The environment variable that names the release a slice's branches build toward,
/// for branch patterns that hold `{version}`.
pub const VERSION_VAR: &str = "ZAF_VERSION";

/// Where `slice pr` records each task's pull request, under the state folder.
const SLICES_DIR: &str = "slices";

/// Runs a slice action.
pub fn run(cli: &Cli, action: &SliceAction) -> u8 {
    let path = cli.verb.path();
    let task = match action {
        SliceAction::Open { task }
        | SliceAction::Pr { task }
        | SliceAction::Merge { task }
        | SliceAction::Drop { task } => *task,
    };
    let (composition, target) = match prepare(&path, cli, task) {
        Ok(prepared) => prepared,
        Err(code) => return code,
    };
    match action {
        SliceAction::Open { .. } => open(cli, &composition, &target),
        SliceAction::Pr { .. } => pr(cli, &composition, &target),
        SliceAction::Merge { .. } => merge(cli, &composition, target),
        SliceAction::Drop { .. } => drop(cli, &composition, &target),
    }
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

/// The exit code a slice verb's error maps to.
fn code_for_slice(error: &SliceError) -> u8 {
    match error {
        SliceError::NoEpic { .. }
        | SliceError::Conflicted { .. }
        | SliceError::GateFailed { .. } => REFUSED,
        SliceError::NoVersion => BACKEND,
        SliceError::Port { error, .. } => code_for(error),
    }
}

/// Prints a slice verb's error, naming its step, and returns its code.
pub(super) fn fail_slice(path: &str, cli: &Cli, error: &SliceError) -> u8 {
    let hint = match error {
        SliceError::NoVersion => format!("; set {VERSION_VAR}"),
        _ => String::new(),
    };
    eprintln!("zaf {path}: {error}{hint}");
    if let SliceError::GateFailed { output } = error {
        eprintln!("{output}");
    }
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

/// Runs a fallible composition step, printing a failure the way every verb does.
fn compose<T>(path: &str, built: Result<T, CompositionError>) -> Result<T, u8> {
    built.map_err(|error| fail_composition(path, &error))
}

fn open(cli: &Cli, composition: &Composition, target: &SliceTarget) -> u8 {
    let path = cli.verb.path();
    let vcs = match compose(&path, composition.vcs()) {
        Ok(vcs) => vcs,
        Err(code) => return code,
    };
    match slice::open(vcs.as_ref(), target) {
        Ok(opening) => {
            let (outcome, verb) = match opening {
                SliceOpening::Opened => ("opened", "opened"),
                SliceOpening::AlreadyOpen => ("already_open", "already open"),
            };
            let dir = workspace_dir(composition, target);
            let json = json!({
                "task": target.ticket.number.get(),
                "slice": target.slice.as_ref(),
                "base": target.base.as_ref(),
                "workspace": dir,
                "outcome": outcome,
            });
            let terse = format!(
                "{} {verb} on {} at {}",
                target.slice,
                target.base,
                dir.display()
            );
            print(cli, &json, terse)
        }
        Err(error) => fail_slice(&path, cli, &error),
    }
}

fn pr(cli: &Cli, composition: &Composition, target: &SliceTarget) -> u8 {
    let path = cli.verb.path();
    let ports = compose(&path, composition.vcs())
        .and_then(|vcs| Ok((vcs, compose(&path, composition.forge())?)));
    let (vcs, forge) = match ports {
        Ok(ports) => ports,
        Err(code) => return code,
    };
    match slice::pr(vcs.as_ref(), forge.as_ref(), target) {
        Ok(report) => {
            record_pr(composition, target.ticket.number, report.pr);
            let published = report.publication == Publication::Published;
            let json = json!({
                "task": target.ticket.number.get(),
                "slice": target.slice.as_ref(),
                "pr": bare_pr(report.pr),
                "commit": report.commit.as_ref().map(AsRef::<str>::as_ref),
                "published": published,
            });
            let terse = format!("{} for {} into {}", report.pr, target.slice, target.base);
            print(cli, &json, terse)
        }
        Err(error) => fail_slice(&path, cli, &error),
    }
}

fn merge(cli: &Cli, composition: &Composition, target: SliceTarget) -> u8 {
    let path = cli.verb.path();
    let built = compose(&path, composition.board()).and_then(|board| {
        let vcs = compose(&path, composition.vcs())?;
        let forge = compose(&path, composition.forge())?;
        Ok((board, vcs, forge))
    });
    let (board, vcs, forge) = match built {
        Ok(ports) => ports,
        Err(code) => return code,
    };
    let gate: Option<Box<dyn Gate>> =
        match composition.gate_in(&workspace_dir(composition, &target)) {
            Ok(gate) => Some(gate),
            Err(CompositionError::Absent { .. }) => None,
            Err(error) => return fail_composition(&path, &error),
        };
    let ports = MergePorts {
        board: board.as_ref(),
        vcs: vcs.as_ref(),
        forge: forge.as_ref(),
        gate: gate.as_deref(),
    };
    let task = target.ticket.number;
    let command = MergeCommand {
        pr: recorded_pr(composition, task),
        target,
    };
    match slice::merge(ports, &command) {
        Ok(MergeOutcome::Merged(report)) => {
            forget_pr(composition, task);
            let base = &command.target.base;
            let json = json!({
                "task": task.get(),
                "slice": command.target.slice.as_ref(),
                "pr": bare_pr(report.pr),
                "outcome": "merged",
                "gate": report.gate.is_some(),
            });
            let terse = format!("{} merged into {base}; {task} done", report.pr);
            print(cli, &json, terse)
        }
        Ok(MergeOutcome::AlreadyDone) => {
            forget_pr(composition, task);
            let json = json!({ "task": task.get(), "outcome": "already_done" });
            print(cli, &json, format!("{task} already done"))
        }
        Err(error) => fail_slice(&path, cli, &error),
    }
}

fn drop(cli: &Cli, composition: &Composition, target: &SliceTarget) -> u8 {
    let path = cli.verb.path();
    let vcs = match compose(&path, composition.vcs()) {
        Ok(vcs) => vcs,
        Err(code) => return code,
    };
    match slice::drop(vcs.as_ref(), target) {
        Ok(()) => {
            forget_pr(composition, target.ticket.number);
            let json =
                json!({ "task": target.ticket.number.get(), "slice": target.slice.as_ref() });
            print(cli, &json, format!("{} dropped", target.slice))
        }
        Err(error) => fail_slice(&path, cli, &error),
    }
}

/// The slice's workspace: `[workspaces] path` under the project root, then the slice name.
fn workspace_dir(composition: &Composition, target: &SliceTarget) -> PathBuf {
    composition
        .root()
        .join(&composition.manifest().workspaces.path)
        .join(target.slice.as_ref())
}

fn bare_pr(pr: PrNumber) -> String {
    pr.to_string().trim_start_matches('#').to_owned()
}

fn pr_record(composition: &Composition, task: IssueNumber) -> PathBuf {
    composition
        .state_dir()
        .join(SLICES_DIR)
        .join(format!("{}.pr", task.get()))
}

/// Records the task's pull request so `slice merge` finds it even once it is no longer open.
/// Best effort: without the record, `slice merge` looks up the open pull request instead.
fn record_pr(composition: &Composition, task: IssueNumber, pr: PrNumber) {
    let file = pr_record(composition, task);
    let written = file.parent().map_or(Ok(()), |dir| {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(".gitignore"), "*\n")?;
        std::fs::write(&file, bare_pr(pr))
    });
    if let Err(error) = written {
        eprintln!("zaf: cannot record {pr} in {}: {error}", file.display());
    }
}

fn recorded_pr(composition: &Composition, task: IssueNumber) -> Option<PrNumber> {
    let text = std::fs::read_to_string(pr_record(composition, task)).ok()?;
    text.trim().parse::<u64>().ok().map(PrNumber::from)
}

fn forget_pr(composition: &Composition, task: IssueNumber) {
    // A record already gone is the state wanted.
    let _ = std::fs::remove_file(pr_record(composition, task));
}
