//! The in-memory gate.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{Gate, GateLevel, GateRun, PortError};

#[derive(Debug, Default)]
struct State {
    outcomes: BTreeMap<GateLevel, GateRun>,
    runs: Vec<GateLevel>,
}

/// A gate held in memory: each declared level returns the run set for it.
#[derive(Debug, Default)]
pub struct FakeGate {
    state: Mutex<State>,
}

impl FakeGate {
    /// Declares a level and what running it returns.
    pub fn declare(&self, level: GateLevel, run: GateRun) {
        self.state().outcomes.insert(level, run);
    }

    /// Every level run, oldest first.
    pub fn runs(&self) -> Vec<GateLevel> {
        self.state().runs.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Gate for FakeGate {
    fn run(&self, level: GateLevel) -> Result<GateRun, PortError> {
        let mut state = self.state();
        state.runs.push(level);
        state
            .outcomes
            .get(&level)
            .cloned()
            .ok_or_else(|| PortError::Unavailable(format!("the {level} gate is not declared")))
    }
}
