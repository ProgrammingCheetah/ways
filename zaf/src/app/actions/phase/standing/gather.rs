//! Gathers what [`derive_standing`](super::derive_standing) decides on: the forge's word on the
//! recorded pull request, and the phase run folders (see the module above for their shape).
//!
//! Nothing here writes: it reads the forge, lists a folder and reads files and their times.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{CheckResult, PhaseRun, PullFacts, ReviewFact, RunExit, StandingFacts};
use crate::app::actions::{ActionName, SliceRecords};
use crate::composition::STATE_DIR;
use crate::domain::TicketId;
use crate::ports::{CheckState, Forge, Holder, PortError};

/// The folder, under the state folder, that holds the phase run folders.
pub const PHASES_DIR: &str = "phases";

/// A read the standing needs that failed: the forge or a run folder could not say.
///
/// A failed read is not a fact about the task, so no standing is derived and none is guessed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{task}'s standing cannot be read: {reason}")]
pub struct StandingUnreadable {
    /// The task whose standing was being derived.
    pub task: TicketId,
    /// What could not be read, and why.
    pub reason: String,
}

/// The facts about `task`'s standing as they stand now.
///
/// `ready_since` is when the task became ready for its first phase; the board keeps no such time,
/// so the caller supplies it. `alive` says whether a recorded process still runs. A recorded pull
/// request the forge cannot read, a finished check with no time, a merged pull request with no
/// merge time or a run folder that cannot be read is a [`StandingUnreadable`], never a guess.
/// Nothing is written.
pub fn gather_standing_facts(
    forge: &dyn Forge,
    root: &Path,
    task: TicketId,
    ready_since: SystemTime,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<StandingFacts, StandingUnreadable> {
    let unreadable = |reason: String| StandingUnreadable { task, reason };
    let pull = match SliceRecords::new(root).pr(task) {
        Some(pr) => Some(
            read_pull(forge, pr)
                .map_err(|reason| unreadable(format!("pull request {pr}: {reason}")))?,
        ),
        None => None,
    };
    let runs =
        read_runs(&root.join(STATE_DIR).join(PHASES_DIR), task, alive).map_err(unreadable)?;
    Ok(StandingFacts {
        ready_since,
        pull,
        runs,
    })
}

/// The forge's facts about one pull request.
fn read_pull(forge: &dyn Forge, pr: crate::ports::PrNumber) -> Result<PullFacts, String> {
    let failed = |source: PortError| source.to_string();
    let merged = forge.merged(pr).map_err(failed)?;
    let request = forge.pull_request(pr).map_err(failed)?;
    let merged_at = if merged {
        Some(
            request
                .merged_at
                .ok_or_else(|| "it is merged but the forge gives no merge time".to_owned())?,
        )
    } else {
        None
    };
    let head = forge.head(pr).map_err(failed)?;
    let mut checks = Vec::new();
    let mut failed_checks = Vec::new();
    for check in forge.checks(pr).map_err(failed)? {
        if check.state == CheckState::Failed {
            failed_checks.push(check.name.to_string());
        }
        checks.push(match (check.state, check.finished_at) {
            (CheckState::Pending, _) => CheckResult::Pending,
            (CheckState::Passed, Some(at)) => CheckResult::Passed(at),
            (CheckState::Failed, Some(at)) => CheckResult::Failed(at),
            (CheckState::Passed | CheckState::Failed, None) => {
                return Err(format!("check {} finished with no time", check.name));
            }
        });
    }
    let reviews = forge
        .review_comments(pr)
        .map_err(failed)?
        .into_iter()
        .map(|comment| ReviewFact {
            head: comment.head.map(|head| head.to_string()),
            posted_at: comment.posted_at,
        })
        .collect();
    Ok(PullFacts {
        opened_at: request.opened_at,
        merged_at,
        head,
        checks,
        failed_checks,
        reviews,
    })
}

/// Every run folder of `task` under `dir`. An absent folder is no runs; a folder of another task,
/// or one without its `task` or `action` file, is not this task's run.
fn read_runs(
    dir: &Path,
    task: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Vec<PhaseRun>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        if let Some(run) = read_run(&path, task, alive)? {
            runs.push(run);
        }
    }
    runs.sort_by_key(|run| run.started_at);
    Ok(runs)
}

/// The tasks with a phase process running under `dir`: a run folder with no exit recorded and a
/// holder that is alive. An absent folder is none; a folder that is no run folder names no task.
pub fn running_tasks(
    dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<BTreeSet<TicketId>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut tasks = BTreeSet::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(text) = read_text(&path.join("task"))? else {
            continue;
        };
        let Ok(task) = text.trim().parse::<TicketId>() else {
            continue;
        };
        if read_run(&path, task, alive)?.is_some_and(|run| run.alive) {
            tasks.insert(task);
        }
    }
    Ok(tasks)
}

/// The instances of the phase run folders under `dir` that no longer run: a run folder with an
/// exit recorded, or whose holder is gone. A folder with no `instance` file names none. An absent
/// folder is none.
pub fn ended_instances(
    dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<BTreeSet<String>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    let mut instances = BTreeSet::new();
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(task) =
            read_text(&path.join("task"))?.and_then(|text| text.trim().parse::<TicketId>().ok())
        else {
            continue;
        };
        let Some(instance) = read_text(&path.join("instance"))?
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        if read_run(&path, task, alive)?.is_some_and(|run| !run.alive) {
            instances.insert(instance);
        }
    }
    Ok(instances)
}

/// One run folder; none when it is not `task`'s or is not a run folder.
fn read_run(
    dir: &Path,
    task: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Option<PhaseRun>, String> {
    let task_file = dir.join("task");
    let Some(text) = read_text(&task_file)? else {
        return Ok(None);
    };
    if text.trim().parse::<u64>().ok() != Some(task.get()) {
        return Ok(None);
    }
    let Some(action) = read_text(&dir.join("action"))? else {
        return Ok(None);
    };
    let action = action.trim();
    if action.is_empty() {
        return Ok(None);
    }
    let started_at = modified(&task_file)?;
    let head = read_text(&dir.join("head"))?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let exit_file = dir.join("exit");
    let exit = match read_text(&exit_file)? {
        Some(text) => match text.trim().parse::<i32>() {
            Ok(code) => Some(RunExit {
                code,
                at: modified(&exit_file)?,
            }),
            Err(_) => None,
        },
        None => None,
    };
    let ended = exit_file.exists();
    let holder = read_text(&dir.join("pid"))?.and_then(|text| Holder::parse(&text));
    let alive = !ended && holder.as_ref().is_some_and(alive);
    Ok(Some(PhaseRun {
        action: ActionName::from(action),
        head,
        started_at,
        alive,
        exit,
    }))
}

/// A file's text; none when it is absent.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(format!("{}: {source}", path.display())),
    }
}

/// When a file was last written.
fn modified(path: &PathBuf) -> Result<SystemTime, String> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(|source| format!("{}: {source}", path.display()))
}
