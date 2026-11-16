//! The hub's tick (`POST /api/tick`, [`crate::app::hub::controls`]): `zaf spawn --once --no-wait
//! --version <v>`, run as the verb with the project's root as its folder.
//!
//! The hub serves every project on the machine from one folder, and the verb finds its project
//! from the folder it runs in, so the tick is a process of its own: the same app code the verb
//! runs, in the project's root, and the hub's own folder and state never change. The hub waits for
//! the tick only [`WAIT`]: a tick that has not ended by then is left running (the models it
//! launched are no concern of the hub's) and the answer is what it printed so far, which already
//! holds what it started, since a tick prints its plan before its launches run.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::super::board::INSTANCE_VAR;
use crate::app::hub::controls::{Ticked, Ticker};
use crate::app::schedule::binary;

/// How long the hub waits for a tick to end before it answers.
const WAIT: Duration = Duration::from_secs(60);

/// How long the hub waits, once a tick has ended, for the last of its output.
const GRACE: Duration = Duration::from_millis(500);

/// The variable a phase's run is named by; a tick the hub runs is no phase's.
pub(super) const RUN_VAR: &str = "ZAF_RUN";

/// Runs the tick as a `zaf` binary: this hub's own, unless it is given another.
#[derive(Debug, Clone, Default)]
pub struct Subprocess {
    program: Option<PathBuf>,
}

impl Subprocess {
    /// The tick run as the binary this hub was started from, found as a phase's launcher finds it
    /// ([`binary::path_to_start`]).
    pub fn this() -> Self {
        Self::default()
    }

    /// The tick run as `program`, which takes `zaf`'s arguments.
    pub fn of(program: PathBuf) -> Self {
        Self {
            program: Some(program),
        }
    }
}

impl Ticker for Subprocess {
    fn tick(&self, root: &Path, version: &str) -> Ticked {
        let program = match &self.program {
            Some(program) => Ok(program.clone()),
            None => binary::path_to_start(),
        };
        match program {
            Ok(program) => run(&program, root, version, WAIT),
            Err(why) => Ticked::Failed(format!("zaf cannot be started to run the tick: {why}")),
        }
    }
}

/// The tick as a command: the verb, in `root`, with its output piped to the hub.
fn command(program: &Path, root: &Path, version: &str) -> Command {
    let mut command = Command::new(program);
    command
        .args(["spawn", "--once", "--no-wait", "--version", version])
        .current_dir(root)
        // The hub is no instance, and no phase's run: neither is a tick it runs.
        .env_remove(INSTANCE_VAR)
        .env_remove(RUN_VAR)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Runs `program spawn --once --no-wait --version <version>` in `root`, waiting `wait` for it.
fn run(program: &Path, root: &Path, version: &str, wait: Duration) -> Ticked {
    let started = command(program, root, version).spawn();
    let mut child = match started {
        Ok(child) => child,
        Err(error) => {
            return Ticked::Failed(format!(
                "zaf could not be started in {}: {error}",
                root.display()
            ));
        }
    };
    let printed = Lines::default();
    let said = Lines::default();
    let readers = [
        child
            .stdout
            .take()
            .map(|stream| collect(stream, printed.clone())),
        child
            .stderr
            .take()
            .map(|stream| collect(stream, said.clone())),
    ];
    let (ended, ends) = mpsc::channel();
    thread::spawn(move || {
        let _ = ended.send(child.wait());
    });
    let status = match ends.recv_timeout(wait) {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            return Ticked::Failed(format!("the tick's end could not be read: {error}"));
        }
        Err(RecvTimeoutError::Timeout) => return Ticked::Running(printed.read()),
        Err(RecvTimeoutError::Disconnected) => {
            return Ticked::Failed("the tick's end could not be read".to_owned());
        }
    };
    // A model the tick started may hold the pipes open past the tick's own end, so the last of
    // the output is waited for a moment, not to its end.
    let until = Instant::now() + GRACE;
    while Instant::now() < until && !readers.iter().flatten().all(JoinHandle::is_finished) {
        thread::sleep(Duration::from_millis(10));
    }
    match status.code() {
        Some(0) => Ticked::Ran(printed.read()),
        Some(1) => Ticked::Refused(why(&said.read(), &printed.read(), "the tick was refused")),
        Some(code) => Ticked::Failed(why(
            &said.read(),
            &printed.read(),
            &format!("the tick failed with exit {code}"),
        )),
        None => Ticked::Failed(why(&said.read(), &printed.read(), "the tick was killed")),
    }
}

/// What a failed tick said, in one line: its stderr, else its stdout, else `otherwise`.
fn why(said: &[String], printed: &[String], otherwise: &str) -> String {
    let lines = if said.is_empty() { printed } else { said };
    if lines.is_empty() {
        otherwise.to_owned()
    } else {
        lines.join("; ")
    }
}

/// The lines a stream has given so far, readable while it is still being read.
#[derive(Debug, Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn read(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Reads `stream` line by line into `into` until it ends; blank lines are dropped.
fn collect(stream: impl Read + Send + 'static, into: Lines) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let line = line.trim_end().to_owned();
            if !line.is_empty() {
                into.0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(line);
            }
        }
    })
}

#[cfg(test)]
mod tests;
