//! The scheduler's lock (docs/zaf-actions.md §5.1, R17): one scheduler at a time for a project.
//!
//! The lock is the file [`LOCK_FILE`] in the folder it is handed (the project's state folder,
//! `.zaf/`, today; the zaf home if the scheduler becomes one per machine, which is one call site).
//! The scheduler holds it with `flock` for its whole life ([`SchedulerLock::take`]), and the OS
//! lets it go when the process ends, however it ends. The file holds one line, [`Holding`], that
//! names the holder: its pid and the start the OS gave it, since when it holds the lock, the zaf
//! version of its image and the `--version` it runs. A second scheduler cannot take the lock and
//! is told where the first is ([`LockError::Held`]).
//!
//! The line is only ever read to say where the holder is, never to decide that the lock is held:
//! the `flock` decides that. A scheduler that starts takes the lock whatever the line says, so a
//! line left by a scheduler that died is taken over ([`Taken::TookOver`]). A verb that only wants
//! to know whether a scheduler runs ([`running`]) asks the `flock` too, without waiting: it tries a
//! shared lock on the file, and a lock it could take (and lets go at once) means no scheduler holds
//! it, whatever the line says; one it could not take is held, and the line then says by whom. So a
//! line that names a live process nobody's lock stands behind (an older binary that holds no lock,
//! a pid handed to another process after a reboot) is no scheduler. A reader never waits for the
//! lock, so it can never stop a scheduler from starting, and a start tries its lock a few times, so
//! a reader's moment on it never stops one either. A restart `exec`s the same process, whose pid
//! and start stay, so the line stays true; the new image takes the lock again and keeps the line's
//! `since` ([`Taken::Restarted`]). The line is rewritten in place, the new line over the old and
//! then cut to its length, so a reader finds a whole line at the head of the file at every moment.
//! A scheduler that stops clears the line ([`SchedulerLock::release`]).
//!
//! Whether a process lives is the caller's to say, so this module reads no process table.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::ports::Holder;

/// The lock's file name in the folder it is kept in.
pub const LOCK_FILE: &str = "scheduler.lock";

/// How many times a start tries a lock a reader may hold for a moment, and reads the line of a lock
/// it found held while its holder may be writing it; and how long it waits between two tries.
const READS: usize = 10;
const READ_PAUSE: Duration = Duration::from_millis(20);

/// How many times a reader looks at a held lock whose line does not read yet, or names a process
/// that is gone, while its holder may be writing it: a start writes its line just after it takes
/// the lock. The same pause lies between two looks.
const LOOKS: usize = 5;

/// The line of the lock: who holds it. Written as one JSON object on one line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Holding {
    /// The scheduler's process id.
    pub pid: u32,
    /// When the process started, as the OS reports it; compared, never parsed.
    pub start: String,
    /// When this scheduler took the lock, RFC 3339 (UTC); a restart keeps it.
    pub since: String,
    /// The zaf version of the image that holds it now.
    pub zaf: String,
    /// The milestone it ticks for, its `--version`.
    pub version: String,
}

impl Holding {
    /// The process this line names.
    pub fn holder(&self) -> Holder {
        Holder {
            pid: self.pid,
            start: self.start.clone(),
        }
    }

    /// The line as the file holds it: one JSON object and a newline.
    pub fn line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }

    /// A line read back; none when it is not one.
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text.lines().next()?.trim()).ok()
    }

    /// Where it is, in words: `pid 4242, since 2026-11-12T13:02:11Z (zaf 0.3.1, --version 0.4.0)`.
    pub fn whereabouts(&self) -> String {
        format!(
            "pid {}, since {} (zaf {}, --version {})",
            self.pid, self.since, self.zaf, self.version
        )
    }
}

/// The scheduler that runs for the lock in `dir`, when one does: a process holds the file's `flock`
/// (a shared lock tried without waiting cannot be taken), and the line names a process that
/// `alive` says still runs. No file, a lock nobody holds (whatever its line says), or a held lock
/// whose line does not read or names a process that is gone, read again a few times while its
/// holder may be writing it, are none. A lock that cannot be tried at all leaves the line alone to
/// say. It never waits for the lock, and holds it only for the moment of the try.
pub fn running(dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Option<Holding> {
    let mut file = File::open(dir.join(LOCK_FILE)).ok()?;
    let live = |file: &mut File| line_of(file).filter(|holding| alive(&holding.holder()));
    for look in 1..=LOOKS {
        if look > 1 {
            std::thread::sleep(READ_PAUSE);
        }
        match file.try_lock_shared() {
            Ok(()) => {
                // Free: no scheduler holds it, whatever its line names.
                let _ = file.unlock();
                return None;
            }
            Err(TryLockError::WouldBlock) => {
                if let Some(holding) = live(&mut file) {
                    return Some(holding);
                }
            }
            Err(TryLockError::Error(_)) => return live(&mut file),
        }
    }
    None
}

/// The line at the head of the open lock file; none when it does not read.
fn line_of(file: &mut File) -> Option<Holding> {
    let mut text = String::new();
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_to_string(&mut text).ok()?;
    Holding::parse(&text)
}

/// How a start got the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// It was free, with no line in it.
    Fresh,
    /// It was this same process's, before it started again as a new image: `from` was the zaf
    /// version of the old one.
    Restarted {
        /// The old image's zaf version.
        from: String,
    },
    /// It was free, its line naming a scheduler that no longer holds it.
    TookOver(Holding),
}

/// Why a start did not get the lock.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another scheduler holds it; its line, when it reads.
    #[error("a scheduler already runs here: {}", .0.as_ref().map_or_else(|| "its lock names no holder yet".to_owned(), Holding::whereabouts))]
    Held(Option<Holding>),
    /// The lock file could not be opened, locked, read or written.
    #[error("cannot use the scheduler lock {}: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
}

/// The lock a running scheduler holds: dropped, the OS lets it go; released, its line is cleared
/// first.
#[derive(Debug)]
pub struct SchedulerLock {
    file: File,
    holding: Holding,
}

impl SchedulerLock {
    /// Takes the lock in `dir` for `me`, which runs zaf `zaf` for the milestone `version`, at
    /// `now`, and writes its line. A lock another process holds is [`LockError::Held`], with that
    /// process's line once it reads; a free one is taken, whatever its line says ([`Taken`]).
    pub fn take(
        dir: &Path,
        me: &Holder,
        zaf: &str,
        version: &str,
        now: SystemTime,
    ) -> Result<(Self, Taken), LockError> {
        let path = dir.join(LOCK_FILE);
        let io = |source: io::Error| LockError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(dir).map_err(io)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io)?;
        try_lock(&file).map_err(|error| match error {
            None => LockError::Held(held_line(&path)),
            Some(source) => io(source),
        })?;
        let mut before = String::new();
        file.read_to_string(&mut before).map_err(io)?;
        let now = humantime::format_rfc3339_seconds(now).to_string();
        let (taken, since) = match Holding::parse(&before) {
            Some(before) if before.pid == me.pid && before.start == me.start => {
                (Taken::Restarted { from: before.zaf }, before.since)
            }
            Some(before) => (Taken::TookOver(before), now),
            None => (Taken::Fresh, now),
        };
        let holding = Holding {
            pid: me.pid,
            start: me.start.clone(),
            since,
            zaf: zaf.to_owned(),
            version: version.to_owned(),
        };
        rewrite(&mut file, &holding.line()).map_err(io)?;
        Ok((Self { file, holding }, taken))
    }

    /// The line this scheduler wrote.
    pub fn holding(&self) -> &Holding {
        &self.holding
    }

    /// Gives the lock up: the line is cleared, so no reader takes the stopped scheduler for a
    /// running one, and the file is let go. A line that cannot be cleared is left; its process is
    /// gone soon after, and a reader then reads it as gone.
    pub fn release(mut self) {
        let _ = rewrite(&mut self.file, "");
    }
}

/// Replaces the whole of the open lock file with `text`: written over the old line from the start,
/// then the file cut to its length. A reader in between finds the old line, or the new one followed
/// by the old one's tail, which the line's own newline ends: a whole line at the head of the file
/// at every moment, never an empty file.
fn rewrite(file: &mut File, text: &str) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(text.as_bytes())?;
    file.set_len(u64::try_from(text.len()).unwrap_or(u64::MAX))?;
    file.flush()
}

/// Locks `file`, trying a few times: a reader holds a shared lock on it for the moment it looks
/// ([`running`]), and a child another thread of this process forks holds every open file until it
/// `exec`s, so a lock just let go can read as held for that instant. `Err(None)` when it stays
/// held, `Err(Some(error))` when it cannot be taken at all.
fn try_lock(file: &File) -> Result<(), Option<io::Error>> {
    for _ in 1..READS {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => std::thread::sleep(READ_PAUSE),
            Err(TryLockError::Error(source)) => return Err(Some(source)),
        }
    }
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(None),
        Err(TryLockError::Error(source)) => Err(Some(source)),
    }
}

/// The line of a lock another process holds, read a few times while its holder may still be
/// writing it; none when it never reads.
fn held_line(path: &Path) -> Option<Holding> {
    for _ in 0..READS {
        if let Some(holding) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Holding::parse(&text))
        {
            return Some(holding);
        }
        std::thread::sleep(READ_PAUSE);
    }
    None
}

#[cfg(test)]
mod tests;
