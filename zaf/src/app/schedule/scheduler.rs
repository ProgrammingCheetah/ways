//! The scheduler that stays up (docs/zaf-actions.md §5.1, R9, R12, R17, R22): `zaf spawn --watch`.
//!
//! [`run`] is its loop, and it is the only thing that ticks while it runs. Before its first turn it
//! catches the stop signals and only then takes what an earlier scheduler left in the request
//! file, so a stop that lands at any moment after hands every request back. Each turn, at the top:
//!
//! 1. A stop signal (SIGINT, SIGTERM) that has arrived ends the loop. One that arrives during a
//!    tick waits for the tick to end; the loop never ends a phase, which runs as its own process.
//! 2. With no request taken and waiting for its tick, the binary it was started from is looked at
//!    ([`super::binary`]): a new one that answers `--version` is restarted into, at once; one that
//!    does not is named and the loop goes on. This is between two ticks, so never in the middle of
//!    one, nor while a planning or challenger launch runs, since the tick joins those before it
//!    returns; and no request is lost, since none is held in memory then. The stop signals are
//!    held back from before the last look for one through the restart, which keeps them pending:
//!    one that lands then stops the new image as soon as it catches them, or this one when the
//!    restart failed.
//! 3. The requests that arrived since the last look are taken ([`super::request`]), and the timer
//!    asks too once `[spawn] tick_minutes` have passed since the last tick ended. Nothing asked:
//!    the loop pauses (about a second) and looks again.
//! 4. While another process holds `spawn.lock` (a `zaf spawn phase` or `zaf spawn stop` between
//!    its read and its write), the tick waits, its requests kept, and the loop says so once.
//! 5. One tick runs for every request taken since the last tick began, its block naming who asked
//!    ([`Asked::line`]), and one tick never overlaps another.
//!
//! The loop keeps what it learned between ticks in [`Memory`], a cache and never a source: each
//! tick gathers the facts again and the facts win, and a tick that cannot read them keeps the last
//! placement, starts nothing, and the loop goes on. Everything kept names the fact it is rebuilt
//! from, so a restart loses nothing.
//!
//! The module is pure: the world it acts in is its [`Host`], which the CLI wires and a test stands
//! in for.

use std::time::{Duration, Instant, SystemTime};

use crate::domain::schedule::Placement;

/// What the scheduler keeps between two ticks. A cache: each tick gathers the facts again, and
/// what a tick read replaces what is kept; nothing here is ever read to decide what a tick does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    /// Where every ticket sat at the last tick that could read the facts. Rebuilt from the
    /// placement facts every tick gathers: the board's open tickets, epics and questions, the
    /// forge's pull requests and checks, and the phase run folders (docs/zaf-actions.md §5.1).
    placement: Option<Placement>,
    /// When the facts behind [`Memory::placement`] were gathered: the start of that tick. Rebuilt
    /// from the next tick that reads the facts, and from the trace, whose last line with tickets
    /// carries the same time.
    gathered_at: Option<SystemTime>,
}

impl Memory {
    /// Keeps the placement a tick that read the facts at `at` acted on, in place of the last.
    pub fn keep(&mut self, placement: Placement, at: SystemTime) {
        self.placement = Some(placement);
        self.gathered_at = Some(at);
    }

    /// The placement of the last tick that read the facts; none before the first.
    pub fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }

    /// When the last tick that read the facts gathered them; none before the first.
    pub fn gathered_at(&self) -> Option<SystemTime> {
        self.gathered_at
    }
}

/// Who asked for one tick: the requests taken since the last tick began, in the order they came.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    who: Vec<String>,
}

/// How many askers the block names before it counts the rest.
pub const NAMED: usize = 8;

impl Asked {
    /// Who asked, each once, in the order first asked.
    pub fn new(who: impl IntoIterator<Item = String>) -> Self {
        let mut once: Vec<String> = Vec::new();
        for who in who {
            if !once.contains(&who) {
                once.push(who);
            }
        }
        Self { who: once }
    }

    /// Who asked, each once.
    pub fn who(&self) -> &[String] {
        &self.who
    }

    /// The block's line: `asked by: phase-end implement #7, ruling #45`, the first [`NAMED`] and
    /// then how many more.
    pub fn line(&self) -> String {
        let mut named: Vec<&str> = self.who.iter().take(NAMED).map(String::as_str).collect();
        let more = self.who.len().saturating_sub(NAMED);
        let rest = format!("and {more} more");
        if more > 0 {
            named.push(&rest);
        }
        format!("asked by: {}", named.join(", "))
    }
}

/// What a look at the binary found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Nothing new, or not yet.
    Nothing,
    /// A new binary that answers: its version (`zaf 0.3.2`).
    Restart(String),
    /// A new binary that does not run, and why, in words.
    Refused(String),
}

/// How the loop ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// A stop signal arrived.
    Stopped {
        /// The signal's name.
        signal: String,
        /// Who asked for a tick that never ran (it waited for `spawn.lock`, or the stop came
        /// before it), for the next scheduler to serve.
        unserved: Vec<String>,
    },
    /// The request file could not be used when the loop began, and nothing was taken from it;
    /// why, in words.
    Failed(String),
}

/// The world the loop acts in.
pub trait Host {
    /// From now on a stop signal is recorded for [`Host::stop`] and ends nothing at once, and one
    /// held back before the restart that started this image is let through, to be recorded.
    fn catch_stop(&mut self);
    /// Takes the requests an earlier scheduler left in the request file: who asked, oldest first.
    /// A file that cannot be used is the error, with nothing taken.
    fn left(&mut self) -> Result<Vec<String>, String>;
    /// Says the scheduler is up.
    fn up(&mut self);
    /// A monotonic now, for the timer.
    fn now(&mut self) -> Instant;
    /// The stop signal that has arrived, by name; none while none has.
    fn stop(&mut self) -> Option<String>;
    /// Looks at the binary once.
    fn binary(&mut self) -> Found;
    /// Holds the stop signals back, pending: from before the last look for one to the restart,
    /// which keeps them pending for the new image.
    fn hold_stop(&mut self);
    /// Lets the stop signals held back through: one that came meanwhile is recorded now.
    fn let_stop_through(&mut self);
    /// Starts again as the new binary; returns only when that failed, with why.
    fn restart(&mut self) -> String;
    /// The requests that arrived since the last look: who asked, oldest first.
    fn requests(&mut self) -> Vec<String>;
    /// Whether another process holds `spawn.lock` now.
    fn held(&mut self) -> bool;
    /// Runs tick number `number` for `asked`, with what the loop keeps, and gives its exit code.
    fn tick(&mut self, number: u64, asked: &Asked, memory: &mut Memory) -> u8;
    /// Says when the timer asks next, `interval` after the tick that just ended.
    fn next_tick(&mut self, interval: Duration);
    /// Says that the tick waits for `spawn.lock`.
    fn waits_for_lock(&mut self);
    /// Waits about a second, or less when a stop signal arrives.
    fn pause(&mut self);
    /// A line for the person watching.
    fn say(&mut self, line: &str);
    /// A line for stderr.
    fn warn(&mut self, line: &str);
}

/// The timer's words in [`Asked`].
pub const TIMER: &str = "timer";

/// Runs the loop until a stop signal arrives (see the module above): the first tick at once, for
/// `first` (who asked before the loop began: `start` or `restart`) and the requests left in the
/// file, then one for every turn something asked, the timer each `interval` after the last tick
/// ended among them. A request file that cannot be used at the start ends it before any tick.
pub fn run(host: &mut dyn Host, interval: Duration, first: Vec<String>) -> Ended {
    // First of all, before anything is taken from the request file: a stop from here on ends the
    // loop at its next look, handing every request back, and one held back across the restart
    // that started this image reaches that look too.
    host.catch_stop();
    let mut pending = first;
    match host.left() {
        Ok(left) => pending.extend(left),
        Err(why) => return Ended::Failed(why),
    }
    host.up();
    let mut memory = Memory::default();
    let mut timer = host.now() + interval;
    let mut ticks = 0;
    let mut waiting = false;
    loop {
        if let Some(signal) = host.stop() {
            return Ended::Stopped {
                signal,
                unserved: pending,
            };
        }
        // Only with nothing taken and not yet ticked: a restart loses no request.
        if pending.is_empty() {
            match host.binary() {
                Found::Nothing => {}
                Found::Restart(version) => {
                    // Held back from here through the exec, which keeps a stop pending for the
                    // new image: none that lands between this last look and the exec is lost.
                    host.hold_stop();
                    // A signal that came while the new binary answered stops rather than restarts.
                    if let Some(signal) = host.stop() {
                        host.let_stop_through();
                        return Ended::Stopped {
                            signal,
                            unserved: pending,
                        };
                    }
                    host.say(&format!("restarting as {version}"));
                    let why = host.restart();
                    // Still this image: a stop that came meanwhile ends the loop at its next look.
                    host.let_stop_through();
                    host.warn(&format!("{why}; this image goes on"));
                }
                Found::Refused(why) => host.warn(&why),
            }
        }
        pending.extend(host.requests());
        if pending.is_empty() && host.now() >= timer {
            pending.push(TIMER.to_owned());
        }
        if pending.is_empty() {
            host.pause();
            continue;
        }
        if host.held() {
            if !waiting {
                host.waits_for_lock();
                waiting = true;
            }
            host.pause();
            continue;
        }
        waiting = false;
        ticks += 1;
        let asked = Asked::new(std::mem::take(&mut pending));
        let code = host.tick(ticks, &asked, &mut memory);
        if code != 0 {
            host.warn(&format!(
                "tick {ticks} failed with exit {code}; the watch goes on"
            ));
        }
        timer = host.now() + interval;
        host.next_tick(interval);
    }
}

#[cfg(test)]
mod tests;
