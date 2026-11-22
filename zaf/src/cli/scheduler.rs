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
//! In a terminal (stdin and stdout, no `--json`) the loop runs behind the live view of
//! [`super::watch_view`]: the lines it says and warns, and each tick's block, go to
//! `.zaf/logs/scheduler.log`, and the view draws. Anywhere else it streams as it always did.
//!
//! A scheduler that takes the lock fresh first runs the build start ([`crate::app::build::start`]):
//! `made <branch>` or `<branch> is open`, then `committed <id>` when a commit was made. A refusal
//! is its sentence on stderr, the lock let go and exit `1`; a backend error the same with exit `2`.
//! A restart and a take-over of a stale lock skip it.
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
use super::watch_view::{self, End, WatchView};
use crate::adapters::{clock, process};
use crate::app::actions::MINUTE;
use crate::app::build::{self, BuildError, Started};
use crate::app::release::next_version;
use crate::app::schedule::binary::{self, Binary, Identity, Look, VersionError};
use crate::app::schedule::lock::{LOCK_FILE, LockError, SchedulerLock, Taken};
use crate::app::schedule::request::{self, Request, Requests};
use crate::app::schedule::scheduler::{self, Asked, Ended, Found, Host, Memory, Opened};
use crate::app::spawn::{self, built};
use crate::composition::Composition;
use crate::ports::PrNumber;
use semver::Version;

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
        crate::eprintln_zaf!("zaf {path}: pass --version <v>, the milestone the instances build");
        return REFUSED;
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let minutes = composition.manifest().spawn.tick_minutes;
    let state_dir = composition.state_dir();
    let Some(me) = process::this() else {
        crate::eprintln_zaf!(
            "zaf {path}: cannot read this process's start time, so it cannot hold the lock"
        );
        return BACKEND;
    };
    let zaf = env!("CARGO_PKG_VERSION");
    let (lock, taken) = match SchedulerLock::take(&state_dir, &me, zaf, version, SystemTime::now())
    {
        Ok(taken) => taken,
        Err(error @ LockError::Held(_)) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            return REFUSED;
        }
        Err(error @ LockError::Io { .. }) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    let binary = Binary::this()
        .map_err(|why| {
            crate::eprintln_zaf!("zaf {path}: warning: {why}; it does not restart as a new one")
        })
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
        root: composition.root().to_owned(),
        view: None,
        ended: false,
        greeting: format!(
            "scheduler {how}: {whereabouts}; a tick every {minutes} minutes and whenever asked, \
             until stopped"
        ),
    };
    // A scheduler fresh on the lock opens the version's build before its first tick; a restart is
    // the same scheduler going on, and a take-over leaves the build as its predecessor had it.
    if matches!(taken, Taken::Fresh) {
        match build::start(&composition, version) {
            Ok(started) => {
                for line in start_lines(&started) {
                    host.say(&line);
                }
            }
            Err(error) => {
                host.warn(&error.sentence(version));
                lock.release();
                return match error {
                    BuildError::Backend(_) => BACKEND,
                    BuildError::Unsigned { .. } | BuildError::NoRemote | BuildError::Refused(_) => {
                        REFUSED
                    }
                };
            }
        }
    }
    let first = vec![match taken {
        Taken::Restarted { .. } => "restart".to_owned(),
        Taken::Fresh | Taken::TookOver(_) => "start".to_owned(),
    }];
    // In a terminal the view draws, and the loop's text goes to the log until it ends.
    host.start_view();
    let ended = scheduler::run(&mut host, MINUTE * minutes, first);
    // The terminal is put back before anything is said to it again.
    let open_question = host.end_view();
    let (signal, unserved) = match ended {
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
    // The end screen is the last thing the terminal shows.
    if !host.ended {
        host.say(&format!(
            "scheduler stopped on {signal}; {LOCK_FILE} released"
        ));
        // A question that was being asked when the stop came is still open: say how to answer it.
        if let Some(number) = open_question {
            host.say(&format!(
                "#{number} stays open: answer it with `zaf board ruling {number} '…'`"
            ));
        }
    }
    OK
}

/// The lines that say what the build start did: the branch made or already open, then the commit.
fn start_lines(started: &Started) -> Vec<String> {
    let mut lines = vec![if started.made {
        format!("made {}", started.branch)
    } else {
        format!("{} is open", started.branch)
    }];
    if let Some(id) = &started.commit {
        lines.push(format!("committed {id}"));
    }
    lines
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
    /// The project's root, for the view's reads.
    root: PathBuf,
    /// The live view, while one is up (a terminal, no `--json`): the loop's text goes to the log.
    view: Option<WatchView>,
    /// The live view ended the run with its end screen: the loop stops as it does on a signal,
    /// and nothing more is said to the terminal.
    ended: bool,
    /// The line that says the scheduler is up.
    greeting: String,
}

impl CliHost<'_> {
    /// Starts the view when this is a terminal that can show one and no `--json` is given; else
    /// the host streams as it always did.
    fn start_view(&mut self) {
        if self.view.is_some() {
            return;
        }
        if let Some(screen) = watch_view::wanted(self.cli.json) {
            self.view = WatchView::start(screen, &self.root, &self.state_dir, self.version);
        }
    }

    /// Ends the view, if one is up: the terminal and the output are put back. Gives the question
    /// that was being asked and stays open, if there was one.
    fn end_view(&mut self) -> Option<u64> {
        self.view.take().and_then(WatchView::end)
    }
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
        process::stop_signal()
            .map(str::to_owned)
            .or_else(|| self.ended.then(|| "the end screen".to_owned()))
    }

    fn asking(&mut self) -> bool {
        self.view.as_ref().is_some_and(WatchView::asking)
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
        // The terminal is put back before the exec: the new image starts its own view on it.
        let viewing = self.view.is_some();
        self.end_view();
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        let Some(binary) = &mut self.binary else {
            if viewing {
                self.start_view();
            }
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
        // This image goes on, with its view.
        if viewing {
            self.start_view();
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

    fn tick(&mut self, number: u64, asked: &Asked, memory: &mut Memory) -> u8 {
        // Discovered again each tick: the manifest the tick acts on is the one on disk now.
        let code = match Composition::discover_here() {
            Ok(composition) => one_tick(
                self.cli,
                &self.path,
                &composition,
                self.version,
                false,
                Some(Scheduled { asked, memory }),
            ),
            Err(error) => exit::fail_composition(&self.path, &error),
        };
        if let Some(view) = &self.view {
            view.ticked(number, code);
        }
        code
    }

    fn version_built(&mut self) -> bool {
        Composition::discover_here_quiet()
            .is_ok_and(|composition| built::version_built(&composition, self.version))
    }

    fn open_release(&mut self) -> Result<Opened, String> {
        let composition = Composition::discover_here_quiet().map_err(|error| error.to_string())?;
        built::open_release(&composition, self.version, None)
    }

    fn built(&mut self, version: &Version, number: PrNumber, link: Option<&str>) {
        // In a terminal the scheduler's own line goes to the log, and the view ends with its end
        // screen; anywhere else the line stands and the watch goes on.
        self.say(&scheduler::built_line(version, number, link));
        let Some(view) = self.view.take() else {
            return;
        };
        let next = Composition::discover_here_quiet().ok().map(|composition| {
            next_version(
                composition.root(),
                composition.manifest(),
                composition.vcs().ok().as_deref(),
            )
        });
        let Some(next) = next else {
            self.view = Some(view);
            return;
        };
        view.finish(End {
            version: version.clone(),
            number,
            next,
        });
        self.ended = true;
    }

    fn next_tick(&mut self, interval: Duration) {
        if !self.cli.json && !self.ended {
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
        crate::eprintln_zaf!("zaf {}: {line}", self.path);
    }
}
