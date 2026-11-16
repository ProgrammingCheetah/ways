//! The phase run folder (docs/zaf-actions.md §5.1): the one place that writes it and reads it
//! back, for a phase the launcher runs and for a `zaf`-kind action the tick runs inline.
//!
//! # The phase run folder
//!
//! One folder for each run of a phase action for a task, in `.zaf/phases/` ([`PHASES_DIR`]); its
//! name is the writer's to choose (`<instance slug>-task-<n>`), since the reader reads files and
//! never a name. A folder holds:
//!
//! - `task`: the task's number. Written last of the files a folder opens with ([`open`]); its
//!   modified time is when the run started.
//! - `action`: the action's name.
//! - `head`: the pull request's head when the run started; empty when the task has no pull
//!   request.
//! - `size`: the task's size, as `ZAF_SIZE` carries it.
//! - `instance`: the instance it runs as, `<action> instance <n> on #<epic>`.
//! - `pid`: the process, as a [`Holder`]: the pid, a space, the start the OS reported. Written once
//!   the process starts. An inline run has none: it is a step of the tick's own process, which a
//!   stop must not end, and no process of its own holds a slot.
//! - `inline`: an inline run's mark that it runs: the tick's own process, as a [`Holder`], written
//!   before `task`, so the run reads as running from its first moment, and a second tick that
//!   reads it leaves the ticket alone. `zaf spawn stop` reads only `pid`, so it never ends the
//!   tick. Absent for a launched run.
//! - `inline-body`: an inline run's action, the process of its body as a [`Holder`], written
//!   whole once the body has started ([`mark_inline_body`]). The body runs in a process group of
//!   its own and outlives a tick that dies, so the run reads as running while either lives.
//!   Absent for a launched run.
//! - `output`: what the action's `zaf do` printed; for a failed run its last line is the last line
//!   of the action's log. Absent in a folder from before it was kept.
//! - `questions`: the issue numbers of the questions raised during the run, one per line, in the
//!   order they were raised ([`record_question`]). Absent when the run raised none.
//! - `head-end`: the pull request's head when the run ended, read after its process exited
//!   ([`head_now`]). Empty when the task had no pull request, or one with no head
//!   ([`HeadEnd::Left`] of none). Absent when the head could not be read, and in a folder from
//!   before it was kept: both read as [`HeadEnd::Unknown`], which never counts toward getting
//!   nowhere.
//! - `exit`: the process's exit code as a decimal number and a newline, written last of all, once
//!   the process has ended (the ruling on #451); its modified time is when the run ended. Absent
//!   while the run goes.
//! - `post-phase.log`: what went wrong once the phase had ended, when something did: what a failed
//!   `post-phase` hook said, and a running scheduler that could not be asked for a tick.
//!
//! A run is live while its `pid`, its `inline` mark or its `inline-body` names a live process and
//! no `exit` is there; whether a process lives is asked of the function the reader is handed. A folder with a dead pid, or none, and no `exit` has ended with no exit code, which is
//! never read as 0; an `exit` that holds no number is no exit code either, and the run is not live.
//!
//! The opening files are written before `task`, and `pid`, `head-end` and `exit` (and an inline
//! run's `inline-body` and `output`) are written whole and renamed into place, so a reader sees each whole or not at
//! all. A launched run's `output` is written as its process prints, and `questions` is appended
//! to, one short line at a time.

use std::collections::BTreeSet;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::app::actions::SliceRecords;
use crate::app::spawn::slug;
use crate::domain::schedule::{ActionName, ExitCode, HeadEnd, OutputLine, RunExit, RunRecord};
use crate::domain::{CommitId, Instance, TaskSize, TicketId};
use crate::ports::{Forge, Holder};

/// The folder, under the state folder, that holds the phase run folders.
pub const PHASES_DIR: &str = "phases";
/// The file of a phase run folder that holds the task's number; written last of the opening files.
pub const TASK_FILE: &str = "task";
/// The file of a phase run folder that holds the action's name.
pub const ACTION_FILE: &str = "action";
/// The file of a phase run folder that holds the pull request's head when it started.
pub const HEAD_FILE: &str = "head";
/// The file of a phase run folder that names the size the action runs at (`ZAF_SIZE`).
pub const SIZE_FILE: &str = "size";
/// The file of a phase run folder that names the instance the action runs as (`ZAF_INSTANCE`).
pub const INSTANCE_FILE: &str = "instance";
/// The file of a phase run folder that holds the action's process, as a holder.
pub const PID_FILE: &str = "pid";
/// The file of an inline run's folder that holds the tick's own process, as a holder, while the
/// run goes.
pub const INLINE_FILE: &str = "inline";
/// The file of an inline run's folder that holds its body's process, as a holder, once the body
/// has started.
pub const INLINE_BODY_FILE: &str = "inline-body";
/// The file of a phase run folder that holds what the action's `zaf do` printed (#550).
pub const OUTPUT_FILE: &str = "output";
/// The file of a phase run folder that holds the questions raised during the run, one per line.
pub const QUESTIONS_FILE: &str = "questions";
/// The file of a phase run folder that holds the pull request's head when the run ended.
pub const HEAD_END_FILE: &str = "head-end";
/// The file of a phase run folder that holds the process's exit code, written once it has ended.
pub const EXIT_FILE: &str = "exit";
/// The file of a phase run folder that holds what went wrong once the phase had ended: what a
/// failed `post-phase` hook said, a scheduler that could not be asked for a tick.
pub const POST_PHASE_LOG: &str = "post-phase.log";

/// What a run folder opens with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening<'a> {
    /// The task.
    pub task: TicketId,
    /// The action it runs.
    pub action: &'a ActionName,
    /// The pull request's head when it starts; none without one.
    pub head: Option<&'a CommitId>,
    /// The task's size.
    pub size: TaskSize,
    /// The instance it runs as.
    pub instance: &'a Instance,
    /// For an inline run, the tick's own process, written as `inline` before `task`; none for a
    /// launched run, whose launcher records its `pid`.
    pub inline: Option<&'a Holder>,
}

/// Opens a run folder under `phases_dir` for `opening`, named `<instance slug>-task-<n>`
/// (`implement-instance-3-on-1-task-4`): makes it and writes `action`, `head`, `size`, `instance`,
/// an inline run's `inline` and, last, `task`, whose time is when the run started. Returns the
/// folder.
pub fn open(phases_dir: &Path, opening: &Opening<'_>) -> Result<PathBuf, String> {
    let name = format!("{}-task-{}", slug(opening.instance), opening.task.get());
    let run_dir = phases_dir.join(name);
    std::fs::create_dir_all(&run_dir)
        .map_err(|source| format!("cannot make {}: {source}", run_dir.display()))?;
    let write = |name: &str, text: &str| -> Result<(), String> {
        let path = run_dir.join(name);
        std::fs::write(&path, text)
            .map_err(|source| format!("cannot write {}: {source}", path.display()))
    };
    write(ACTION_FILE, opening.action.as_str())?;
    write(HEAD_FILE, opening.head.map_or("", CommitId::as_str))?;
    write(SIZE_FILE, opening.size.name())?;
    write(INSTANCE_FILE, &opening.instance.to_string())?;
    if let Some(holder) = opening.inline {
        write(INLINE_FILE, &holder.to_string())?;
    }
    // Last: its time is when the run started.
    write(TASK_FILE, &opening.task.get().to_string())?;
    Ok(run_dir)
}

/// Writes `text` to the run folder's `name` whole: staged under a name of its own, then renamed
/// into place, so a reader sees no file or the whole of it. The staging name carries the writer's
/// process id, since two writers may record one file.
fn write_whole(run_dir: &Path, name: &str, text: &str) -> Result<(), String> {
    let path = run_dir.join(name);
    let staging = run_dir.join(format!("{name}.{}.new", std::process::id()));
    std::fs::write(&staging, text)
        .and_then(|()| std::fs::rename(&staging, &path))
        .map_err(|source| {
            let _ = std::fs::remove_file(&staging);
            format!("cannot write {}: {source}", path.display())
        })
}

/// Records a process in the run folder as its `pid`, as a holder. The file is replaced whole, so a
/// reader sees the old record or the new one, never half of either: the verb and the holder both
/// record.
pub fn record_pid(run_dir: &Path, holder: &Holder) -> Result<(), String> {
    write_whole(run_dir, PID_FILE, &holder.to_string())
}

/// Records an inline run's body, `holder`, as the run folder's `inline-body`, written whole: the run
/// reads as running while the body lives, even once the tick that started it has gone.
pub fn mark_inline_body(run_dir: &Path, holder: &Holder) -> Result<(), String> {
    write_whole(run_dir, INLINE_BODY_FILE, &holder.to_string())
}

/// Takes an inline run's `inline` mark and its `inline-body` away, for a run whose `exit` could not
/// be written: the run then reads as ended with no exit code, rather than as running for as long
/// as the tick's process lives. No mark is nothing to take.
pub fn clear_inline(run_dir: &Path) -> Result<(), String> {
    for name in [INLINE_FILE, INLINE_BODY_FILE] {
        let path = run_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::NotFound => {}
            Err(source) => return Err(format!("cannot remove {}: {source}", path.display())),
        }
    }
    Ok(())
}

/// Records what the run printed as the run folder's `output`, for a run whose output no launcher
/// sent there (an inline run).
pub fn record_output(run_dir: &Path, text: &str) -> Result<(), String> {
    write_whole(run_dir, OUTPUT_FILE, text)
}

/// Records the head the run left as the run folder's `head-end`: the head, or nothing for
/// [`HeadEnd::Left`] of none. [`HeadEnd::Unknown`] writes no file, which reads back as unknown.
pub fn record_head_end(run_dir: &Path, head_end: &HeadEnd) -> Result<(), String> {
    match head_end {
        HeadEnd::Unknown => Ok(()),
        HeadEnd::Left(head) => write_whole(
            run_dir,
            HEAD_END_FILE,
            head.as_ref().map_or("", CommitId::as_str),
        ),
    }
}

/// Records `code` in the run folder as its `exit` file: the decimal code and a newline, staged
/// whole and renamed into place, so a reader sees no file or the whole of it, never half.
pub fn record_exit(run_dir: &Path, code: i32) -> Result<(), String> {
    write_whole(run_dir, EXIT_FILE, &format!("{code}\n"))
}

/// What ends a run once its process has ended with `code`, having left `head_end`: records the
/// head it left, then the exit, last of all, then runs `after`, the steps that follow (post-phase,
/// the tick's ask). A record that fails is returned (the first, when both do), and what follows it
/// still runs: a `head-end` that cannot be written leaves none, which reads as unknown, and the
/// exit is still written; a failed write must not swallow the run's end.
pub fn end(run_dir: &Path, code: i32, head_end: &HeadEnd, after: &dyn Fn()) -> Result<(), String> {
    let head = record_head_end(run_dir, head_end);
    let exit = record_exit(run_dir, code);
    after();
    head.and(exit)
}

/// The head of `task`'s pull request as it is now, for a run that just ended: [`HeadEnd::Left`] of
/// none when `root` records no pull request for the task, or the forge reports none, and
/// [`HeadEnd::Unknown`] when the forge could not say or the record cannot be read. Never a guess.
pub fn head_now(forge: Option<&dyn Forge>, root: &Path, task: TicketId) -> HeadEnd {
    let pr = match SliceRecords::new(root).pr(task) {
        Ok(Some(pr)) => pr,
        Ok(None) => return HeadEnd::Left(None),
        Err(_) => return HeadEnd::Unknown,
    };
    match forge.map(|forge| forge.head(pr)) {
        Some(Ok(head)) => HeadEnd::Left(head.map(CommitId::from)),
        Some(Err(_)) | None => HeadEnd::Unknown,
    }
}

/// Records `question`, just raised on `on`, in the run folder `run_dir` when it was raised during
/// that run: the folder is a phase run's whose `task` is `on`, and no `exit` is recorded yet. The
/// number is appended to `questions`, one per line. Returns whether it was recorded; a folder that
/// is no phase run's, another task's, or a run that has ended records nothing.
pub fn record_question(run_dir: &Path, on: TicketId, question: TicketId) -> Result<bool, String> {
    let Some(task) = read_text(&run_dir.join(TASK_FILE))? else {
        return Ok(false);
    };
    if task.trim().parse::<TicketId>().ok() != Some(on) || run_dir.join(EXIT_FILE).exists() {
        return Ok(false);
    }
    let path = run_dir.join(QUESTIONS_FILE);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(format!("{}\n", question.get()).as_bytes()))
        .map_err(|source| format!("cannot write {}: {source}", path.display()))?;
    Ok(true)
}

/// Every run folder of `task` under `dir`, oldest start first. An absent folder is no runs; a
/// folder of another task, or one without its `task` or `action` file, is not this task's run.
/// A file that cannot be read, or a `questions` line that is no issue number, is an error naming
/// it, never a guess.
pub fn read_runs(
    dir: &Path,
    task: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Vec<RunRecord>, String> {
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

/// What the run folder of `run`, one of `task`'s runs in `dir`, kept of its output ([`OUTPUT_FILE`]):
/// for a failed inline run its `FAIL` line and the last lines of its log. None when no folder of
/// `task` reads as `run`, or the folder kept none.
pub fn output_of(
    dir: &Path,
    task: TicketId,
    run: &RunRecord,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Option<String>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(format!("{}: {source}", dir.display())),
    };
    for entry in entries {
        let path = entry
            .map_err(|source| format!("{}: {source}", dir.display()))?
            .path();
        if path.is_dir() && read_run(&path, task, alive)?.as_ref() == Some(run) {
            return read_text(&path.join(OUTPUT_FILE));
        }
    }
    Ok(None)
}

/// One run folder as its [`RunRecord`]; none when it is not `task`'s or is not a run folder.
///
/// Its process is alive when no `exit` file is there and its `pid`, its `inline` mark (the tick) or
/// its `inline-body` (the action the tick started) names a live process, as `alive` reads the
/// process table.
pub fn read_run(
    dir: &Path,
    task: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Option<RunRecord>, String> {
    let task_file = dir.join(TASK_FILE);
    let Some(text) = read_text(&task_file)? else {
        return Ok(None);
    };
    if text.trim().parse::<u64>().ok() != Some(task.get()) {
        return Ok(None);
    }
    let Some(action) = read_text(&dir.join(ACTION_FILE))? else {
        return Ok(None);
    };
    let action = action.trim();
    if action.is_empty() {
        return Ok(None);
    }
    let started_at = modified(&task_file)?;
    let head = read_text(&dir.join(HEAD_FILE))?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .map(CommitId::from);
    let head_end = match read_text(&dir.join(HEAD_END_FILE))? {
        None => HeadEnd::Unknown,
        Some(text) => {
            let text = text.trim();
            HeadEnd::Left((!text.is_empty()).then(|| CommitId::from(text)))
        }
    };
    let exit_file = dir.join(EXIT_FILE);
    let exit = match read_text(&exit_file)? {
        Some(text) => match text.trim().parse::<i32>() {
            Ok(code) => Some(RunExit {
                code: ExitCode::from(code),
                at: modified(&exit_file)?,
            }),
            Err(_) => None,
        },
        None => None,
    };
    let last_line = read_text(&dir.join(OUTPUT_FILE))?.and_then(|text| {
        text.lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(OutputLine::from)
    });
    let questions = read_questions(&dir.join(QUESTIONS_FILE))?;
    let ended = exit_file.exists();
    let holder = read_text(&dir.join(PID_FILE))?.and_then(|text| Holder::parse(&text));
    let inline = read_text(&dir.join(INLINE_FILE))?.and_then(|text| Holder::parse(&text));
    let body = read_text(&dir.join(INLINE_BODY_FILE))?.and_then(|text| Holder::parse(&text));
    let alive = !ended && [holder, inline, body].iter().flatten().any(alive);
    Ok(Some(RunRecord {
        action: ActionName::from(action),
        head,
        head_end,
        started_at,
        alive,
        exit,
        questions,
        last_line,
    }))
}

/// The questions a run raised, read from its `questions` file; none when it has none. A line that
/// is no issue number is an error naming the file.
fn read_questions(path: &Path) -> Result<BTreeSet<TicketId>, String> {
    let Some(text) = read_text(path)? else {
        return Ok(BTreeSet::new());
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.parse::<TicketId>()
                .map_err(|_| format!("{} holds `{line}`, not an issue number", path.display()))
        })
        .collect()
}

/// The task a run folder names in its `task` file; none when it has none, or one that is no
/// number.
pub fn task_of(run_dir: &Path) -> Result<Option<TicketId>, String> {
    Ok(read_text(&run_dir.join(TASK_FILE))?.and_then(|text| text.trim().parse().ok()))
}

/// A file's text; none when it is absent.
pub fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(format!("{}: {source}", path.display())),
    }
}

/// When a file was last written.
fn modified(path: &Path) -> Result<SystemTime, String> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(|source| format!("{}: {source}", path.display()))
}

#[cfg(test)]
mod tests;
