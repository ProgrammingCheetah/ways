//! `zaf spawn --watch --version <v>`: the scheduler that stays up (docs/zaf-actions.md §5.1).
//!
//! It takes `.zaf/scheduler.lock` for as long as it runs ([`crate::app::schedule::lock`]): a second
//! one is told where the first is and exits `1`. It runs the loop of
//! [`crate::app::schedule::scheduler`] with this process as its [`Host`]: one tick at once, then
//! one for each turn something asked for one in `.zaf/tick.request`
//! ([`crate::app::schedule::request`]) or the timer did, `[spawn] tick_minutes` after the last tick
//! ended. Each tick is the tick of `zaf spawn --once`, from the `pre-tick` hook to the `post-tick`
//! one ([`super::spawn::one_tick`]), its block naming who asked.
//!
//! At the top of each turn, never during a tick, it looks at the binary it was started from
//! ([`crate::app::schedule::binary`]): a new one that answers `--version` is `exec`ed with the same
//! arguments ([`process::exec`]), so the same process goes on as the new image and takes its lock
//! again; one that does not is named on stderr and this image goes on. SIGINT and SIGTERM end it
//! between two ticks, or once the tick that runs has ended: it clears its lock line, lets the lock
//! go and exits `0`. They are caught before anything is taken from the request file, and held back
//! across a restart, which keeps them pending for the new image. A Ctrl-C in its terminal reaches
//! it alone: every process a tick starts runs in a process group of its own, so the tick runs to
//! its end with its reads whole. A phase it started runs on as its own process.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use super::Cli;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::spawn::{Scheduled, one_tick};
use crate::adapters::{clock, process};
use crate::app::actions::MINUTE;
use crate::app::schedule::binary::{self, Binary, Identity, Look, VersionError};
use crate::app::schedule::lock::{LOCK_FILE, LockError, SchedulerLock, Taken};
use crate::app::schedule::request::{self, Request, Requests};
use crate::app::schedule::scheduler::{self, Asked, Ended, Found, Host, Memory};
use crate::app::spawn;
use crate::composition::Composition;

/// How long the loop waits between two looks when nothing asked.
const PAUSE: Duration = Duration::from_secs(1);

/// How often a pause looks for a stop signal.
const PAUSE_STEP: Duration = Duration::from_millis(100);

/// Runs the scheduler until a stop signal (see the module above). Refuses without `--version`, and
/// with a scheduler already running here; a lock or a request file that cannot be used is a backend
/// failure.
pub fn run(cli: &Cli, version: Option<&str>) -> u8 {
    let path = cli.verb.path();
    let Some(version) = version else {
        eprintln!("zaf {path}: pass --version <v>, the milestone the instances build");
        return REFUSED;
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let minutes = composition.manifest().spawn.tick_minutes;
    let state_dir = composition.state_dir();
    let Some(me) = process::this() else {
        eprintln!("zaf {path}: cannot read this process's start time, so it cannot hold the lock");
        return BACKEND;
    };
    let zaf = env!("CARGO_PKG_VERSION");
    let (lock, taken) = match SchedulerLock::take(&state_dir, &me, zaf, version, SystemTime::now())
    {
        Ok(taken) => taken,
        Err(error @ LockError::Held(_)) => {
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
        Err(error @ LockError::Io { .. }) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    let binary = Binary::this()
        .map_err(|why| eprintln!("zaf {path}: warning: {why}; it does not restart as a new one"))
        .ok();
    let whereabouts = lock.holding().whereabouts();
    let how = match &taken {
        Taken::Fresh => "up".to_owned(),
        Taken::Restarted { from } => format!("restarted from zaf {from}"),
        Taken::TookOver(gone) => format!(
            "up, taking {LOCK_FILE} over from pid {}, which is gone",
            gone.pid
        ),
    };
    let mut host = CliHost {
        cli,
        path: path.clone(),
        version,
        state_dir,
        binary,
        ready: None,
        requests: None,
        unreadable: false,
        greeting: format!(
            "scheduler {how}: {whereabouts}; a tick every {minutes} minutes and whenever asked, \
             until stopped"
        ),
    };
    let first = vec![match taken {
        Taken::Restarted { .. } => "restart".to_owned(),
        Taken::Fresh | Taken::TookOver(_) => "start".to_owned(),
    }];
    let (signal, unserved) = match scheduler::run(&mut host, MINUTE * minutes, first) {
        Ended::Stopped { signal, unserved } => (signal, unserved),
        Ended::Failed(why) => {
            host.warn(&why);
            lock.release();
            return BACKEND;
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
    lock.release();
    host.say(&format!(
        "scheduler stopped on {signal}; {LOCK_FILE} released"
    ));
    OK
}

/// The world of the loop: this process, the project's state folder and the binary it runs.
struct CliHost<'a> {
    cli: &'a Cli,
    path: String,
    version: &'a str,
    state_dir: PathBuf,
    /// The binary started from; none when it cannot be watched.
    binary: Option<Binary>,
    /// The new binary that answered `--version`, until the restart into it is tried.
    ready: Option<Identity>,
    /// The request file, once the loop has taken what was left in it.
    requests: Option<Requests>,
    /// The request file could not be read at the last look, which was said.
    unreadable: bool,
    /// The line that says the scheduler is up.
    greeting: String,
}

impl Host for CliHost<'_> {
    fn catch_stop(&mut self) {
        if let Err(why) = process::catch_stop() {
            self.warn(&format!("warning: {why}; a signal ends it at once"));
        }
        // A stop held back across the restart that started this image is recorded now.
        if let Err(why) = process::let_stop_through() {
            self.warn(&format!("warning: {why}"));
        }
    }

    fn left(&mut self) -> Result<Vec<String>, String> {
        let (requests, left) = Requests::start(&self.state_dir)?;
        self.requests = Some(requests);
        Ok(left.into_iter().map(|request| request.who).collect())
    }

    fn up(&mut self) {
        let line = self.greeting.clone();
        self.say(&line);
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn stop(&mut self) -> Option<String> {
        process::stop_signal().map(str::to_owned)
    }

    fn binary(&mut self) -> Found {
        let Some(binary) = &mut self.binary else {
            return Found::Nothing;
        };
        let Look::Ready(identity) = binary.look() else {
            return Found::Nothing;
        };
        match binary::version_of(binary.path()) {
            Ok(version) => {
                self.ready = Some(identity);
                Found::Restart(version)
            }
            // Still open for writing: the next look tries again.
            Err(VersionError::Busy) => Found::Nothing,
            Err(VersionError::Refused(why)) => {
                binary.refuse(identity);
                Found::Refused(format!(
                    "the new binary at {} does not run ({why}); this image goes on, and tries \
                     again once the file changes",
                    binary.path().display()
                ))
            }
        }
    }

    fn hold_stop(&mut self) {
        if let Err(why) = process::hold_stop() {
            self.warn(&format!("warning: {why}"));
        }
    }

    fn let_stop_through(&mut self) {
        if let Err(why) = process::let_stop_through() {
            self.warn(&format!("warning: {why}"));
        }
    }

    fn restart(&mut self) -> String {
        // What was served stays served: the new image names only what no tick has.
        if let Some(Err(why)) = self.requests.as_mut().map(Requests::let_go) {
            self.warn(&why);
        }
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        let Some(binary) = &mut self.binary else {
            return "no binary is watched".to_owned();
        };
        // The same arguments, and the stop signals still held back: `std`'s exec would let them
        // through to this image's handler, which the exec then wipes.
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let error = process::exec(binary.path(), &args);
        let why = format!("cannot exec {}: {error}", binary.path().display());
        // Not tried again until the file changes.
        if let Some(identity) = self.ready.take() {
            binary.refuse(identity);
        }
        why
    }

    fn requests(&mut self) -> Vec<String> {
        let Some(looked) = self.requests.as_mut().map(Requests::look) else {
            return Vec::new();
        };
        match looked {
            Ok(looked) => {
                self.unreadable = false;
                // Said each time the file could not be emptied; what was read is served.
                if let Some(why) = &looked.not_emptied {
                    self.warn(why);
                }
                looked
                    .requests
                    .into_iter()
                    .map(|request| request.who)
                    .collect()
            }
            Err(why) => {
                if !self.unreadable {
                    self.warn(&format!("{why}; the timer still asks"));
                }
                self.unreadable = true;
                Vec::new()
            }
        }
    }

    fn held(&mut self) -> bool {
        spawn::lock_is_held(&self.state_dir).unwrap_or(false)
    }

    fn tick(&mut self, _number: u64, asked: &Asked, memory: &mut Memory) -> u8 {
        // Discovered again each tick: the manifest the tick acts on is the one on disk now.
        let composition = match Composition::discover_here() {
            Ok(composition) => composition,
            Err(error) => return exit::fail_composition(&self.path, &error),
        };
        one_tick(
            self.cli,
            &self.path,
            &composition,
            self.version,
            false,
            Some(Scheduled { asked, memory }),
        )
    }

    fn next_tick(&mut self, interval: Duration) {
        if !self.cli.json {
            let now = SystemTime::now();
            println!(
                "{}",
                spawn::digest::next_tick_line(now, interval, clock::utc_offset(now))
            );
        }
    }

    fn waits_for_lock(&mut self) {
        let now = SystemTime::now();
        let line = format!(
            "{}; it runs once the lock is free",
            spawn::digest::skipped_line(now, clock::utc_offset(now))
        );
        self.say(&line);
    }

    fn pause(&mut self) {
        let until = Instant::now() + PAUSE;
        while Instant::now() < until && process::stop_signal().is_none() {
            std::thread::sleep(PAUSE_STEP);
        }
    }

    fn say(&mut self, line: &str) {
        if self.cli.json {
            println!("{}", serde_json::json!({ "scheduler": line }));
        } else {
            println!("{line}");
        }
    }

    fn warn(&mut self, line: &str) {
        eprintln!("zaf {}: {line}", self.path);
    }
}
