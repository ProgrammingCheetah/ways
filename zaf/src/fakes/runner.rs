//! The replaying runner: recorded transcripts stand in for real runs, at zero tokens.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::RoleName;
use crate::ports::{PortError, RunOutput, RunRequest, Runner};

#[derive(Debug, Default)]
struct State {
    recorded: BTreeMap<RoleName, VecDeque<RunOutput>>,
    requests: Vec<RunRequest>,
}

/// A runner that replays recorded outputs per role, in the order recorded,
/// and keeps every request it received.
#[derive(Debug, Default)]
pub struct FakeRunner {
    state: Mutex<State>,
}

impl FakeRunner {
    /// Queues an output for the next run of a role.
    pub fn record(&self, role: RoleName, output: RunOutput) {
        self.state()
            .recorded
            .entry(role)
            .or_default()
            .push_back(output);
    }

    /// Every request received, oldest first.
    pub fn requests(&self) -> Vec<RunRequest> {
        self.state().requests.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Runner for FakeRunner {
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError> {
        let mut state = self.state();
        state.requests.push(request.clone());
        state
            .recorded
            .get_mut(&request.role)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| PortError::Unavailable(format!("no recorded run for {}", request.role)))
    }
}
