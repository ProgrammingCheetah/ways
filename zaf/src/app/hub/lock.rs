//! The hub's machine-wide lock (docs/zaf-actions.md §5, door ruling on #1): one serving hub
//! (`zaf hub --port <n>`) at a time on a machine.
//!
//! The lock is the file [`LOCK_FILE`] in the zaf home. It names the serving process, as a
//! [`Holder`] (its pid and start time, so a pid the OS has handed to another process reads as
//! dead), and the port it serves. A second start finds it held by a live process and is refused
//! with the first hub's port; a lock whose process is dead is taken over. Starts take turns under
//! an OS lock on [`GUARD_FILE`], and the lock is written whole and renamed into place, so it is
//! never seen empty and never removed by a start that read an older one. The terminal frame
//! (`zaf hub` with no port) never reads or writes it.
//!
//! Whether a process lives is the caller's to say, as in the spawner, so this module reads the
//! process table through no adapter.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::ports::Holder;

/// The lock's file name in the zaf home.
pub const LOCK_FILE: &str = "hub.lock";

/// The file every start locks while it reads and replaces [`LOCK_FILE`]; it holds nothing and
/// stays in the zaf home.
const GUARD_FILE: &str = "hub.lock.guard";

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
        // Every start reads and replaces the lock under the guard, so no two judge the same lock
        // stale and no start removes one another took a moment ago.
        let guard = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.join(GUARD_FILE))
            .map_err(io)?;
        guard.lock().map_err(io)?;
        match fs::read_to_string(&path) {
            Ok(text) => {
                if let Some(lock) = Self::parse(&text).filter(|lock| alive(&lock.holder())) {
                    return Err(LockError::Held(lock));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        // Written whole beside the lock, then renamed over it: a reader finds the old lock or the
        // new one, never an empty file it would take for stale.
        let temp = home.join(format!("{LOCK_FILE}.{}.tmp", std::process::id()));
        File::create(&temp)
            .and_then(|mut file| file.write_all(mine.render().as_bytes()))
            .and_then(|()| fs::rename(&temp, &path))
            .map_err(|error| {
                let _ = fs::remove_file(&temp);
                io(error)
            })?;
        Ok(mine)
        // The guard is released when `guard` drops, after the rename.
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
