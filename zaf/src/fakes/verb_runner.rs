//! The fake verb runner: queued outcomes stand in for real runs, and every run is kept.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{Cancel, Tick};
use crate::ports::{VerbOutcome, VerbRun, VerbRunner};

#[derive(Debug, Default)]
struct State {
    outcomes: VecDeque<VerbOutcome>,
    ticks: Vec<Tick>,
    runs: Vec<VerbRun>,
    stamp: Option<String>,
}

/// A verb runner that returns the outcomes queued, in order, and keeps every run it was given.
/// With none queued, a run succeeds with no output.
#[derive(Debug, Default)]
pub struct FakeVerbRunner {
    state: Mutex<State>,
}

impl FakeVerbRunner {
    /// Queues the outcome of the next run.
    pub fn record(&self, outcome: VerbOutcome) {
        self.state().outcomes.push_back(outcome);
    }

    /// Queues a tick every run reports before it returns.
    pub fn tick_with(&self, tick: Tick) {
        self.state().ticks.push(tick);
    }

    /// Sets the stamp the binary reports from now on (none, by default).
    pub fn stamp_with(&self, stamp: Option<&str>) {
        self.state().stamp = stamp.map(str::to_owned);
    }

    /// Every run received, oldest first.
    pub fn runs(&self) -> Vec<VerbRun> {
        self.state().runs.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl VerbRunner for FakeVerbRunner {
    fn binary_stamp(&self) -> Option<String> {
        self.state().stamp.clone()
    }

    fn run(&self, run: VerbRun, tick: &dyn Fn(Tick), cancel: &Cancel) -> VerbOutcome {
        let (outcome, ticks) = {
            let mut state = self.state();
            state.runs.push(run);
            (state.outcomes.pop_front(), state.ticks.clone())
        };
        for each in ticks {
            tick(each);
        }
        if cancel.is_cancelled() {
            return VerbOutcome {
                stdout: String::new(),
                stderr: "cancelled".into(),
                exit: 2,
            };
        }
        outcome.unwrap_or(VerbOutcome {
            stdout: String::new(),
            stderr: String::new(),
            exit: 0,
        })
    }
}
