//! The in-memory forge.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{
    Check, Forge, PortError, PrMerge, PrNumber, PrRequest, ReviewComment, ScanningAlert,
};

#[derive(Debug, Clone)]
struct PullRequest {
    request: PrRequest,
    checks: Vec<Check>,
    comments: Vec<ReviewComment>,
    alerts: Vec<ScanningAlert>,
    head: Option<String>,
    merged: bool,
}

#[derive(Debug, Default)]
struct State {
    pulls: BTreeMap<PrNumber, PullRequest>,
    next_number: u64,
}

/// A forge held in memory. Arrange reviews with [`FakeForge::set_checks`]
/// [`FakeForge::add_review_comment`], [`FakeForge::set_head`] and [`FakeForge::set_scanning_alerts`].
#[derive(Debug, Default)]
pub struct FakeForge {
    state: Mutex<State>,
}

impl FakeForge {
    /// Replaces a pull request's check runs.
    pub fn set_checks(&self, pr: PrNumber, checks: Vec<Check>) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.checks = checks;
        Ok(())
    }

    /// Adds a review comment to a pull request.
    pub fn add_review_comment(
        &self,
        pr: PrNumber,
        comment: ReviewComment,
    ) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.comments.push(comment);
        Ok(())
    }

    /// Replaces a pull request's open code scanning alerts.
    pub fn set_scanning_alerts(
        &self,
        pr: PrNumber,
        alerts: Vec<ScanningAlert>,
    ) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.alerts = alerts;
        Ok(())
    }

    /// Sets the commit id a pull request's head is at; a new one reports none.
    pub fn set_head(&self, pr: PrNumber, head: Option<&str>) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.head = head.map(str::to_owned);
        Ok(())
    }

    /// The request a pull request was opened with.
    pub fn request(&self, pr: PrNumber) -> Option<PrRequest> {
        self.state().pulls.get(&pr).map(|pull| pull.request.clone())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    fn pull_mut(&mut self, pr: PrNumber) -> Result<&mut PullRequest, PortError> {
        self.pulls
            .get_mut(&pr)
            .ok_or_else(|| PortError::NotFound(format!("pull request {pr}")))
    }
}

impl Forge for FakeForge {
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        let mut state = self.state();
        let existing = state
            .pulls
            .iter()
            .find(|(_, pull)| !pull.merged && pull.request.slice == request.slice)
            .map(|(number, _)| *number);
        if let Some(number) = existing {
            return Ok(number);
        }
        state.next_number += 1;
        let number = PrNumber::from(state.next_number);
        let pull = PullRequest {
            request: request.clone(),
            checks: Vec::new(),
            comments: Vec::new(),
            alerts: Vec::new(),
            head: None,
            merged: false,
        };
        state.pulls.insert(number, pull);
        Ok(number)
    }

    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.checks.clone())
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.comments.clone())
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.head.clone())
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.alerts.clone())
    }

    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError> {
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        if pull.merged {
            return Ok(PrMerge::AlreadyMerged);
        }
        pull.merged = true;
        Ok(PrMerge::Merged)
    }
}
