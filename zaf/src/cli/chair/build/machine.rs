//! The build controls on this machine: the scheduler started detached, its lock read, SIGTERM
//! sent, a tick asked for, a task stopped through `zaf spawn stop`'s own path.
//!
//! The scheduler the chair starts is the verb, `zaf spawn --watch --version <v>`, run as the
//! binary a phase's launcher would start ([`binary::path_to_start`]) with the project's root as
//! its folder. It leads a session of its own (`setsid` before it runs), so neither a closed
//! terminal nor the chair's end reaches it; it reads nothing (stdin is `/dev/null`) and writes
//! into its own log under `.zaf/logs/`. It is no phase's and no instance's: every `ZAF_` variable
//! but `ZAF_HOME` is dropped from what it inherits. One that ends at once (refused, or broken) is
//! said with its log's last line; one that keeps running is waited for on a thread of its own, so
//! it never lingers as a zombie while the chair is open.
//!
//! [`Processes`] is what touches the machine, so the controls' logic runs against a fake in the
//! tests.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};

use super::{Controls, log_name, watch_args};
use crate::adapters::process::{self, Signal};
use crate::app::chair::ReleaseVersion;
use crate::app::hub::controls::Failure;
use crate::app::schedule::lock::{self, Holding};
use crate::app::schedule::{binary, request};
use crate::composition::STATE_DIR;
use crate::domain::TicketId;
use crate::ports::Holder;

/// How long a scheduler just started is watched for ending at once.
const SETTLE: Duration = Duration::from_millis(1500);

/// How often it is looked at meanwhile.
const LOOK: Duration = Duration::from_millis(50);

/// The folder of the logs inside the state folder.
const LOGS_DIR: &str = "logs";

/// Who asks for a tick, as the scheduler's tick block names it.
const WHO: &str = "zaf chair (t)";

/// The one `ZAF_` variable a scheduler the chair starts keeps: the zaf home it was given.
const KEPT: &str = "ZAF_HOME";

/// A process started detached: what runs, where, and the log it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The binary.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub folder: PathBuf,
    /// The log its stdout and stderr go to, made new.
    pub log: PathBuf,
    /// The variables it does not inherit.
    pub dropped: Vec<OsString>,
}

/// What the controls need of the machine.
pub trait Processes: Send + Sync {
    /// The binary to start `zaf` as.
    fn program(&self) -> Result<PathBuf, String>;
    /// The scheduler the lock in `state_dir` names, while one runs.
    fn scheduler(&self, state_dir: &Path) -> Option<Holding>;
    /// Sends SIGTERM to `holder`, when it is still the process recorded.
    fn terminate(&self, holder: &Holder);
    /// Starts `launch` detached: its pid, or why it did not start or ended at once.
    fn detach(&self, launch: &Launch) -> Result<u32, String>;
    /// Asks the scheduler of `state_dir` for a tick: the scheduler asked, none when none runs.
    fn ask_tick(&self, state_dir: &Path) -> Result<Option<Holding>, String>;
}

/// The variables of `names` a scheduler the chair starts does not inherit: every `ZAF_` one but
/// [`KEPT`].
pub fn dropped<'a>(names: impl IntoIterator<Item = &'a OsStr>) -> Vec<OsString> {
    names
        .into_iter()
        .filter(|name| {
            name.to_str()
                .is_some_and(|name| name.starts_with("ZAF_") && name != KEPT)
        })
        .map(OsStr::to_owned)
        .collect()
}

/// The process `B` sends SIGTERM to: the scheduler `holding` names, when it is still the one the
/// Engineer was asked about (`pid`). Another, or none, is refused and nothing is sent.
pub fn signal_target(holding: Option<&Holding>, pid: u32) -> Result<Holder, Failure> {
    match holding {
        None => Err(Failure::Refused("no scheduler runs now".to_owned())),
        Some(holding) if holding.pid != pid => Err(Failure::Refused(format!(
            "another scheduler holds the lock now ({}); read again first",
            holding.whereabouts()
        ))),
        Some(holding) => Ok(holding.holder()),
    }
}

/// `path` as it reads from `root`: relative when it is inside.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The controls over `P`'s processes, stopping tasks as the zaf home `home`.
#[derive(Debug, Clone)]
pub struct Machine<P = Real> {
    home: Option<PathBuf>,
    processes: P,
}

impl Machine<Real> {
    /// The controls on this machine, as the zaf home `home`.
    pub fn new(home: Option<PathBuf>) -> Self {
        Self::with(home, Real)
    }
}

impl<P: Processes> Machine<P> {
    /// The controls over `processes`.
    pub fn with(home: Option<PathBuf>, processes: P) -> Self {
        Self { home, processes }
    }
}

impl<P: Processes> Controls for Machine<P> {
    fn start(&self, root: &Path, version: &ReleaseVersion) -> Result<String, Failure> {
        let state_dir = root.join(STATE_DIR);
        if let Some(holding) = self.processes.scheduler(&state_dir) {
            return Err(Failure::Refused(format!(
                "a scheduler already runs ({})",
                holding.whereabouts()
            )));
        }
        let program = self.processes.program().map_err(Failure::Failed)?;
        let names: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
        let launch = Launch {
            program,
            args: watch_args(version),
            folder: root.to_owned(),
            log: state_dir
                .join(LOGS_DIR)
                .join(log_name(SystemTime::now(), version)),
            dropped: dropped(names.iter().map(OsString::as_os_str)),
        };
        let pid = self.processes.detach(&launch).map_err(Failure::Failed)?;
        Ok(format!(
            "the build of {version} started (pid {pid}); its log is {}",
            shown(root, &launch.log)
        ))
    }

    fn stop(&self, root: &Path, pid: u32) -> Result<String, Failure> {
        let holding = self.processes.scheduler(&root.join(STATE_DIR));
        let holder = signal_target(holding.as_ref(), pid)?;
        self.processes.terminate(&holder);
        Ok(format!("the scheduler (pid {pid}) stops after its tick"))
    }

    fn stop_task(&self, root: &Path, task: TicketId) -> Result<String, Failure> {
        let stopped = crate::cli::spawn_stop::stop_in(root, self.home.as_deref(), task)?;
        let mut said: Vec<String> = stopped
            .runs
            .iter()
            .map(|run| {
                let how = if run.killed { ", killed" } else { "" };
                format!("stopped {} (pid {}{how})", run.instance, run.pid)
            })
            .collect();
        said.push(if stopped.released.is_empty() {
            format!("{task} held no claim")
        } else {
            format!("released {task}")
        });
        Ok(said.join("; "))
    }

    fn tick(&self, root: &Path) -> Result<String, Failure> {
        match self.processes.ask_tick(&root.join(STATE_DIR)) {
            Ok(Some(holding)) => Ok(format!(
                "asked the running scheduler for a tick: {}",
                holding.whereabouts()
            )),
            Ok(None) => Err(Failure::Refused(
                "no scheduler runs now; b starts one".to_owned(),
            )),
            Err(why) => Err(Failure::Failed(why)),
        }
    }
}

/// This machine's processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Real;

impl Processes for Real {
    fn program(&self) -> Result<PathBuf, String> {
        binary::path_to_start()
    }

    fn scheduler(&self, state_dir: &Path) -> Option<Holding> {
        lock::running(state_dir, &process::alive)
    }

    fn terminate(&self, holder: &Holder) {
        process::signal(holder, None, Signal::Terminate);
    }

    fn detach(&self, launch: &Launch) -> Result<u32, String> {
        detach(launch, SETTLE)
    }

    fn ask_tick(&self, state_dir: &Path) -> Result<Option<Holding>, String> {
        request::ask(state_dir, WHO, SystemTime::now(), &process::alive)
            .map_err(|not_asked| not_asked.to_string())
    }
}

/// Starts `launch` in a session of its own and watches it for `settle`: its pid while it runs,
/// or why it did not start or ended within that time.
pub fn detach(launch: &Launch, settle: Duration) -> Result<u32, String> {
    let failed = |what: &str, error: &io::Error| format!("{what}: {error}");
    if let Some(folder) = launch.log.parent() {
        fs::create_dir_all(folder)
            .map_err(|error| failed(&format!("cannot make {}", folder.display()), &error))?;
    }
    let log = OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .open(&launch.log)
        .map_err(|error| failed(&format!("cannot make {}", launch.log.display()), &error))?;
    let errors = log
        .try_clone()
        .map_err(|error| failed("cannot share the log", &error))?;
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .current_dir(&launch.folder)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors);
    for name in &launch.dropped {
        command.env_remove(name);
    }
    let limit = process::descriptor_limit();
    // SAFETY: the hook runs in the child between fork and exec, and calls only `setsid` and
    // `close_above_stdio`'s plain system calls, which are async-signal-safe and touch no memory
    // of the parent's. The chair's terminal and its log are no business of a detached process.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            process::close_above_stdio(limit);
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| {
        failed(
            &format!("{} did not start", launch.program.display()),
            &error,
        )
    })?;
    let pid = child.id();
    let until = Instant::now() + settle;
    while Instant::now() < until {
        match child.try_wait() {
            Ok(Some(status)) => return Err(ended(status, &launch.log)),
            Ok(None) => std::thread::sleep(LOOK),
            Err(_) => break,
        }
    }
    let _ = std::thread::Builder::new()
        .name("chair-reap".to_owned())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(pid)
}

/// Why a process that ended at once ended: its log's last line, else its exit.
fn ended(status: ExitStatus, log: &Path) -> String {
    let said = fs::read_to_string(log).unwrap_or_default();
    let last = said
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(crate::domain::printable);
    let exit = status.code().map_or_else(
        || "it was killed".to_owned(),
        |code| format!("it exited {code}"),
    );
    match last {
        Some(line) => format!("{exit}: {line}"),
        None => exit,
    }
}

#[cfg(test)]
mod tests;
