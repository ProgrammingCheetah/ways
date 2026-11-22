//! What the chair is about to write, held for a few seconds so it can be taken back (D7).
//!
//! [`PendingQueue`] holds actions of any kind, each due [`UNDO_WINDOW`] after it was queued. The
//! newest is the one `u` takes back; the ones whose time has come are taken out to be written;
//! quitting takes out all of them at once, to be written before the chair closes. The clock is
//! handed in, so the queue itself never sleeps and a test never waits.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long an action waits before it is written: the time `u` has to take it back.
pub const UNDO_WINDOW: Duration = Duration::from_secs(5);

/// An action and when it is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending<A> {
    /// What is to be written.
    pub action: A,
    /// When it is written, unless it is taken back first.
    pub due: Instant,
}

impl<A> Pending<A> {
    /// The whole seconds left at `now` before it is due, rounded up: what a countdown shows.
    pub fn seconds_left(&self, now: Instant) -> u64 {
        let left = self.due.saturating_duration_since(now);
        left.as_secs() + u64::from(left.subsec_nanos() > 0)
    }
}

/// Actions waiting to be written, oldest first.
#[derive(Debug, Clone)]
pub struct PendingQueue<A> {
    delay: Duration,
    items: VecDeque<Pending<A>>,
}

impl<A> Default for PendingQueue<A> {
    /// A queue whose actions wait [`UNDO_WINDOW`].
    fn default() -> Self {
        Self::new(UNDO_WINDOW)
    }
}

impl<A> PendingQueue<A> {
    /// An empty queue whose actions wait `delay`.
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            items: VecDeque::new(),
        }
    }

    /// Queues `action` at `now`: it is due `delay` later.
    pub fn push(&mut self, action: A, now: Instant) {
        self.items.push_back(Pending {
            action,
            due: now + self.delay,
        });
    }

    /// Takes back the newest action, which is then never written.
    pub fn undo(&mut self) -> Option<A> {
        self.items.pop_back().map(|pending| pending.action)
    }

    /// Takes out every action due at `now`, oldest first, for the caller to write.
    pub fn take_due(&mut self, now: Instant) -> Vec<A> {
        let mut due = Vec::new();
        while self.items.front().is_some_and(|pending| pending.due <= now) {
            if let Some(pending) = self.items.pop_front() {
                due.push(pending.action);
            }
        }
        due
    }

    /// Takes out every action, due or not, oldest first: what quitting writes before it closes.
    pub fn drain(&mut self) -> Vec<A> {
        self.items.drain(..).map(|pending| pending.action).collect()
    }

    /// The newest action, the one `u` would take back.
    pub fn newest(&self) -> Option<&Pending<A>> {
        self.items.back()
    }

    /// Every waiting action, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Pending<A>> {
        self.items.iter()
    }

    /// Whether nothing waits.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many actions wait.
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests;
