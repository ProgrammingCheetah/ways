//! The watches a hub runs (epic #1 §4, docs/zaf-actions.md §5.1): `zaf spawn --watch` for a
//! project, started and stopped from the hub's process.
//!
//! A [`Watches`] is the one owner of every watch the hub started. A watch is the loop of `zaf spawn
//! --watch` ([`scheduler::run`](crate::app::schedule::scheduler::run)) on a thread of this process,
//! holding the project's `.zaf/scheduler.lock` with this process as its holder, so [`watch_running`]
//! reads it as running, and a `zaf spawn --once` or a phase's end that asks for a tick appends its
//! request to `.zaf/tick.request`, which the loop serves. What a tick is, is [`Tick`]'s: the hub
//! runs it as a `zaf` process in the project's root, since the tick is the verb's code and finds its
//! project from the folder it runs in, and a hub serves every project from its own.
//!
//! One watch for a project, whoever started it: a start refuses when the lock is held, by a `zaf
//! spawn --watch` in a terminal or by this hub's own. A stop asks the loop to end after its current
//! tick, which is how the loop ends anywhere; it never ends a phase or a tick. A watch the hub did
//! not start is never stopped by it.
//!
//! Every watch ends with the hub: dropping the owner asks each loop to end and waits for them, and
//! a hub that ends with no drop (a signal) takes its threads with the process, and the OS lets go of
//! the locks. [`WatchHandle`] is what the hub's server holds: it starts and stops but never ends the
//! watches by being dropped, since the server's threads live as long as the process.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use crate::adapters::process;
use crate::app::schedule::lock::{self, LockError, SchedulerLock};
use crate::app::schedule::request::{self, Request, Requests};
use crate::app::schedule::scheduler::{self, Asked, Ended, Found, Host, Memory, Opened};
use crate::app::spawn;

/// The variable a tick the hub runs carries: the pid of the scheduler it runs for. A `zaf spawn
/// --once` that finds that scheduler holding the project's lock ticks here instead of asking it,
/// since this tick is the scheduler's own.
pub const SCHEDULED_BY_VAR: &str = "ZAF_SCHEDULED_BY";

/// How long a stop waits for the loop to end before it answers that it is still ending.
const STOP_WAIT: Duration = Duration::from_secs(20);

/// How often a wait looks again.
const LOOK: Duration = Duration::from_millis(20);

/// How often a pause looks for the stop.
const PAUSE_STEP: Duration = Duration::from_millis(50);

/// How long the loop pauses when nothing asked.
const PAUSE: Duration = Duration::from_secs(1);

/// What runs one tick of a project: the world a watch acts in.
pub trait Tick: Send + Sync {
    /// Runs the tick for `asked`, waiting for it, and gives its exit code.
    fn tick(&self, asked: &Asked) -> u8;
    /// Says a line about the watch, for whoever reads the hub's output.
    fn say(&self, line: &str);
    /// Whether every epic of the watched version is done, read now; not built by default.
    fn version_built(&self) -> bool {
        false
    }
    /// Runs `release-open` for the watched version and gives its pull request, or why not.
    fn open_release(&self) -> Result<Opened, String> {
        Err("this watch opens no release pull request".to_owned())
    }
}

/// Why a watch was not started or stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchError {
    /// The state refuses it and nothing changed, with why.
    Refused(String),
    /// The machine could not do it, with why.
    Failed(String),
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// A watch the hub started, as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The milestone it ticks for.
    pub version: String,
    /// When it took the lock, as the lock's line says.
    pub since: String,
}

/// What a stop came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// The loop ended, and the lock is let go.
    Ended,
    /// The loop was asked to end and a tick still runs: it ends once that tick has.
    Ending,
}

struct Entry {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
    version: String,
}

#[derive(Default)]
struct State {
    /// Set once the owner is dropped: nothing starts after.
    ended: bool,
    /// The watches this hub runs, by the project's state folder.
    running: BTreeMap<PathBuf, Entry>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks every loop to end after its tick and waits for each; nothing starts after.
    fn end(&self) {
        let entries = {
            let mut state = self.state();
            state.ended = true;
            std::mem::take(&mut state.running)
        };
        for entry in entries.values() {
            entry.stop.store(true, Ordering::SeqCst);
        }
        for entry in entries.into_values() {
            let _ = entry.thread.join();
        }
    }
}

/// The owner of the hub's watches: dropped, every one ends.
#[derive(Default)]
pub struct Watches {
    shared: Arc<Shared>,
}

impl Watches {
    /// An owner with no watch.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the hub's server starts and stops watches with.
    pub fn handle(&self) -> WatchHandle {
        WatchHandle {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for Watches {
    fn drop(&mut self) {
        self.shared.end();
    }
}

impl std::fmt::Debug for Watches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watches").finish_non_exhaustive()
    }
}

/// Starts and stops the watches of a [`Watches`], and ends none by being dropped.
#[derive(Clone)]
pub struct WatchHandle {
    shared: Arc<Shared>,
}

impl WatchHandle {
    /// Starts the watch of the project whose state folder is `state_dir` for the milestone
    /// `version`, with a tick every `interval` and whenever asked: the first tick runs at once.
    /// Refused while a scheduler runs for the project, this hub's or another's, and once the hub's
    /// watches have ended.
    pub fn start(
        &self,
        state_dir: &Path,
        version: &str,
        interval: Duration,
        tick: Arc<dyn Tick>,
    ) -> Result<Started, WatchError> {
        let mut state = self.shared.state();
        if state.ended {
            return Err(WatchError::Refused(
                "the hub is ending, so no watch starts".to_owned(),
            ));
        }
        if let Some(entry) = state.running.get(state_dir) {
            if !entry.thread.is_finished() {
                return Err(WatchError::Refused(format!(
                    "this hub already runs a watch here, for --version {}",
                    entry.version
                )));
            }
            // A loop that ended by itself: its entry is joined and replaced.
            if let Some(entry) = state.running.remove(state_dir) {
                let _ = entry.thread.join();
            }
        }
        if let Some(holding) = lock::running(state_dir, &process::alive) {
            return Err(WatchError::Refused(format!(
                "a scheduler already runs here: {}",
                holding.whereabouts()
            )));
        }
        let Some(me) = process::this() else {
            return Err(WatchError::Failed(
                "cannot read this process's start time, so it cannot hold the lock".to_owned(),
            ));
        };
        let (lock, _) = SchedulerLock::take(
            state_dir,
            &me,
            env!("CARGO_PKG_VERSION"),
            version,
            SystemTime::now(),
        )
        .map_err(|error| match error {
            LockError::Held(_) => WatchError::Refused(error.to_string()),
            LockError::Io { .. } => WatchError::Failed(error.to_string()),
        })?;
        let started = Started {
            version: version.to_owned(),
            since: lock.holding().since.clone(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut host = HubHost {
            state_dir: state_dir.to_owned(),
            stop: Arc::clone(&stop),
            tick,
            requests: None,
        };
        let spawned = thread::Builder::new()
            .name("zaf-hub-watch".to_owned())
            .spawn(move || {
                run(&mut host, interval);
                lock.release();
            });
        match spawned {
            Ok(thread) => {
                state.running.insert(
                    state_dir.to_owned(),
                    Entry {
                        stop,
                        thread,
                        version: version.to_owned(),
                    },
                );
                Ok(started)
            }
            // The closure, and the lock in it, is dropped: the OS lets the lock go.
            Err(error) => Err(WatchError::Failed(format!(
                "the watch's thread could not start: {error}"
            ))),
        }
    }

    /// Asks the watch this hub runs for the project at `state_dir` to end after its current tick,
    /// and waits for it a while. Refused when no watch of this hub's runs there; a scheduler of
    /// another process is named, and never stopped from here.
    pub fn stop(&self, state_dir: &Path) -> Result<Stopped, WatchError> {
        let stopping = {
            let state = self.shared.state();
            state.running.get(state_dir).map(|entry| {
                entry.stop.store(true, Ordering::SeqCst);
            })
        };
        if stopping.is_none() {
            return Err(WatchError::Refused(
                match lock::running(state_dir, &process::alive) {
                    Some(holding) => format!(
                        "the scheduler that runs here ({}) was not started by this hub: stop it \
                         where it runs",
                        holding.whereabouts()
                    ),
                    None => "no watch runs here".to_owned(),
                },
            ));
        }
        let until = Instant::now() + STOP_WAIT;
        loop {
            let entry = {
                let mut state = self.shared.state();
                match state.running.get(state_dir) {
                    // Another stop, or a start after the loop ended, took it.
                    None => return Ok(Stopped::Ended),
                    Some(entry) if entry.thread.is_finished() => state.running.remove(state_dir),
                    Some(_) => None,
                }
            };
            if let Some(entry) = entry {
                let _ = entry.thread.join();
                return Ok(Stopped::Ended);
            }
            if Instant::now() >= until {
                return Ok(Stopped::Ending);
            }
            thread::sleep(LOOK);
        }
    }

    /// Whether this hub runs a watch for the project at `state_dir`, its loop not yet ended.
    pub fn runs(&self, state_dir: &Path) -> bool {
        self.shared
            .state()
            .running
            .get(state_dir)
            .is_some_and(|entry| !entry.thread.is_finished())
    }
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHandle").finish_non_exhaustive()
    }
}

/// Runs the loop until its stop is asked, and gives back what no tick served.
fn run(host: &mut HubHost, interval: Duration) {
    let first = vec!["hub".to_owned()];
    let unserved = match scheduler::run(host, interval, first) {
        Ended::Stopped { unserved, .. } => unserved,
        Ended::Failed(why) => {
            host.warn(&why);
            return;
        }
    };
    // The requests no tick served stay for the next scheduler to name.
    if let Some(Err(why)) = host.requests.as_mut().map(Requests::let_go) {
        host.warn(&why);
    }
    for who in unserved {
        let request = Request {
            at: SystemTime::now(),
            who,
        };
        if let Err(why) = request::append(&host.state_dir, &request) {
            host.warn(&why);
        }
    }
}

/// The loop's world in the hub: the project's state folder, the stop a handle sets and what ticks.
struct HubHost {
    state_dir: PathBuf,
    stop: Arc<AtomicBool>,
    tick: Arc<dyn Tick>,
    /// The request file, once the loop has taken what was left in it.
    requests: Option<Requests>,
}

impl Host for HubHost {
    fn catch_stop(&mut self) {}

    fn left(&mut self) -> Result<Vec<String>, String> {
        let (requests, left) = Requests::start(&self.state_dir)?;
        self.requests = Some(requests);
        Ok(left.into_iter().map(|request| request.who).collect())
    }

    fn up(&mut self) {
        self.say("watch up: a tick now, then on every request and on the timer");
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn stop(&mut self) -> Option<String> {
        self.stop
            .load(Ordering::SeqCst)
            .then(|| "the hub's stop".to_owned())
    }

    // A hub's watch never restarts as a new binary: the hub is the process.
    fn binary(&mut self) -> Found {
        Found::Nothing
    }

    fn hold_stop(&mut self) {}

    fn let_stop_through(&mut self) {}

    fn restart(&mut self) -> String {
        "a watch of the hub does not restart".to_owned()
    }

    fn requests(&mut self) -> Vec<String> {
        let Some(looked) = self.requests.as_mut().map(Requests::look) else {
            return Vec::new();
        };
        match looked {
            Ok(looked) => looked
                .requests
                .into_iter()
                .map(|request| request.who)
                .collect(),
            Err(why) => {
                self.warn(&format!("{why}; the timer still asks"));
                Vec::new()
            }
        }
    }

    fn held(&mut self) -> bool {
        spawn::lock_is_held(&self.state_dir).unwrap_or(false)
    }

    fn tick(&mut self, _number: u64, asked: &Asked, _memory: &mut Memory) -> u8 {
        self.tick.tick(asked)
    }

    fn version_built(&mut self) -> bool {
        self.tick.version_built()
    }

    fn open_release(&mut self) -> Result<Opened, String> {
        self.tick.open_release()
    }

    fn built(&mut self, version: &str, number: &str, link: Option<&str>) {
        self.say(&scheduler::built_line(version, number, link));
    }

    fn next_tick(&mut self, _interval: Duration) {}

    fn waits_for_lock(&mut self) {
        self.say("a tick waits for the lock");
    }

    fn pause(&mut self) {
        let until = Instant::now() + PAUSE;
        while Instant::now() < until && !self.stop.load(Ordering::SeqCst) {
            thread::sleep(PAUSE_STEP);
        }
    }

    fn say(&mut self, line: &str) {
        self.tick.say(line);
    }

    fn warn(&mut self, line: &str) {
        self.tick.say(&format!("warning: {line}"));
    }
}

/// Whether the scheduler whose lock line names `holder_pid` is the one a tick is run for:
/// `scheduled_by` is [`SCHEDULED_BY_VAR`]'s value. Only that scheduler's own pid counts.
pub fn is_scheduled_by(scheduled_by: Option<&str>, holder_pid: u32) -> bool {
    scheduled_by.and_then(|pid| pid.trim().parse::<u32>().ok()) == Some(holder_pid)
}

#[cfg(test)]
mod tests;
