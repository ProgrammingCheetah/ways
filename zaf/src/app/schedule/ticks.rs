//! The trace's file, `.zaf/ticks.jsonl` ([`TICKS_FILE`]): the one place that writes it and reads
//! it back (docs/zaf-actions.md §5.1). The lines are [`super::trace`]'s.
//!
//! A real tick appends its line ([`append`]) while it holds `.zaf/ticks.lock` ([`TICKS_LOCK`]),
//! for the few milliseconds of reading the file and writing it, so two ticks that overlap (the
//! tick lock, `spawn.lock`, is held only while a tick plans) never interleave and each line's
//! moves are worked out against the line before it. A tick drops the lines older than
//! [`super::trace::KEEP_FOR`] as it writes: the file is then written whole under a name of its own
//! and renamed into place, so a reader never sees half of it. Otherwise the line is appended in
//! one write. `zaf spawn trace` reads the file with no lock ([`read`]), passing over a line it
//! cannot read, such as the half of one a tick is writing.
//!
//! The file is git-ignored by the allow-list `zaf setup` writes as `.zaf/.gitignore`, which tracks
//! only its own few names.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::SystemTime;

use super::trace::{Before, Entry, Line, Recorded};

/// The trace, under the state folder: one JSON line per real tick.
pub const TICKS_FILE: &str = "ticks.jsonl";

/// The file a tick holds locked while it reads and writes the trace.
pub const TICKS_LOCK: &str = "ticks.lock";

/// What appending a tick's line gave: the line, its moves worked out against the last one, and
/// why it was not written when it was not.
#[derive(Debug, Clone, PartialEq)]
pub struct Appended {
    /// The line, written or not. When the trace could not be read, nothing in it is a move.
    pub line: Line,
    /// Why the line was not written, when it was not.
    pub failed: Option<String>,
}

/// Appends `entry`'s line to the trace under `state_dir` at `now`, holding [`TICKS_LOCK`], and
/// drops the lines older than [`super::trace::KEEP_FOR`] as it does (see the module above). A
/// file that cannot be read, locked or written is named in [`Appended::failed`]; it fails nothing,
/// and the line is still made, with no move when the last line could not be read.
pub fn append(state_dir: &Path, entry: &Entry, now: SystemTime) -> Appended {
    let path = state_dir.join(TICKS_FILE);
    let lock = lock(state_dir);
    let text = match read_text(&path) {
        Ok(text) => text,
        Err(why) => {
            return Appended {
                line: entry.line(None),
                failed: Some(why),
            };
        }
    };
    let before = Before::of(&text, now);
    let line = entry.line(Some(&before.previous));
    let failed = match lock {
        Err(why) => Some(why),
        Ok(_held) => write(&path, &text, &before, line.text()).err(),
    };
    Appended { line, failed }
}

/// Every line of the trace under `state_dir` that reads back, in the file's order; none when there
/// is no trace yet. It takes no lock, so it never holds up a tick, and a line it cannot read is
/// passed over.
pub fn read(state_dir: &Path) -> Result<Option<Vec<Recorded>>, String> {
    let path = state_dir.join(TICKS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(
            String::from_utf8_lossy(&bytes)
                .lines()
                .filter_map(Recorded::parse)
                .collect(),
        )),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(format!("cannot read {}: {source}", path.display())),
    }
}

/// How much of the trace's end [`last_at`] reads: many lines, the longest included.
const TAIL: u64 = 256 * 1024;

/// When the last tick in the trace under `state_dir` began: its last line that reads back, read
/// from the file's end with no lock. None when there is no trace, or no line in its end reads.
pub fn last_at(state_dir: &Path) -> Option<SystemTime> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(state_dir.join(TICKS_FILE)).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A read that began inside a line passes over that line's end.
    let whole = if from > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    whole
        .lines()
        .rev()
        .find_map(Recorded::parse)
        .map(|recorded| recorded.at)
}

/// Takes the trace's lock, waiting for a tick that holds it; the OS lets it go with the file.
fn lock(state_dir: &Path) -> Result<File, String> {
    let path = state_dir.join(TICKS_LOCK);
    let failed = |source: std::io::Error| format!("cannot lock {}: {source}", path.display());
    let file = std::fs::create_dir_all(state_dir)
        .and_then(|()| {
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
        })
        .map_err(failed)?;
    file.lock().map_err(failed)?;
    Ok(file)
}

/// The trace's text; empty when there is none yet. Bytes that are no UTF-8 (a line cut in the
/// middle of a character by a crash) read as the replacement character, so that line alone does
/// not read back and the rest of the trace does.
fn read_text(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(format!("cannot read {}: {source}", path.display())),
    }
}

/// Writes `line` after the lines `before` keeps of `text`, the file at `path`: written whole and
/// renamed into place when a line is dropped, appended in one write otherwise (after a newline
/// when the file does not end with one).
fn write(path: &Path, text: &str, before: &Before<'_>, line: &str) -> Result<(), String> {
    let failed = |source: std::io::Error| format!("cannot write {}: {source}", path.display());
    if before.dropped {
        let mut whole: String = before.kept.iter().map(|kept| format!("{kept}\n")).collect();
        whole.push_str(line);
        whole.push('\n');
        let staging = path.with_file_name(format!("{TICKS_FILE}.{}.new", std::process::id()));
        return std::fs::write(&staging, whole)
            .and_then(|()| std::fs::rename(&staging, path))
            .map_err(|source| {
                let _ = std::fs::remove_file(&staging);
                failed(source)
            });
    }
    let mut appended = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        appended.push('\n');
    }
    appended.push_str(line);
    appended.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(appended.as_bytes()))
        .map_err(failed)
}

#[cfg(test)]
mod tests;
