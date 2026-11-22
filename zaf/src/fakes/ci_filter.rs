//! The in-memory CI filter.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{PullRequestFilter, RepoPath, WorkflowFilter};
use crate::ports::{BranchName, CiFilter, PortError};

#[derive(Debug, Default)]
struct State {
    branches: BTreeMap<BranchName, BTreeMap<RepoPath, PullRequestFilter>>,
    failure: Option<String>,
    reads: usize,
}

/// Workflow files held per branch, as [`CiFilter::workflows`] reads them: only the branch asked
/// for, so a workflow set on a pull request's head is never seen at its base.
#[derive(Debug, Default)]
pub struct FakeCiFilter {
    state: Mutex<State>,
}

impl FakeCiFilter {
    /// Puts the workflow at `path` on `branch` with `filter`, replacing one at the same path.
    pub fn set_workflow(&self, branch: &str, path: &str, filter: PullRequestFilter) {
        self.state()
            .branches
            .entry(BranchName::from(branch))
            .or_default()
            .insert(RepoPath::from(path), filter);
    }

    /// Puts a workflow on `branch` that cannot be read, as the forge refusing the file.
    pub fn set_unreadable(&self, branch: &str, path: &str) {
        self.set_workflow(branch, path, PullRequestFilter::Unsure);
    }

    /// Fails every later list with a backend error saying `why`.
    pub fn fail(&self, why: &str) {
        self.state().failure = Some(why.to_owned());
    }

    /// How many times [`CiFilter::workflows`] was asked, a failing ask included.
    pub fn reads(&self) -> usize {
        self.state().reads
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CiFilter for FakeCiFilter {
    fn workflows(&self, base: &BranchName) -> Result<Vec<WorkflowFilter>, PortError> {
        let mut state = self.state();
        state.reads += 1;
        if let Some(why) = &state.failure {
            return Err(PortError::Backend(why.clone()));
        }
        Ok(state
            .branches
            .get(base)
            .into_iter()
            .flatten()
            .map(|(path, filter)| WorkflowFilter {
                path: path.clone(),
                filter: filter.clone(),
            })
            .collect())
    }
}
