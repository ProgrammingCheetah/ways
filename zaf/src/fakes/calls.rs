//! The count of port calls a fake answers, by method name.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// How many times each method of a port was called. It has a lock of its own, so counting
/// never waits on the fake's state and calls from many threads are all counted.
#[derive(Debug, Default)]
pub(super) struct Calls {
    counts: Mutex<BTreeMap<&'static str, usize>>,
}

impl Calls {
    /// Counts one call of `method`.
    pub(super) fn record(&self, method: &'static str) {
        *self
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(method)
            .or_default() += 1;
    }

    /// How many times `method` was called; 0 for one never called.
    pub(super) fn of(&self, method: &str) -> usize {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(method)
            .copied()
            .unwrap_or(0)
    }

    /// Sets every count to 0.
    pub(super) fn reset(&self) {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}
