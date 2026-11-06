//! The slice records: what zaf keeps of what `slice-open` and `slice-pr` hand back
//! (docs/zaf-actions.md §3, "Handing a value back" and "The records").
//!
//! The phase guard reads these records, and a handed-back value is whatever a body wrote last, so
//! nothing is kept until it holds: a value that does not is `FAIL`, exit 2, naming the log, with
//! the record left as it was. The task comes from the caller's arguments and only ever reaches a
//! file name as a number, so no argument can point a record outside its folder.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::run::{RunOutcome, RunRequest, RunResult};
use crate::app::brief::RUNS_DIR;
use crate::app::check::{CheckRecord, Target};
use crate::composition::STATE_DIR;
use crate::domain::IssueNumber;
use crate::ports::PrNumber;

/// The action whose hand-back is the task's workspace.
const SLICE_OPEN: &str = "slice-open";

/// The action whose hand-back is the task's pull request.
const SLICE_PR: &str = "slice-pr";

/// The actions that end a slice: on exit 0 the records are forgotten.
const SLICE_ENDS: [&str; 2] = ["slice-merge", "slice-drop"];

/// The folder of `.zaf/` that holds the records.
const SLICES_DIR: &str = "slices";

/// The exit code of a body that exited 0 and handed back nothing zaf can keep.
const BAD_HAND_BACK_EXIT: u8 = 2;

/// How much of a refused value a refusal shows.
const SHOWN_CHARS: usize = 200;

/// The value a body hands back: the last non-empty line of its log, without its line ending. A
/// line of blanks alone is empty; `None` when the log has no other. Bytes that are no text are
/// replaced, so they can never name a real directory or a number.
pub fn hand_back(log: &Path) -> io::Result<Option<String>> {
    let mut reader = BufReader::new(File::open(log)?);
    let mut line = Vec::new();
    let mut last = None;
    while reader.read_until(b'\n', &mut line)? > 0 {
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if !text.trim().is_empty() {
            last = Some(text.to_owned());
        }
        line.clear();
    }
    Ok(last)
}

/// A project's records: `.zaf/slices/<task>` and `.zaf/slices/<task>.pr`, plain files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceRecords {
    dir: PathBuf,
    /// Where the check records are: `.zaf/runs`.
    runs: PathBuf,
}

impl SliceRecords {
    /// The records of the project at `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            dir: root.join(STATE_DIR).join(SLICES_DIR),
            runs: root.join(STATE_DIR).join(RUNS_DIR),
        }
    }

    /// Forgets the task: both slice records, and the check records of the task and of its
    /// recorded pull request (`.zaf/runs/check-task-<task>.json`, `check-pr-<n>.json`), so a
    /// dropped task is back in `claim` and a reopened slice starts clean. A record that is not
    /// there is already forgotten. Every removal is tried, and the first error is returned.
    pub fn forget(&self, task: IssueNumber) -> io::Result<()> {
        // Read before the record goes: it names the pull request whose check record goes too.
        let mut doomed = vec![
            self.dir.join(workspace_file(task)),
            self.dir.join(pr_file(task)),
            self.runs.join(check_file(Target::Task(task))),
        ];
        if let Some(pr) = self.pr(task) {
            doomed.push(self.runs.join(check_file(Target::Pr { pr, task: None })));
        }
        let mut first = Ok(());
        for file in doomed {
            match fs::remove_file(&file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound && first.is_ok() => {
                    first = Err(error);
                }
                _ => {}
            }
        }
        first
    }

    /// The task's workspace. Read by the same rule that wrote it: a record that does not hold, or
    /// whose directory has gone, is no record, so the guard falls to the earlier phase.
    pub fn workspace(&self, task: IssueNumber) -> Option<PathBuf> {
        let text = fs::read_to_string(self.dir.join(workspace_file(task))).ok()?;
        workspace_value(&text).map(Path::to_owned)
    }

    /// The task's pull request. Read by the same rule that wrote it: a record that does not hold
    /// is no record.
    pub fn pr(&self, task: IssueNumber) -> Option<PrNumber> {
        let text = fs::read_to_string(self.dir.join(pr_file(task))).ok()?;
        pr_value(&text)
    }

    /// When the task's pull request record was written; none when that cannot be read.
    pub fn pr_recorded_at(&self, task: IssueNumber) -> Option<SystemTime> {
        let metadata = fs::metadata(self.dir.join(pr_file(task))).ok()?;
        metadata.modified().ok()
    }

    /// The last `zaf check <task>` record; one that cannot be read is no record.
    pub fn task_check(&self, task: IssueNumber) -> Option<CheckRecord> {
        self.check_record(Target::Task(task))
    }

    /// The last `zaf check pr` record of a pull request; one that cannot be read is no record.
    pub fn pr_check(&self, pr: PrNumber) -> Option<CheckRecord> {
        self.check_record(Target::Pr { pr, task: None })
    }

    fn check_record(&self, target: Target) -> Option<CheckRecord> {
        let text = fs::read_to_string(self.runs.join(check_file(target))).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes one record whole, or not at all. The rename replaces whatever sits at the name, so
    /// a link planted there is replaced rather than written through.
    fn write(&self, name: &str, value: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let temp = self.dir.join(format!(".{name}.{}.tmp", std::process::id()));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .and_then(|mut file| file.write_all(value.as_bytes()))
            .and_then(|()| fs::rename(&temp, self.dir.join(name)));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written
    }
}

fn workspace_file(task: IssueNumber) -> String {
    task.get().to_string()
}

fn pr_file(task: IssueNumber) -> String {
    format!("{}.pr", task.get())
}

/// The file name of a check target's record, as `zaf check` stores it.
fn check_file(target: Target) -> String {
    format!("check-{}.json", target.slug())
}

/// `text` as a workspace: an absolute path of an existing directory. Absolute because the record
/// is read later from elsewhere, where a relative path would name some other directory.
fn workspace_value(text: &str) -> Option<&Path> {
    let path = Path::new(text);
    (path.is_absolute() && path.is_dir()).then_some(path)
}

/// `text` as a pull request: ASCII digits alone, naming a number a forge can give out.
fn pr_value(text: &str) -> Option<PrNumber> {
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = text.parse::<u64>().ok().filter(|number| *number > 0)?;
    Some(PrNumber::from(number))
}

/// Keeps what a finished run handed back, when it is a slice name that exited 0. A hand-back that
/// cannot be kept turns the run into `FAIL`, exit 2, naming the log; any other run passes through.
pub(super) fn keep_hand_back(request: &RunRequest<'_>, outcome: RunOutcome) -> RunOutcome {
    if outcome.result != RunResult::Ok || !matches!(request.name, SLICE_OPEN | SLICE_PR) {
        return outcome;
    }
    let kept = outcome
        .log
        .as_deref()
        .ok_or_else(|| "no log to read a value from".to_owned())
        .and_then(|log| keep(request, log));
    refuse_if(kept, outcome)
}

/// Forgets the task's records when a slice-ending name exited 0. Records that cannot be forgotten
/// turn the run into `FAIL`, exit 2, rather than leave the guard reading a slice that has ended;
/// a run that did not exit 0 forgets nothing, and any other name passes through.
pub(super) fn forget_records(request: &RunRequest<'_>, outcome: RunOutcome) -> RunOutcome {
    if outcome.result != RunResult::Ok || !SLICE_ENDS.contains(&request.name) {
        return outcome;
    }
    let forgotten = task_and_root(request).and_then(|(task, root)| {
        SliceRecords::new(root)
            .forget(task)
            .map_err(|error| format!("cannot forget the records: {error}"))
    });
    refuse_if(forgotten, outcome)
}

/// `outcome`, or `FAIL` with exit 2 and `reason` when the records could not be dealt with.
fn refuse_if(done: Result<(), String>, outcome: RunOutcome) -> RunOutcome {
    match done {
        Ok(()) => outcome,
        Err(reason) => RunOutcome {
            result: RunResult::Fail,
            exit: BAD_HAND_BACK_EXIT,
            meaning: Some(reason),
            ..outcome
        },
    }
}

/// The task the caller named first, and the project whose records it is.
fn task_and_root<'a>(request: &RunRequest<'a>) -> Result<(IssueNumber, &'a Path), String> {
    let task = request
        .args
        .first()
        .and_then(|task| task.to_str())
        .and_then(|task| task.parse::<IssueNumber>().ok())
        .ok_or("no task number as its first argument")?;
    let root = request
        .project_root
        .ok_or("no project to keep the records in")?;
    Ok((task, root))
}

/// Reads the task, the place and the value, and writes the record once all of them hold.
fn keep(request: &RunRequest<'_>, log: &Path) -> Result<(), String> {
    let (task, root) = task_and_root(request)?;
    let value = hand_back(log)
        .map_err(|error| format!("cannot read the value: {error}"))?
        .ok_or("handed back no value")?;
    let records = SliceRecords::new(root);
    let written = if request.name == SLICE_OPEN {
        workspace_value(&value).ok_or_else(|| {
            let shown = shown(&value);
            format!("handed back {shown}, not an absolute path of an existing directory")
        })?;
        records.write(&workspace_file(task), &value)
    } else {
        let number = pr_value(&value).ok_or_else(|| {
            let shown = shown(&value);
            format!("handed back {shown}, not a pull request number")
        })?;
        records.write(&pr_file(task), &number.get().to_string())
    };
    written.map_err(|error| format!("cannot write the record: {error}"))
}

/// A refused value as a refusal prints it: quoted, so a control character in it reaches no
/// terminal, and cut short, so a body cannot fill the line.
fn shown(value: &str) -> String {
    let start: String = value.chars().take(SHOWN_CHARS).collect();
    format!("{start:?}")
}

#[cfg(test)]
mod tests;
