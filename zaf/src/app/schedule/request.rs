//! Asking the running scheduler for a tick (docs/zaf-actions.md §5.1, R22): the file
//! `.zaf/tick.request` ([`REQUEST_FILE`]).
//!
//! An event only asks; the tick alone moves tickets. A verb that changed something the placement
//! reads (a phase that ended, a ruling, a signature, a question, a claim, a merge) appends one line
//! naming who asked and why ([`ask`]): `2026-11-12T17:15:03Z phase-end implement #7`. It writes
//! only while a scheduler runs ([`super::lock::running`]); with none up nothing is written, the
//! verb works by itself (R10), and the scheduler reads the change from the facts when it next
//! starts. A line that lands as a scheduler stops is harmless: the next one ticks when it starts.
//!
//! The scheduler looks at the file's size and modified time each second ([`Requests::look`], the
//! pattern of `app/hub/watch.rs`'s `Mark`) and reads only the complete lines appended since its
//! last look, so a line a writer is still writing waits for the next look. Each writer appends
//! its one line under the file's `flock`; the scheduler empties the file under the same lock once
//! it has grown past [`EMPTY_PAST`], after reading what is left, so no line is lost to the cut,
//! and empties it when it starts, naming what was there in its first tick ([`Requests::start`]).
//! A file that cannot be emptied loses nothing either: what was read is served, the failure is
//! named, and the next look goes on from where this one stopped. A file that cannot be written
//! is the asker's error, naming the scheduler it could not ask ([`NotAsked`]): no tick was asked
//! for. A verb whose own change stands (a ruling, a signature) keeps its exit and says so in one
//! line, ending with the scheduler's timer, which picks the change up at its next tick
//! ([`NotAsked::left_to_the_timer`]).
//! Nothing here decides anything: a request is a reason for a tick and the words its block
//! prints.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::lock::{self, Holding};
use crate::ports::Holder;

/// The request file, under the state folder.
pub const REQUEST_FILE: &str = "tick.request";

/// The size past which the scheduler empties the request file once it has read it.
pub const EMPTY_PAST: u64 = 16 * 1024;

/// One request: when it was made and who asked, and why, in a few words (`ruling #45`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// When it was asked.
    pub at: SystemTime,
    /// Who asked and why.
    pub who: String,
}

impl Request {
    /// The request as the file holds it: the time (RFC 3339, UTC), a space, who asked, a newline.
    /// A newline in `who` becomes a space, so a request is always one line.
    pub fn line(&self) -> String {
        format!(
            "{} {}\n",
            humantime::format_rfc3339_seconds(self.at),
            self.who.replace(['\n', '\r'], " ").trim()
        )
    }

    /// One line read back; none when it holds no time or no one who asked.
    pub fn parse(line: &str) -> Option<Self> {
        let (at, who) = line.trim().split_once(' ')?;
        let who = who.trim();
        if who.is_empty() {
            return None;
        }
        Some(Self {
            at: humantime::parse_rfc3339(at).ok()?,
            who: who.to_owned(),
        })
    }
}

/// Why a running scheduler was not asked for a tick: the request file could not be written. No
/// tick was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAsked {
    /// The scheduler that runs, which was not asked (boxed: a line is large beside the rest).
    pub holding: Box<Holding>,
    /// Why the request could not be written.
    pub why: String,
}

impl NotAsked {
    /// What a verb whose own work is done says when it could not ask for a tick for `who` (its
    /// change still stands, and its exit is its own): `the running scheduler (<where>) was not
    /// asked for a tick for <who>: <why>; its timer picks the change up at its next tick`.
    pub fn left_to_the_timer(&self, who: &str) -> String {
        format!(
            "the running scheduler ({}) was not asked for a tick for {who}: {}; its timer picks \
             the change up at its next tick",
            self.holding.whereabouts(),
            self.why
        )
    }
}

impl std::fmt::Display for NotAsked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the running scheduler ({}) was not asked for a tick: {}; no tick was asked for",
            self.holding.whereabouts(),
            self.why
        )
    }
}

/// Asks the scheduler that runs for the project of `state_dir` for a tick, as `who` at `now`: one
/// line appended to the request file. Gives the scheduler's line when one runs and the request was
/// written; none when no scheduler runs, and then nothing is written. A file that cannot be
/// written is the error, naming the scheduler that runs.
pub fn ask(
    state_dir: &Path,
    who: &str,
    now: SystemTime,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Option<Holding>, NotAsked> {
    let Some(holding) = lock::running(state_dir, alive) else {
        return Ok(None);
    };
    let request = Request {
        at: now,
        who: who.to_owned(),
    };
    match append(state_dir, &request) {
        Ok(()) => Ok(Some(holding)),
        Err(why) => Err(NotAsked {
            holding: Box::new(holding),
            why,
        }),
    }
}

/// Appends `request` to the request file under `state_dir`, in one write under its `flock`.
pub fn append(state_dir: &Path, request: &Request) -> Result<(), String> {
    let path = state_dir.join(REQUEST_FILE);
    let failed = |source: std::io::Error| format!("cannot write {}: {source}", path.display());
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(failed)?;
    file.lock().map_err(failed)?;
    file.write_all(request.line().as_bytes()).map_err(failed)
}

/// What the request file looked like at a look: its size and when it last changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Mark {
    len: u64,
    modified: Option<SystemTime>,
}

impl Mark {
    /// The mark of the file at `path`; a missing file is the empty mark.
    fn read(path: &Path) -> Self {
        std::fs::metadata(path).map_or_else(
            |_| Self::default(),
            |meta| Self {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
        )
    }
}

/// What one look at the request file found: the requests appended since the last look, and why the
/// file could not be emptied when it was due to be and was not. The requests read before that
/// failure are kept: the next look goes on after them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looked {
    /// The requests read, oldest first.
    pub requests: Vec<Request>,
    /// Why the file, past [`EMPTY_PAST`], could not be emptied at this look.
    pub not_emptied: Option<String>,
}

/// The request file as the scheduler reads it: how far it has read and what the file looked like
/// then.
#[derive(Debug)]
pub struct Requests {
    path: PathBuf,
    /// The bytes read so far, up to the end of the last complete line.
    read: u64,
    mark: Mark,
}

impl Requests {
    /// The file for a scheduler that starts: every request already in it is given back, to be
    /// named in the first tick (a scheduler was asked and stopped before it read them), and the
    /// file is emptied. A file that cannot be read or emptied is the error, with nothing read.
    pub fn start(state_dir: &Path) -> Result<(Self, Vec<Request>), String> {
        let mut requests = Self {
            path: state_dir.join(REQUEST_FILE),
            read: 0,
            mark: Mark::default(),
        };
        let (pending, failed) = requests.empty();
        if let Some(why) = failed {
            return Err(why);
        }
        Ok((requests, pending))
    }

    /// The requests appended since the last look, each complete line once, oldest first. The
    /// file's size and modified time are looked at first, and nothing is read when neither moved.
    /// Past [`EMPTY_PAST`] the file is emptied, after the rest of it is read, under its lock; a
    /// file that cannot be emptied keeps what was read in the look, says why in
    /// [`Looked::not_emptied`], and is tried again at the next look that finds it grown. A line
    /// that does not read is passed over. A file that cannot be read is the error.
    pub fn look(&mut self) -> Result<Looked, String> {
        let mark = Mark::read(&self.path);
        if mark == self.mark {
            return Ok(Looked::default());
        }
        self.mark = mark;
        // Emptied or replaced by something else since the last look: read it from the start.
        if mark.len < self.read {
            self.read = 0;
        }
        let mut requests = match self.read_from_mark() {
            Ok(requests) => requests,
            Err(why) if why.kind() == ErrorKind::NotFound => {
                self.read = 0;
                return Ok(Looked::default());
            }
            Err(why) => return Err(self.failed(&why)),
        };
        let mut not_emptied = None;
        if self.read >= EMPTY_PAST {
            let (rest, failed) = self.empty();
            requests.extend(rest);
            not_emptied = failed;
        }
        Ok(Looked {
            requests,
            not_emptied,
        })
    }

    /// Reads the complete lines after what was read, and moves past them.
    fn read_from_mark(&mut self) -> std::io::Result<Vec<Request>> {
        let mut file = File::open(&self.path)?;
        self.read_lines(&mut file)
    }

    /// Reads the complete lines of `file` after what was read, and moves past them.
    fn read_lines(&mut self, file: &mut File) -> std::io::Result<Vec<Request>> {
        file.seek(SeekFrom::Start(self.read))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let complete = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |last| last + 1);
        self.read += u64::try_from(complete).unwrap_or(u64::MAX);
        Ok(String::from_utf8_lossy(&bytes[..complete])
            .lines()
            .filter_map(Request::parse)
            .collect())
    }

    /// Under the file's lock, reads what is left after what was read and empties the file; gives
    /// what it read, and why it could not do all of it. What it read is given whatever failed
    /// after, and how far it read is kept, so a file that could not be emptied is read on from
    /// there. No file is nothing to empty. The mark of the emptied file is taken before the lock
    /// is let go, so no request can land between the two and be passed over.
    fn empty(&mut self) -> (Vec<Request>, Option<String>) {
        let mut file = match OpenOptions::new().read(true).write(true).open(&self.path) {
            Ok(file) => file,
            Err(why) if why.kind() == ErrorKind::NotFound => {
                self.read = 0;
                return (Vec::new(), None);
            }
            Err(why) => return (Vec::new(), Some(self.not_emptied(&why))),
        };
        if let Err(why) = file.lock() {
            return (Vec::new(), Some(self.not_emptied(&why)));
        }
        let requests = match self.read_lines(&mut file) {
            Ok(requests) => requests,
            Err(why) => return (Vec::new(), Some(self.failed(&why))),
        };
        if let Err(why) = file.set_len(0) {
            return (requests, Some(self.not_emptied(&why)));
        }
        self.read = 0;
        // Still under the lock: a request appended once it is let go moves the file past this
        // mark, so the next look reads it. No mark when it cannot be read: the next look reads.
        self.mark = file.metadata().map_or_else(
            |_| Mark::default(),
            |meta| Mark {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
        );
        (requests, None)
    }

    /// Drops what this scheduler has read from the file and keeps what it has not, under the
    /// file's lock: done before it stops or starts again as a new binary, so the next start names
    /// only the requests no tick has served. No file is nothing to drop. The next look reads the
    /// file from its start, so a restart that failed still serves what was kept.
    pub fn let_go(&mut self) -> Result<(), String> {
        let mut file = match OpenOptions::new().read(true).write(true).open(&self.path) {
            Ok(file) => file,
            Err(why) if why.kind() == ErrorKind::NotFound => {
                self.read = 0;
                return Ok(());
            }
            Err(why) => return Err(self.failed(&why)),
        };
        file.lock().map_err(|why| self.failed(&why))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|why| self.failed(&why))?;
        let read = usize::try_from(self.read)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let rest = &bytes[read..];
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| file.write_all(rest))
            .map_err(|why| self.failed(&why))?;
        self.read = 0;
        // No mark: the next look reads what was kept, as a restart that failed must serve it.
        self.mark = Mark::default();
        Ok(())
    }

    fn failed(&self, why: &std::io::Error) -> String {
        format!("cannot read {}: {why}", self.path.display())
    }

    fn not_emptied(&self, why: &std::io::Error) -> String {
        format!(
            "cannot empty {}: {why}; what was read is served, and the next look reads on",
            self.path.display()
        )
    }
}

#[cfg(test)]
mod tests;
