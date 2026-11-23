//! The verb runner over a child process (I3): each tool call runs the zaf binary on disk.
//!
//! The child is the binary at its path now ([`without_deleted`] of the running image's path,
//! read at each call), so an update takes effect on the next call. It runs in
//! [`directory`]'s directory, gets the call's stdin, and has the server's environment less the
//! two nesting markers ([`DEPTH_VAR`], [`CHAIN_VAR`]): a call is a new top-level run, never a step
//! of the action that started the server. It runs in a process group of its own, so a cancel can
//! take its descendants down with it.
//!
//! The run is synchronous (the server runs it on a blocking task): stdout and stderr are read on
//! threads of their own, and the calling thread polls the child, raises a [`Tick`] each interval
//! and watches the [`Cancel`] flag. A run it could not start, one a signal ended and one a cancel
//! stopped are outcomes with exit 2 and their reason on `stderr`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::{Cancel, Tick, without_deleted};
use crate::ports::{VerbOutcome, VerbRun, VerbRunner};

/// The environment variable that holds an action's nesting depth; a call never inherits it.
const DEPTH_VAR: &str = "ZAF_DEPTH";
/// The environment variable that holds the chain of actions above this run; a call never
/// inherits it.
const CHAIN_VAR: &str = "ZAF_ACTION_CHAIN";
/// The variable that names the project's root.
const ROOT_VAR: &str = "ZAF_ROOT";
/// The variable Claude Code sets to the project's directory.
const PROJECT_VAR: &str = "CLAUDE_PROJECT_DIR";

/// How often a running call reports a [`Tick`] in production.
pub const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// How often the run looks at the child and the cancel flag.
const POLL: Duration = Duration::from_millis(10);
/// How long the output of a child that ended is waited for, when something it left behind still
/// holds the pipe open.
const DRAIN: Duration = Duration::from_millis(500);
/// What the outcome says of a call that was cancelled.
const CANCELLED: &str = "cancelled";

/// Where the binary to run is found.
#[derive(Debug, Clone)]
enum Binary {
    /// The running image's own path, read at each call.
    Running,
    /// A fixed file: a stand-in for a test.
    At(PathBuf),
}

/// Runs verbs as child processes of the zaf binary on disk.
#[derive(Debug, Clone)]
pub struct VerbProcess {
    binary: Binary,
    interval: Duration,
}

impl Default for VerbProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl VerbProcess {
    /// A runner of the binary this process runs, ticking every [`TICK_INTERVAL`].
    pub fn new() -> Self {
        Self {
            binary: Binary::Running,
            interval: TICK_INTERVAL,
        }
    }

    /// The same runner ticking every `interval`.
    #[must_use]
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// A runner of the program at `path` in the binary's place (a test's stand-in child),
    /// ticking every `interval`.
    pub fn standing_in(path: impl Into<PathBuf>, interval: Duration) -> Self {
        Self {
            binary: Binary::At(path.into()),
            interval,
        }
    }

    /// The program to run now, or why there is none.
    fn program(&self) -> Result<PathBuf, String> {
        match &self.binary {
            Binary::At(path) => Ok(path.clone()),
            Binary::Running => std::env::current_exe()
                .map(without_deleted)
                .map_err(|error| format!("cannot find the zaf binary: {error}")),
        }
    }

    /// [`VerbRunner::run`] with the environment the child starts from given.
    fn run_in(
        &self,
        run: &VerbRun,
        environment: Vec<(OsString, OsString)>,
        tick: &dyn Fn(Tick),
        cancel: &Cancel,
    ) -> VerbOutcome {
        let program = match self.program() {
            Ok(program) => program,
            Err(reason) => return failed(reason),
        };
        let dir = directory(
            std::env::var_os(ROOT_VAR),
            std::env::var_os(PROJECT_VAR),
            || std::env::current_dir().ok(),
        );
        let mut command = Command::new(&program);
        command
            .args(run.argv.iter().skip(1))
            .env_clear()
            .envs(child_environment(environment))
            .stdin(if run.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own process group, so a cancel takes its descendants down too.
            .process_group(0);
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return failed(format!("cannot run {}: {error}", program.display())),
        };
        self.watch(child, run.stdin.clone(), tick, cancel)
    }

    /// Feeds, reads and polls `child` to its end.
    fn watch(
        &self,
        mut child: Child,
        stdin: Option<String>,
        tick: &dyn Fn(Tick),
        cancel: &Cancel,
    ) -> VerbOutcome {
        let group = child.id();
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // A child that ends before it reads everything is not an error of the call.
            thread::spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
        }
        let (stdout, stdout_done) = collect(child.stdout.take());
        let (stderr, stderr_done) = collect(child.stderr.take());
        let started = Instant::now();
        let mut last_tick = started;
        let (status, cancelled) = loop {
            if cancel.is_cancelled() {
                kill_group(group);
                break (child.wait(), true);
            }
            match child.try_wait() {
                Ok(Some(status)) => break (Ok(status), false),
                Ok(None) => {}
                Err(error) => {
                    kill_group(group);
                    let _ = child.wait();
                    return failed(format!("cannot wait on the verb: {error}"));
                }
            }
            if last_tick.elapsed() >= self.interval {
                last_tick = Instant::now();
                tick(Tick {
                    elapsed: started.elapsed(),
                    last_stderr: last_line(&text_of(&stderr)),
                });
            }
            thread::sleep(POLL);
        };
        // What a descendant of the child left holding a pipe open is not waited for.
        let _ = stdout_done.recv_timeout(DRAIN);
        let _ = stderr_done.recv_timeout(DRAIN);
        let mut outcome = VerbOutcome {
            stdout: text_of(&stdout),
            stderr: text_of(&stderr),
            exit: 2,
        };
        match status {
            _ if cancelled => note(&mut outcome.stderr, CANCELLED),
            Err(error) => note(
                &mut outcome.stderr,
                &format!("cannot wait on the verb: {error}"),
            ),
            Ok(status) => match status.code() {
                Some(code) => outcome.exit = code,
                None => note(&mut outcome.stderr, "the verb was ended by a signal"),
            },
        }
        outcome
    }
}

impl VerbRunner for VerbProcess {
    fn binary_stamp(&self) -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(self.program().ok()?).ok()?;
        Some(format!(
            "{}:{}:{}.{}:{}",
            meta.dev(),
            meta.ino(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.len()
        ))
    }

    fn run(&self, run: VerbRun, tick: &dyn Fn(Tick), cancel: &Cancel) -> VerbOutcome {
        self.run_in(&run, std::env::vars_os().collect(), tick, cancel)
    }
}

/// The directory a verb runs in: `ZAF_ROOT` when it is set (and not empty), else
/// `CLAUDE_PROJECT_DIR`, else the server's own (`own`, asked only then; none when the server's
/// own cannot be read, and the child then starts where the server is).
pub fn directory(
    zaf_root: Option<OsString>,
    project_dir: Option<OsString>,
    own: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    zaf_root
        .into_iter()
        .chain(project_dir)
        .find(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(own)
}

/// `environment` without the nesting markers: a call starts a new top-level run.
fn child_environment(
    environment: Vec<(OsString, OsString)>,
) -> impl Iterator<Item = (OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(name, _)| name != DEPTH_VAR && name != CHAIN_VAR)
}

/// An outcome for a run that never ran.
fn failed(reason: String) -> VerbOutcome {
    VerbOutcome {
        stdout: String::new(),
        stderr: reason,
        exit: 2,
    }
}

/// Adds `line` to `text` on a line of its own.
fn note(text: &mut String, line: &str) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
}

/// The bytes read so far, shared between the reader and the run.
type Shared = Arc<Mutex<Vec<u8>>>;

/// Reads `pipe` to its end on a thread of its own into the buffer returned, and signals on the
/// receiver when it ends.
fn collect(pipe: Option<impl Read + Send + 'static>) -> (Shared, Receiver<()>) {
    let buffer = Shared::default();
    let (done, ended) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        let into = Arc::clone(&buffer);
        thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                into.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..read]);
            }
            let _ = done.send(());
        });
    } else {
        let _ = done.send(());
    }
    (buffer, ended)
}

/// The text of what was read so far.
fn text_of(buffer: &Shared) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
}

/// The last line of `text` that holds something.
fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim_end)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Kills the process group `group` (the child's pid: it leads its own group), best effort.
fn kill_group(group: u32) {
    let Ok(group) = libc::pid_t::try_from(group) else {
        return;
    };
    // SAFETY: `kill` takes plain integers; the negative pid names the group the child leads.
    unsafe { libc::kill(-group, libc::SIGKILL) };
}

#[cfg(test)]
mod tests;
