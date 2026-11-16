//! The hub's machine-wide lock (docs/zaf-actions.md §5, door ruling on #1): one serving hub
//! (`zaf hub --port <n>`) at a time on a machine.
//!
//! The lock is the file [`LOCK_FILE`] in the zaf home. It names the serving process, as a
//! [`Holder`] (its pid and start time, so a pid the OS has handed to another process reads as
//! dead), and the port it serves. A second start finds it held by a live process and is refused
//! with the first hub's port; a lock whose process is dead is taken over. The terminal frame
//! (`zaf hub` with no port) never reads or writes it.
//!
//! Whether a process lives is the caller's to say, as in the spawner, so this module reads the
//! process table through no adapter.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::ports::Holder;

/// The lock's file name in the zaf home.
pub const LOCK_FILE: &str = "hub.lock";

/// How many times a start retries after it loses a race for the file.
const ATTEMPTS: usize = 5;

/// A serving hub: its process and the port it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubLock {
    /// The serving process's id.
    pub pid: u32,
    /// The port its page is served on.
    pub port: u16,
    /// When the process started, as the OS reports it.
    start: String,
}

/// Why a start did not get the lock.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// A live hub holds it.
    #[error("a hub is already serving on port {}", .0.port)]
    Held(HubLock),
    /// The lock file could not be read or written.
    #[error("cannot use the hub lock {}: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
}

impl HubLock {
    /// The holder this lock names.
    pub fn holder(&self) -> Holder {
        Holder {
            pid: self.pid,
            start: self.start.clone(),
        }
    }

    /// The lock a live hub holds in `home`, or none: no file, a file that is not a lock, or a
    /// process that no longer runs.
    pub fn held(home: &Path, alive: &dyn Fn(&Holder) -> bool) -> Option<Self> {
        let lock = Self::read(&home.join(LOCK_FILE))?;
        alive(&lock.holder()).then_some(lock)
    }

    /// Takes the lock for `me`, which serves `port`. A lock held by a live process is
    /// [`LockError::Held`]; one whose process is dead, or that is not a lock, is taken over.
    pub fn take(
        home: &Path,
        me: &Holder,
        port: u16,
        alive: &dyn Fn(&Holder) -> bool,
    ) -> Result<Self, LockError> {
        let path = home.join(LOCK_FILE);
        let io = |source: io::Error| LockError::Io {
            path: path.clone(),
            source,
        };
        fs::create_dir_all(home).map_err(io)?;
        let mine = Self {
            pid: me.pid,
            port,
            start: me.start.clone(),
        };
        for _ in 0..ATTEMPTS {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(mine.render().as_bytes()).map_err(io)?;
                    return Ok(mine);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io(error)),
            }
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                // Gone since the open: try to create it again.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io(error)),
            };
            if let Some(lock) = Self::parse(&text).filter(|lock| alive(&lock.holder())) {
                return Err(LockError::Held(lock));
            }
            // A stale lock goes only if it is still the one read, so a start that took it over
            // a moment ago keeps its own.
            if fs::read_to_string(&path).is_ok_and(|now| now == text) {
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io(error)),
                }
            }
        }
        Err(io(io::Error::other("the lock kept changing hands")))
    }

    /// Gives the lock up, when it is still this process's. A lock that has since passed to
    /// another process, or is already gone, is left alone.
    pub fn release(&self, home: &Path) {
        let path = home.join(LOCK_FILE);
        if Self::read(&path).is_some_and(|lock| lock == *self) {
            let _ = fs::remove_file(path);
        }
    }

    fn render(&self) -> String {
        format!("{}\n{}\n", self.holder(), self.port)
    }

    fn read(path: &Path) -> Option<Self> {
        Self::parse(&fs::read_to_string(path).ok()?)
    }

    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let holder = Holder::parse(lines.next()?)?;
        let port = lines.next()?.trim().parse().ok()?;
        Some(Self {
            pid: holder.pid,
            port,
            start: holder.start,
        })
    }
}

/// The lock a start of `zaf hub` needs: none for the terminal frame (no `port`), which reads
/// nothing; for a served hub, the lock taken for `me`.
pub fn claim(
    home: &Path,
    port: Option<u16>,
    me: &Holder,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Option<HubLock>, LockError> {
    port.map(|port| HubLock::take(home, me, port, alive))
        .transpose()
}

#[cfg(test)]
mod tests;
