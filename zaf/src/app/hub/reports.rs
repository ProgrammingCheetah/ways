//! The reports file `.zaf/hub/reports` (docs/zaf-actions.md §5, §9 assumption 18).
//!
//! One line per report: `<epoch seconds> <task, or -> <worker> <what happened>`. The task is the
//! issue number without a `#`; the worker is one word (`programmer`, `team`, a role); the rest of
//! the line is what happened, in a few general words and never a command. The file keeps its last
//! [`MAX_LINES`] lines, the oldest going first. Writing a report never fails its caller: a report
//! that cannot be written is dropped. The hub only reads.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::domain::TicketId;

/// The word of a ticket with no report on it.
pub const WORKING: &str = "working";

/// The most lines the file holds; the oldest go first.
pub const MAX_LINES: usize = 1000;

/// The file the reports are in, under the project's state folder.
pub fn reports_file(state_dir: &Path) -> PathBuf {
    state_dir.join("hub").join("reports")
}

/// One report: when, on which task, by whom and what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// When it was made, to the second.
    pub at: SystemTime,
    /// The task it is about, when there is one.
    pub task: Option<TicketId>,
    /// Who made it: one word.
    pub worker: String,
    /// What happened, in general words.
    pub what: String,
}

impl Report {
    /// The report's line, without its newline. A worker or a `what` is cut to one line of plain
    /// words: control characters become spaces and a worker holds no whitespace.
    pub fn to_line(&self) -> String {
        let seconds = self
            .at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let task = self
            .task
            .map_or_else(|| "-".to_owned(), |task| task.get().to_string());
        let worker: String = self
            .worker
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .collect();
        let worker = if worker.is_empty() {
            "team".to_owned()
        } else {
            worker
        };
        let what = self
            .what
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>();
        let what = what.split_whitespace().collect::<Vec<_>>().join(" ");
        let what = if what.is_empty() {
            WORKING.to_owned()
        } else {
            what
        };
        format!("{seconds} {task} {worker} {what}")
    }

    /// Reads a line; none when it is not one.
    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.trim().splitn(4, ' ');
        let seconds = parts.next()?.parse::<u64>().ok()?;
        let task = match parts.next()? {
            "-" => None,
            number => Some(TicketId::from(number.parse::<u64>().ok()?)),
        };
        let worker = parts.next()?.to_owned();
        let what = parts.next()?.trim().to_owned();
        if what.is_empty() {
            return None;
        }
        Some(Self {
            at: UNIX_EPOCH + Duration::from_secs(seconds),
            task,
            worker,
            what,
        })
    }
}

/// Appends a report, then trims the file to its last [`MAX_LINES`] lines.
pub fn append(state_dir: &Path, report: &Report) -> std::io::Result<()> {
    let path = reports_file(state_dir);
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    // One write, so two reports made at once do not interleave inside a line.
    file.write_all(format!("{}\n", report.to_line()).as_bytes())?;
    drop(file);
    trim(&path)
}

/// Appends a report and ignores a failure: a report that cannot be written never fails or
/// changes the verb or the tool call that made it.
pub fn record(state_dir: &Path, report: &Report) {
    let _ = append(state_dir, report);
}

/// Keeps the last [`MAX_LINES`] lines of the file.
fn trim(path: &Path) -> std::io::Result<()> {
    let text = fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_LINES {
        return Ok(());
    }
    let kept = lines[lines.len() - MAX_LINES..].join("\n");
    let scratch = path.with_extension("tmp");
    fs::write(&scratch, format!("{kept}\n"))?;
    fs::rename(&scratch, path)
}

/// The last report on `task`: its word and when it was made. None when there is none, so the
/// ticket says [`WORKING`] and no time.
pub fn last_report(state_dir: &Path, task: TicketId) -> Option<(String, SystemTime)> {
    let text = fs::read_to_string(reports_file(state_dir)).ok()?;
    text.lines()
        .rev()
        .filter_map(Report::parse)
        .find(|report| report.task == Some(task))
        .map(|report| (report.what, report.at))
}
