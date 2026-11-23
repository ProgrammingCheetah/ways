//! The scheduler is never stale (docs/zaf-actions.md §5.1, R17): it compares itself with the
//! binary it was started from and starts again as the new one.
//!
//! [`Binary::this`] remembers the path this process was started from and the identity of the
//! image it runs ([`Identity`]: the device, the inode, the size and the modified time), read from
//! the running image itself (`/proc/self/exe` where there is one), so a file replaced before the
//! first look is still seen as new. [`Binary::look`] stats the path again; [`next`] decides from
//! the two, purely:
//!
//! - the same file as the image, or no file at the path for now (a rename in flight): nothing;
//! - the file a restart was already refused for: nothing, until the file changes again;
//! - a file not seen at the last look: wait one more look, so a binary still being written is not
//!   run half way;
//! - the same new file at two looks in a row: ready, and [`version_of`] runs `<path> --version`.
//!
//! The scheduler looks only at the top of its loop, between two ticks, so it never restarts in
//! the middle of one, nor while a planning or challenger launch runs (those are joined inside the
//! tick). A new binary that answers `--version` with exit 0 is `exec`ed with the same arguments
//! (by the CLI, `adapters::process::exec`, which keeps the stop signals held back): the same
//! process, so its pid and its start stay, and the lock's line stays true. One that does not is
//! refused ([`Binary::refuse`]) and the old image goes on.
//!
//! Every place that starts this zaf binary again (the restart, a tick's `zaf spawn phase`, the
//! launcher's `zaf do` and its ask for a tick) starts it from one path, [`path_to_start`]: the path
//! the process was started from, never the ` (deleted)` name the system gives a running image whose
//! file was replaced, which no file has.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::domain::without_deleted;

/// How long a new binary gets to answer `--version`.
pub const VERSION_WAIT: Duration = Duration::from_secs(10);

/// What tells one file from another at a path: a rename over it changes the inode, a write in
/// place the size or the modified time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The device the file is on.
    pub device: u64,
    /// Its inode.
    pub inode: u64,
    /// Its size in bytes.
    pub size: u64,
    /// When it was last written, when the system says.
    pub modified: Option<SystemTime>,
}

impl Identity {
    /// The identity of the file `metadata` describes.
    pub fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            modified: metadata.modified().ok(),
        }
    }

    /// The identity of the file at `path`, followed through links; none when there is none.
    pub fn at(path: &Path) -> Option<Self> {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| Self::of(&metadata))
    }
}

/// What one look at the binary found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Nothing to do: the file is the running image, the one a restart was refused for, or not
    /// there for now.
    Same,
    /// A new file, seen once: wait for the next look.
    Settling,
    /// A new file, the same at two looks in a row: ready to try.
    Ready(Identity),
}

/// Decides from what the path holds `now` (none when there is no file) and what it held at the
/// last look (`before`), against the running image's identity (`running`) and the file a restart
/// was refused for (`refused`).
pub fn next(
    running: Identity,
    refused: Option<Identity>,
    before: Option<Identity>,
    now: Option<Identity>,
) -> Look {
    match now {
        None => Look::Same,
        Some(now) if now == running || Some(now) == refused => Look::Same,
        Some(now) if Some(now) == before => Look::Ready(now),
        Some(_) => Look::Settling,
    }
}

/// The path to start this zaf binary with (see the module above): the path this process was started
/// from, its first argument when that names a file by a path (`/usr/local/bin/zaf`, `./zaf`, read
/// against the folder it was started in, which zaf never leaves), so a link that names the
/// installed binary is followed to the file it names now; else the running image's path as the
/// system reports it (`std::env::current_exe`), with the ` (deleted)` it adds once the file was
/// replaced dropped. A binary replaced on disk is started as the new file at the same path. An
/// error says why there is no path.
pub fn path_to_start() -> Result<PathBuf, String> {
    start_path(
        std::env::args_os().next(),
        std::env::current_dir().ok(),
        std::env::current_exe(),
    )
}

/// [`path_to_start`] from what it reads: the process's first argument, the folder it runs in, and
/// the running image's path as the system reports it.
fn start_path(
    first: Option<OsString>,
    cwd: Option<PathBuf>,
    exe: std::io::Result<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(first) = first.filter(|first| first.as_bytes().contains(&b'/')) {
        let first = PathBuf::from(first);
        let started = if first.is_absolute() {
            Some(first)
        } else {
            cwd.map(|cwd| cwd.join(first))
        };
        if let Some(started) = started.filter(|started| started.is_file()) {
            return Ok(started);
        }
    }
    let exe = exe.map_err(|error| format!("cannot find the zaf binary: {error}"))?;
    Ok(without_deleted(exe))
}

/// The binary this scheduler was started from, and what it has seen of it since.
#[derive(Debug, Clone)]
pub struct Binary {
    path: PathBuf,
    running: Identity,
    refused: Option<Identity>,
    before: Option<Identity>,
}

impl Binary {
    /// The binary this process runs: the path it was started from ([`path_to_start`]) and the
    /// identity of the image it runs. An error says why the scheduler cannot watch its binary.
    pub fn this() -> Result<Self, String> {
        let path = path_to_start()?;
        let image = Path::new("/proc/self/exe");
        let running = if image.exists() {
            Identity::at(image)
        } else {
            Identity::at(&path)
        }
        .ok_or_else(|| format!("cannot read {}", path.display()))?;
        Ok(Self::new(path, running))
    }

    /// A binary at `path` whose running image is `running`, nothing seen yet.
    pub fn new(path: PathBuf, running: Identity) -> Self {
        Self {
            path,
            running,
            refused: None,
            before: None,
        }
    }

    /// The path it was started from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Looks at the path once (see [`next`]); the file at the path going back to the running
    /// image forgets a refusal.
    pub fn look(&mut self) -> Look {
        let now = Identity::at(&self.path);
        if now == Some(self.running) {
            self.refused = None;
        }
        let look = next(self.running, self.refused, self.before, now);
        self.before = now;
        look
    }

    /// Refuses the file of `identity`: it is not tried again until the path holds another.
    pub fn refuse(&mut self, identity: Identity) {
        self.refused = Some(identity);
    }
}

/// Why a new binary was not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    /// The file is still open for writing somewhere (`ETXTBSY`): try at a later look.
    #[error("it is still being written")]
    Busy,
    /// It does not run, or does not answer: say so and keep the old image.
    #[error("{0}")]
    Refused(String),
}

/// Runs `<path> --version` and gives what it printed, its first line (`zaf 0.3.2`), when it exits
/// 0 within [`VERSION_WAIT`]. A file that cannot be run, a non-zero exit, a signal, no answer in
/// time or an output still held open then is [`VersionError::Refused`], in words; a file still open for writing is
/// [`VersionError::Busy`]. It runs in a process group of its own, so a Ctrl-C meant for the
/// scheduler does not refuse a binary that is fine.
pub fn version_of(path: &Path) -> Result<String, VersionError> {
    let child = match Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => {
            return Err(VersionError::Busy);
        }
        Err(error) => return Err(VersionError::Refused(format!("it cannot be run: {error}"))),
    };
    version_within(child, VERSION_WAIT)
}

/// [`version_of`] for a `child` started with its output piped, given `wait` to answer. Its output
/// is read on a thread of its own and waited for no longer than the child is: a descendant that
/// keeps the output open after the child ended never holds the scheduler up.
fn version_within(mut child: Child, wait: Duration) -> Result<String, VersionError> {
    let (send, read) = std::sync::mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
            let _ = send.send(out);
        });
    }
    let deadline = Instant::now() + wait;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(VersionError::Refused(format!(
                    "it did not answer --version within {} s",
                    wait.as_secs()
                )));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(VersionError::Refused(format!("cannot wait on it: {error}")));
            }
        }
    };
    if !status.success() {
        let how = status
            .code()
            .map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
        return Err(VersionError::Refused(format!(
            "its --version ended with {how}"
        )));
    }
    let Ok(out) = read.recv_timeout(deadline.saturating_duration_since(Instant::now())) else {
        return Err(VersionError::Refused(format!(
            "its --version output did not end within {} s",
            wait.as_secs()
        )));
    };
    let version = out.lines().next().unwrap_or_default().trim().to_owned();
    if version.is_empty() {
        return Err(VersionError::Refused(
            "its --version printed nothing".to_owned(),
        ));
    }
    Ok(version)
}

#[cfg(test)]
mod tests;
