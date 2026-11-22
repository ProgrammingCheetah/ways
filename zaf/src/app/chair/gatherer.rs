//! A thread that reads one project again and again and hands each state on (D3).
//!
//! [`Gatherer::spawn`] starts the thread. It reads at once, then again `every` after the last
//! read ended, at once when asked ([`Gatherer::now`], after the chair wrote something), and soon
//! after a change the caller watches for (the hub's reports file), but never sooner than
//! `min_gap` after the last read ended. Each read is told to the sink twice: when it starts and
//! with its state when it ends. Whoever draws only takes the newest state and never reads a port
//! itself. A `floor` holds every one of those back: no read starts sooner than `floor` after the
//! last one started, so a project whose board is rationed (GitHub's, see [`floor_of`]) is read at
//! most that often, and a report or a write inside the floor waits for it to pass and then reads
//! once. The thread ends once its [`Gatherer`] is dropped or the sink says nobody listens, after
//! the read it is in: a read is never cut short.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use crate::manifest::BoardKind;

/// The least time from the start of one read of a GitHub board to the start of the next.
pub const GITHUB_FLOOR: Duration = Duration::from_secs(20);

/// The floor of a project whose board is `kind`: 20 seconds on GitHub, none on Forgejo, whose
/// reads keep the cadence as it stands.
#[must_use]
pub const fn floor_of(kind: BoardKind) -> Duration {
    match kind {
        BoardKind::GithubIssues | BoardKind::GithubProjects => GITHUB_FLOOR,
        BoardKind::ForgejoIssues => Duration::ZERO,
    }
}

/// How often a gatherer reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// From the end of one read to the start of the next.
    pub every: Duration,
    /// How often the watched change is looked at.
    pub poll: Duration,
    /// The least time from the end of one read to a read the watched change starts.
    pub min_gap: Duration,
}

impl Cadence {
    /// The cadence of the project on screen: every 5 seconds.
    pub const SHOWN: Self = Self {
        every: Duration::from_secs(5),
        poll: Duration::from_millis(250),
        min_gap: Duration::from_secs(1),
    };

    /// The cadence of a project in a tab behind: every minute, and soon after a report.
    pub const BEHIND: Self = Self {
        every: Duration::from_secs(60),
        poll: Duration::from_millis(250),
        min_gap: Duration::from_secs(5),
    };
}

/// What a gatherer tells its sink.
#[derive(Debug, Clone, PartialEq)]
pub enum GatherEvent<S> {
    /// A read began, at this time: the time its state is of.
    Started(SystemTime),
    /// A read ended with this state.
    Gathered(Box<S>),
}

/// A job for the gatherer's thread: a read a key asked for, which tells its own result.
pub type Job = Box<dyn FnOnce() + Send>;

/// What the thread is asked.
enum Request {
    /// Read at once.
    Now,
    /// Read at this cadence from now on.
    Cadence(Cadence),
    /// Run this between reads.
    Job(Job),
}

impl std::fmt::Debug for Request {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Now => formatter.write_str("Now"),
            Self::Cadence(cadence) => formatter.debug_tuple("Cadence").field(cadence).finish(),
            Self::Job(_) => formatter.write_str("Job"),
        }
    }
}

/// The handle of a gatherer's thread: asks it to read, changes its cadence, and ends it when
/// dropped.
#[derive(Debug)]
pub struct Gatherer {
    requests: Sender<Request>,
}

impl Gatherer {
    /// Starts a thread named `name` that reads with `read` at `cadence`, never starting a read
    /// sooner than `floor` after the last one started, telling `sink` each read; `changed` says
    /// whether what the caller watches changed since it last asked. The sink answers false when
    /// nobody listens any more, and the thread then ends.
    pub fn spawn<S, R, C, K>(
        name: &str,
        cadence: Cadence,
        floor: Duration,
        mut read: R,
        mut changed: C,
        mut sink: K,
    ) -> std::io::Result<Self>
    where
        S: Send + 'static,
        R: FnMut(SystemTime) -> S + Send + 'static,
        C: FnMut() -> bool + Send + 'static,
        K: FnMut(GatherEvent<S>) -> bool + Send + 'static,
    {
        let (requests, asked) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut cadence = cadence;
                loop {
                    let at = SystemTime::now();
                    let started = Instant::now();
                    if !sink(GatherEvent::Started(at)) {
                        return;
                    }
                    let state = read(at);
                    if !sink(GatherEvent::Gathered(Box::new(state))) {
                        return;
                    }
                    let ended = Instant::now();
                    if !wait(&asked, &mut cadence, (started, ended, floor), &mut changed) {
                        return;
                    }
                }
            })?;
        Ok(Self { requests })
    }

    /// Asks for a read at once: right away when the thread waits, else as soon as its read ends.
    pub fn now(&self) {
        // A thread that has ended has nobody to read for.
        let _ = self.requests.send(Request::Now);
    }

    /// Reads at `cadence` from now on, counted from the end of the last read.
    pub fn set_cadence(&self, cadence: Cadence) {
        let _ = self.requests.send(Request::Cadence(cadence));
    }

    /// Runs `job` on the thread: at once while it waits, else as soon as its read ends. It
    /// changes neither when the next read is due nor the cadence.
    pub fn run(&self, job: Job) {
        // A thread that has ended has nobody to run it for.
        let _ = self.requests.send(Request::Job(job));
    }
}

/// Waits until the next read is due; false when the handle is gone and the thread is to end.
/// `times` are when the last read started, when it ended and the floor: nothing starts the next
/// read before `started + floor`, however it was asked for, and asks inside the floor are one read.
fn wait(
    asked: &mpsc::Receiver<Request>,
    cadence: &mut Cadence,
    times: (Instant, Instant, Duration),
    changed: &mut dyn FnMut() -> bool,
) -> bool {
    let (started, ended, floor) = times;
    let open = started + floor;
    let mut seen_change = false;
    let mut wanted = false;
    loop {
        let now = Instant::now();
        let due = ended + cadence.every;
        let ready = wanted || now >= due || (seen_change && now >= ended + cadence.min_gap);
        if ready && now >= open {
            return true;
        }
        let until = if ready { open } else { due };
        let left = until.saturating_duration_since(now).min(cadence.poll);
        match asked.recv_timeout(left) {
            Ok(Request::Now) => {
                // Asks that piled up while reading are one read; the jobs among them run first.
                while let Ok(request) = asked.try_recv() {
                    match request {
                        Request::Cadence(next) => *cadence = next,
                        Request::Job(job) => job(),
                        Request::Now => {}
                    }
                }
                wanted = true;
            }
            Ok(Request::Cadence(next)) => *cadence = next,
            Ok(Request::Job(job)) => job(),
            Err(RecvTimeoutError::Timeout) => seen_change |= changed(),
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

#[cfg(test)]
mod tests;
