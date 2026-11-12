//! The forge: pull requests, their checks and review comments, and the merge.
//!
//! zaf reads the forge for the scheduler and the briefs, and writes what the standard actions
//! hand it through `zaf forge`: a pull request opened for a branch, a merge, the hand-off
//! comment. Every forge's words fold into the ones here, so one set of actions serves them all.

use std::str::FromStr;
use std::time::SystemTime;

use super::{AccessCheck, CommitId, PortError};
use crate::domain::Location;
use crate::newtype::text_newtype;

/// The number of a pull request on the forge.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::From,
    derive_more::Display,
)]
#[display("#{_0}")]
pub struct PrNumber(u64);

impl PrNumber {
    /// The bare number, for adapters that address pull requests numerically.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A pull request number as a person writes it: plain digits, with or without the `#`.
impl FromStr for PrNumber {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits = text.strip_prefix('#').unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("`{text}` is not a pull request number"));
        }
        digits
            .parse::<u64>()
            .map(Self)
            .map_err(|_| format!("`{text}` is not a pull request number"))
    }
}

text_newtype! {
    /// A branch on the forge: the head a pull request carries or the base it merges into.
    BranchName
}

impl BranchName {
    /// Whether it can be handed to a forge as a branch: not empty and not read as an option.
    pub fn is_usable(&self) -> bool {
        !self.0.is_empty() && !self.0.starts_with('-')
    }
}

text_newtype! {
    /// A pull request's title.
    PrTitle
}

text_newtype! {
    /// A pull request's description.
    PrBody
}

text_newtype! {
    /// The page a check run shows on (a CI job, an external build).
    CheckLink
}

text_newtype! {
    /// The id of a CI job whose log the forge keeps.
    JobId
}

/// A pull request to open: from a head branch into a base, with its title and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPullRequest {
    /// The branch it carries.
    pub head: BranchName,
    /// The branch it merges into.
    pub base: BranchName,
    /// Its title.
    pub title: PrTitle,
    /// Its body.
    pub body: PrBody,
}

/// What asking for a head branch's pull request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// It opened now.
    New(PrNumber),
    /// One was already open for the head; nothing was opened.
    AlreadyOpen(PrNumber),
}

impl Opened {
    /// The pull request, new or already open.
    pub fn number(self) -> PrNumber {
        match self {
            Self::New(number) | Self::AlreadyOpen(number) => number,
        }
    }
}

/// How a pull request's commits land on its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum MergeStrategy {
    /// A merge commit over the head.
    Merge,
    /// One commit holding the whole change.
    Squash,
}

/// What a merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// It merged now.
    Merged,
    /// It had merged before; nothing changed.
    AlreadyMerged,
}

/// Where a check run stands in the words a script reads: GitHub's buckets, which every forge's
/// states fold into.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::VariantArray,
    serde::Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CheckBucket {
    /// Finished green.
    Pass,
    /// Finished red.
    Fail,
    /// Queued or running.
    Pending,
    /// Skipped, which blocks nothing.
    Skipping,
    /// Cancelled before it finished.
    Cancel,
}

impl CheckBucket {
    /// The bucket as the scheduler reads it: skipped counts as passed, cancelled as failed.
    pub fn state(self) -> CheckState {
        match self {
            Self::Pass | Self::Skipping => CheckState::Passed,
            Self::Fail | Self::Cancel => CheckState::Failed,
            Self::Pending => CheckState::Pending,
        }
    }
}

/// One check of a pull request's head as `zaf forge checks` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    /// Its bucket.
    pub bucket: CheckBucket,
    /// Its name.
    pub name: CheckName,
    /// The page it shows on, when the forge gives one.
    pub link: Option<CheckLink>,
}

/// What the forge can show of a failed check's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckLog {
    /// The failed lines of the job the check's link names, as the forge printed them.
    Failed {
        /// The job.
        job: JobId,
        /// Its failed lines.
        lines: String,
    },
    /// The forge has no API for a job's log: the check's link is where a person reads it.
    AtLink,
    /// The link names no job of the forge's own CI, so there is no log to read.
    NoJob,
}

text_newtype! {
    /// The name of a check run (`ci`, a scanner, a bot).
    CheckName
}

text_newtype! {
    /// Who wrote a review comment.
    Author
}

/// Where a check run stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum CheckState {
    /// Queued or running.
    Pending,
    /// Finished green.
    Passed,
    /// Finished red.
    Failed,
}

/// One check run on a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Its name.
    pub name: CheckName,
    /// Where it stands.
    pub state: CheckState,
    /// When it finished; none while it runs.
    pub finished_at: Option<SystemTime>,
}

/// One review comment on a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    /// Who wrote it.
    pub author: Author,
    /// What it says.
    pub body: String,
    /// Where it points, when it is on a line.
    pub location: Option<Location>,
    /// The commit it was made on, when the forge says.
    pub head: Option<CommitId>,
    /// When it was posted.
    pub posted_at: SystemTime,
}

/// A pull request's own times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequest {
    /// When it was opened.
    pub opened_at: SystemTime,
    /// When it merged; none while it is open or if it closed unmerged.
    pub merged_at: Option<SystemTime>,
}

text_newtype! {
    /// The id of the rule a code scanner flagged (`js/sql-injection`).
    RuleId
}

/// One open code scanning alert on a pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanningAlert {
    /// The rule that fired.
    pub rule: RuleId,
    /// Where it fired, when the scanner said.
    pub location: Option<Location>,
}

/// The forge that hosts the code and its reviews. zaf reads it, writes one comment on it (the
/// hand-off), and opens and merges pull requests for the actions that ask (`zaf forge`).
pub trait Forge {
    /// The pull request's check runs, each with the time it finished. A finished check the
    /// forge gives no time for is an error naming the pull request, never a guess.
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError>;

    /// The pull request's review comments, oldest first, each with its post time. A comment the
    /// forge gives no time for is an error naming the pull request.
    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError>;

    /// When the pull request opened and, once merged, when it merged. A time the forge does not
    /// give is an error naming the pull request.
    fn pull_request(&self, pr: PrNumber) -> Result<PullRequest, PortError>;

    /// The commit id the pull request's head is at now, when the forge reports one.
    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// Whether the pull request has merged; a read, it changes nothing. A pull request that is
    /// open or closed unmerged is not merged.
    fn merged(&self, pr: PrNumber) -> Result<bool, PortError>;

    /// The last file of the pull request's diff, in the order the forge lists the
    /// changed files; none when the pull request changes no file.
    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// The open code scanning alerts for the pull request's head ref.
    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError>;

    /// `Forge::handoff(pr) -> Result<Option<String>, PortError>`: the hand-off the pull request
    /// carries, the text of its hand-off comment, the latest when a forge somehow holds several;
    /// none when it has none.
    fn handoff(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// Writes the pull request's hand-off comment, replacing the one it has: after it the
    /// pull request carries exactly one, holding `text`.
    fn set_handoff(&self, pr: PrNumber, text: &str) -> Result<(), PortError>;

    /// Opens a pull request for `request.head` into `request.base`, or hands back the one
    /// already open for that head and opens nothing. Every failure, a refusal by the forge
    /// included, is a backend error: the caller has nothing to change but the forge.
    fn open_pull_request(&self, request: &NewPullRequest) -> Result<Opened, PortError>;

    /// Merges the pull request with `strategy`; with `head`, only while its head is that commit.
    /// One merged before is [`MergeOutcome::AlreadyMerged`]. A pull request closed unmerged, a
    /// head other than `head`, and a merge the forge refuses are [`PortError::Rejected`]; a
    /// merge the forge refuses only for now (the base moved, its mergeability is still being
    /// worked out) is [`PortError::Conflict`], its message saying `not mergeable yet`, so a
    /// caller may try again. A pull request that cannot be read is a backend error.
    fn merge_pull_request(
        &self,
        pr: PrNumber,
        strategy: MergeStrategy,
        head: Option<&CommitId>,
    ) -> Result<MergeOutcome, PortError>;

    /// The checks of the pull request's head, each with its bucket and link, in the order the
    /// forge lists them; none when the forge reports none.
    fn check_runs(&self, pr: PrNumber) -> Result<Vec<CheckRun>, PortError>;

    /// The failed log of the check whose page is `link`, as far as the forge keeps one.
    fn check_log(&self, link: &CheckLink) -> Result<CheckLog, PortError>;

    /// Whether the backend answers and the forge's credential may read and write the
    /// repository, for `zaf doctor`: one check per question, a failed call a failed check, never
    /// an error. A forge reached through a tool `[requires]` already names (gh, with its own
    /// login) answers none.
    fn access(&self) -> Vec<AccessCheck> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
