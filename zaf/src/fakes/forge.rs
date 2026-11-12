//! The in-memory forge.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use crate::ports::{
    BaseName, Check, CheckBucket, CheckLink, CheckLog, CheckRun, CommitId, Forge, MergeOutcome,
    MergeStrategy, NewPullRequest, Opened, PortError, PrNumber, PullRequest, ReviewComment,
    ScanningAlert, SliceName,
};

/// What a pull request is opened from and into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRequest {
    /// The slice it carries.
    pub slice: SliceName,
    /// The line it targets.
    pub base: BaseName,
    /// Its title.
    pub title: String,
    /// Its body.
    pub body: String,
}

#[derive(Debug, Clone)]
struct OpenPull {
    request: PrRequest,
    checks: Vec<Check>,
    comments: Vec<ReviewComment>,
    alerts: Vec<ScanningAlert>,
    head: Option<String>,
    files: Vec<String>,
    merged: bool,
    closed: bool,
    merged_with: Option<MergeStrategy>,
    runs: Option<Vec<CheckRun>>,
    handoff: Option<String>,
    opened_at: SystemTime,
    merged_at: Option<SystemTime>,
}

#[derive(Debug, Default)]
struct State {
    pulls: BTreeMap<PrNumber, OpenPull>,
    next_number: u64,
    merged_fails: bool,
    handoff_fails: bool,
    merge_refusal: Option<PortError>,
    logs: BTreeMap<CheckLink, CheckLog>,
}

/// A forge held in memory. Arrange reviews with [`FakeForge::set_checks`]
/// [`FakeForge::add_review_comment`], [`FakeForge::set_head`], [`FakeForge::set_changed_files`] and [`FakeForge::set_scanning_alerts`].
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

    /// Replaces the files a pull request changes, in the order its diff lists them.
    pub fn set_changed_files(&self, pr: PrNumber, files: &[&str]) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.files = files.iter().map(|file| (*file).to_owned()).collect();
        Ok(())
    }

    /// Sets the time a pull request opened; a new one opened at the epoch.
    pub fn set_opened_at(&self, pr: PrNumber, at: SystemTime) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.opened_at = at;
        Ok(())
    }

    /// Sets the time a pull request merged; [`FakeForge::merge`] sets it to the epoch when none
    /// was arranged first.
    pub fn set_merged_at(&self, pr: PrNumber, at: SystemTime) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.merged_at = Some(at);
        Ok(())
    }

    /// Makes every [`Forge::merged`] read fail with a backend error, as a forge that is down does.
    pub fn fail_merged_reads(&self) {
        self.state().merged_fails = true;
    }

    /// Makes every [`Forge::handoff`] and [`Forge::set_handoff`] call fail with a backend error.
    pub fn fail_handoffs(&self) {
        self.state().handoff_fails = true;
    }

    /// The request a pull request was opened with.
    pub fn request(&self, pr: PrNumber) -> Option<PrRequest> {
        self.state().pulls.get(&pr).map(|pull| pull.request.clone())
    }

    /// Closes a pull request unmerged.
    pub fn close(&self, pr: PrNumber) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.closed = true;
        Ok(())
    }

    /// The strategy [`Forge::merge_pull_request`] merged a pull request with; none when the port
    /// did not merge it.
    pub fn merged_with(&self, pr: PrNumber) -> Option<MergeStrategy> {
        self.state()
            .pulls
            .get(&pr)
            .and_then(|pull| pull.merged_with)
    }

    /// Replaces the checks [`Forge::check_runs`] lists for a pull request; until then it lists
    /// the checks [`FakeForge::set_checks`] arranged, with no link.
    pub fn set_check_runs(&self, pr: PrNumber, runs: Vec<CheckRun>) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.runs = Some(runs);
        Ok(())
    }

    /// Sets what [`Forge::check_log`] answers for `link`; a link with none arranged names no job.
    pub fn set_check_log(&self, link: CheckLink, log: CheckLog) {
        self.state().logs.insert(link, log);
    }

    /// Makes every [`Forge::merge_pull_request`] of an open pull request fail with `error`.
    pub fn refuse_merges(&self, error: PortError) {
        self.state().merge_refusal = Some(error);
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    fn pull_mut(&mut self, pr: PrNumber) -> Result<&mut OpenPull, PortError> {
        self.pulls
            .get_mut(&pr)
            .ok_or_else(|| PortError::NotFound(format!("pull request {pr}")))
    }
}

impl FakeForge {
    /// Opens a pull request for a slice; a slice with an open one gets that one back. It
    /// arranges a scene the way [`Forge::open_pull_request`] opens one.
    pub fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        Ok(self.open(request).number())
    }

    /// Opens a pull request for a slice, or finds the one open for it.
    fn open(&self, request: &PrRequest) -> Opened {
        let mut state = self.state();
        let existing = state
            .pulls
            .iter()
            .find(|(_, pull)| !pull.merged && !pull.closed && pull.request.slice == request.slice)
            .map(|(number, _)| *number);
        if let Some(number) = existing {
            return Opened::AlreadyOpen(number);
        }
        state.next_number += 1;
        let number = PrNumber::from(state.next_number);
        let pull = OpenPull {
            request: request.clone(),
            checks: Vec::new(),
            comments: Vec::new(),
            alerts: Vec::new(),
            head: None,
            files: Vec::new(),
            merged: false,
            closed: false,
            merged_with: None,
            runs: None,
            handoff: None,
            opened_at: SystemTime::UNIX_EPOCH,
            merged_at: None,
        };
        state.pulls.insert(number, pull);
        Opened::New(number)
    }

    /// Merges the pull request; one already merged is [`MergeOutcome::AlreadyMerged`]. It
    /// arranges a scene: [`Forge::merge_pull_request`] is the port's merge, with its refusals.
    pub fn merge(&self, pr: PrNumber) -> Result<MergeOutcome, PortError> {
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        if pull.merged {
            return Ok(MergeOutcome::AlreadyMerged);
        }
        pull.merged = true;
        pull.merged_at.get_or_insert(SystemTime::UNIX_EPOCH);
        Ok(MergeOutcome::Merged)
    }
}

/// A check as `zaf forge checks` lists it, from its state alone.
fn run_of(check: &Check) -> CheckRun {
    let bucket = match check.state {
        crate::ports::CheckState::Pending => CheckBucket::Pending,
        crate::ports::CheckState::Passed => CheckBucket::Pass,
        crate::ports::CheckState::Failed => CheckBucket::Fail,
    };
    CheckRun {
        bucket,
        name: check.name.clone(),
        link: None,
    }
}

impl Forge for FakeForge {
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.checks.clone())
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.comments.clone())
    }

    fn pull_request(&self, pr: PrNumber) -> Result<PullRequest, PortError> {
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        Ok(PullRequest {
            opened_at: pull.opened_at,
            merged_at: pull.merged_at,
        })
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.head.clone())
    }

    fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
        let mut state = self.state();
        if state.merged_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        Ok(state.pull_mut(pr)?.merged)
    }

    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.files.last().cloned())
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.alerts.clone())
    }

    fn handoff(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let mut state = self.state();
        if state.handoff_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        Ok(state.pull_mut(pr)?.handoff.clone())
    }

    fn set_handoff(&self, pr: PrNumber, text: &str) -> Result<(), PortError> {
        let mut state = self.state();
        if state.handoff_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        state.pull_mut(pr)?.handoff = Some(text.to_owned());
        Ok(())
    }

    /// The head becomes the slice and the base its line, as [`PrRequest`] holds them.
    fn open_pull_request(&self, request: &NewPullRequest) -> Result<Opened, PortError> {
        let request = PrRequest {
            slice: SliceName::from(request.head.as_str()),
            base: BaseName::from(request.base.as_str()),
            title: request.title.to_string(),
            body: request.body.to_string(),
        };
        Ok(self.open(&request))
    }

    fn merge_pull_request(
        &self,
        pr: PrNumber,
        strategy: MergeStrategy,
        head: Option<&CommitId>,
    ) -> Result<MergeOutcome, PortError> {
        let mut state = self.state();
        let refusal = state.merge_refusal.clone();
        let pull = state.pull_mut(pr)?;
        if pull.merged {
            return Ok(MergeOutcome::AlreadyMerged);
        }
        if pull.closed {
            return Err(PortError::Rejected(format!(
                "pull request {pr} is closed unmerged"
            )));
        }
        if let Some(pin) = head
            && pull.head.as_deref() != Some(pin.as_str())
        {
            let at = pull.head.as_deref().unwrap_or("no commit");
            return Err(PortError::Rejected(format!(
                "pull request {pr}'s head is {at}, not {pin}"
            )));
        }
        if let Some(error) = refusal {
            return Err(error);
        }
        pull.merged = true;
        pull.merged_with = Some(strategy);
        pull.merged_at.get_or_insert(SystemTime::UNIX_EPOCH);
        Ok(MergeOutcome::Merged)
    }

    fn check_runs(&self, pr: PrNumber) -> Result<Vec<CheckRun>, PortError> {
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        Ok(pull
            .runs
            .clone()
            .unwrap_or_else(|| pull.checks.iter().map(run_of).collect()))
    }

    fn check_log(&self, link: &CheckLink) -> Result<CheckLog, PortError> {
        Ok(self
            .state()
            .logs
            .get(link)
            .cloned()
            .unwrap_or(CheckLog::NoJob))
    }
}
