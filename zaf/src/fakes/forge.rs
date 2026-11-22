//! The in-memory forge.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use super::calls::Calls;
use crate::domain::RepoPath;
use crate::ports::{
    AccessCheck, BaseName, BranchChecks, BranchName, ChangedFiles, Check, CheckBucket, CheckLink,
    CheckLog, CheckRun, CommitId, Forge, MergeOutcome, MergeStrategy, NewPullRequest, Opened,
    PortError, PrNumber, PrRef, PullRequest, ReviewComment, ScanningAlert, SliceName,
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
    files_cut_short: bool,
    merged: bool,
    closed: bool,
    merged_with: Option<MergeStrategy>,
    runs: Option<Vec<CheckRun>>,
    handoff: Option<String>,
    opened_at: SystemTime,
    merged_at: Option<SystemTime>,
    mergeable: Option<bool>,
}

#[derive(Debug, Default)]
struct State {
    pulls: BTreeMap<PrNumber, OpenPull>,
    next_number: u64,
    merged_fails: bool,
    handoff_fails: bool,
    merge_refusal: Option<PortError>,
    logs: BTreeMap<CheckLink, CheckLog>,
    branches: BTreeMap<BranchName, BranchChecks>,
    branch_reads: usize,
    branch_checks_fail: bool,
    default_branch: Option<BranchName>,
}

/// A forge held in memory. Arrange reviews with [`FakeForge::set_checks`]
/// [`FakeForge::add_review_comment`], [`FakeForge::set_head`], [`FakeForge::set_changed_files`] and [`FakeForge::set_scanning_alerts`].
#[derive(Debug, Default)]
pub struct FakeForge {
    state: Mutex<State>,
    calls: Calls,
}

impl FakeForge {
    /// How many times the port's method `method` was called (`checks`, `head`, ...); 0 for one
    /// never called. Only calls through [`Forge`] count, not the methods that arrange a scene.
    pub fn calls(&self, method: &str) -> usize {
        self.calls.of(method)
    }

    /// Sets every count [`FakeForge::calls`] reads to 0.
    pub fn reset_calls(&self) {
        self.calls.reset();
    }

    /// Replaces a pull request's check runs.
    pub fn set_checks(&self, pr: PrNumber, checks: Vec<Check>) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.checks = checks;
        Ok(())
    }

    /// Puts `branch` at `head` with `checks` there, as [`Forge::branch_checks`] then reads it; a
    /// branch never set is one the forge does not have.
    pub fn set_branch(&self, branch: &str, head: &str, checks: Vec<Check>) {
        self.state().branches.insert(
            BranchName::from(branch),
            BranchChecks {
                head: Some(CommitId::from(head)),
                checks,
                reached_at: None,
            },
        );
    }

    /// Sets the branch [`Forge::default_branch`] answers; `main` until then.
    pub fn set_default_branch(&self, branch: &str) {
        self.state().default_branch = Some(BranchName::from(branch));
    }

    /// Makes every [`Forge::branch_checks`] read fail with a backend error.
    pub fn fail_branch_checks(&self) {
        self.state().branch_checks_fail = true;
    }

    /// How many times [`Forge::branch_checks`] was asked.
    pub fn branch_reads(&self) -> usize {
        self.state().branch_reads
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

    /// Replaces the files a pull request changes, in the order its diff lists them; a rename
    /// is listed under both names, the old one first. The list is complete.
    pub fn set_changed_files(&self, pr: PrNumber, files: &[&str]) -> Result<(), PortError> {
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        pull.files = files.iter().map(|file| (*file).to_owned()).collect();
        pull.files_cut_short = false;
        Ok(())
    }

    /// Says whether the list of files a pull request changes may be cut short.
    pub fn set_changed_files_cut_short(
        &self,
        pr: PrNumber,
        cut_short: bool,
    ) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.files_cut_short = cut_short;
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

    /// Sets whether the forge says a pull request merges into its base: true, false, or none for
    /// a forge that does not say; a new one says none.
    pub fn set_mergeable(&self, pr: PrNumber, mergeable: Option<bool>) -> Result<(), PortError> {
        let mut state = self.state();
        state.pull_mut(pr)?.mergeable = mergeable;
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
            return Opened::AlreadyOpen(PrRef::new(number, link_of(number)));
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
            files_cut_short: false,
            merged: false,
            closed: false,
            merged_with: None,
            runs: None,
            handoff: None,
            opened_at: SystemTime::UNIX_EPOCH,
            merged_at: None,
            mergeable: None,
        };
        state.pulls.insert(number, pull);
        Opened::New(PrRef::new(number, link_of(number)))
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

/// The page the fake forge shows a pull request on.
pub fn link_of(pr: PrNumber) -> String {
    format!("https://forge.test/pulls/{}", pr.get())
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
        self.calls.record("checks");
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.checks.clone())
    }

    fn branch_checks(&self, branch: &BranchName) -> Result<BranchChecks, PortError> {
        self.calls.record("branch_checks");
        let mut state = self.state();
        state.branch_reads += 1;
        if state.branch_checks_fail {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        Ok(state.branches.get(branch).cloned().unwrap_or(BranchChecks {
            head: None,
            checks: Vec::new(),
            reached_at: None,
        }))
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        self.calls.record("review_comments");
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.comments.clone())
    }

    fn checks_at(&self, pr: PrNumber, _head: Option<&str>) -> Result<Vec<Check>, PortError> {
        self.checks(pr)
    }

    fn pull_request(&self, pr: PrNumber) -> Result<PullRequest, PortError> {
        self.calls.record("pull_request");
        let mut state = self.state();
        if state.merged_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        let pull = state.pull_mut(pr)?;
        Ok(PullRequest {
            opened_at: pull.opened_at,
            merged_at: pull.merged_at,
            mergeable: pull.mergeable,
            head: pull.head.clone(),
            merged: pull.merged,
        })
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.calls.record("head");
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.head.clone())
    }

    fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
        self.calls.record("merged");
        let mut state = self.state();
        if state.merged_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        Ok(state.pull_mut(pr)?.merged)
    }

    fn changed_files(&self, pr: PrNumber) -> Result<ChangedFiles, PortError> {
        self.calls.record("changed_files");
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        Ok(ChangedFiles {
            paths: pull.files.iter().cloned().map(RepoPath::from).collect(),
            cut_short: pull.files_cut_short,
        })
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        self.calls.record("scanning_alerts");
        let mut state = self.state();
        Ok(state.pull_mut(pr)?.alerts.clone())
    }

    fn handoff(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.calls.record("handoff");
        let mut state = self.state();
        if state.handoff_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        Ok(state.pull_mut(pr)?.handoff.clone())
    }

    fn set_handoff(&self, pr: PrNumber, text: &str) -> Result<(), PortError> {
        self.calls.record("set_handoff");
        let mut state = self.state();
        if state.handoff_fails {
            return Err(PortError::Backend("the forge is down".to_owned()));
        }
        state.pull_mut(pr)?.handoff = Some(text.to_owned());
        Ok(())
    }

    /// `main`, unless [`FakeForge::set_default_branch`] arranged another.
    fn default_branch(&self) -> Result<BranchName, PortError> {
        self.calls.record("default_branch");
        Ok(self
            .state()
            .default_branch
            .clone()
            .unwrap_or_else(|| BranchName::from("main")))
    }

    /// The head becomes the slice and the base its line, as [`PrRequest`] holds them.
    fn open_pull_request(&self, request: &NewPullRequest) -> Result<Opened, PortError> {
        self.calls.record("open_pull_request");
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
        self.calls.record("merge_pull_request");
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
        self.calls.record("check_runs");
        let mut state = self.state();
        let pull = state.pull_mut(pr)?;
        Ok(pull
            .runs
            .clone()
            .unwrap_or_else(|| pull.checks.iter().map(run_of).collect()))
    }

    fn access(&self) -> Vec<AccessCheck> {
        self.calls.record("access");
        Vec::new()
    }

    fn check_log(&self, link: &CheckLink) -> Result<CheckLog, PortError> {
        self.calls.record("check_log");
        Ok(self
            .state()
            .logs
            .get(link)
            .cloned()
            .unwrap_or(CheckLog::NoJob))
    }
}

#[cfg(test)]
mod tests;
