//! The last report on a task, read from `.zaf/hub/reports` (docs/zaf-actions.md §9, 18).
//!
//! Writing reports is another unit's work; this reads what it will leave. Until that unit
//! settles the format, a report is the file `<task number>` in the folder: its first
//! non-blank line is the word, its modified time is when it was made. The hub only reads.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::IssueNumber;

/// The word of a ticket with no report on it.
pub const WORKING: &str = "working";

/// The folder the reports are in, under the project's state folder.
pub fn reports_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("hub").join("reports")
}

/// The last report on `task`: its word and when it was made. None when there is none, or it
/// holds no word, so the ticket says [`WORKING`] and no time.
pub fn last_report(state_dir: &Path, task: IssueNumber) -> Option<(String, SystemTime)> {
    let path = reports_dir(state_dir).join(task.get().to_string());
    let text = std::fs::read_to_string(&path).ok()?;
    let word = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let made = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()?;
    Some((word.to_owned(), made))
}
