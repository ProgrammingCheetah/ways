//! What `gh` prints, as far as the adapters read it.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ports::{CheckBucket, Login};

/// The fields every issue read asks `gh issue view` and `gh issue list` for.
pub(super) const ISSUE_FIELDS: &str = "number,title,body,labels,parent,state,comments,issueType";

/// The fields a list of tickets asks for: [`ISSUE_FIELDS`] less what a list has no use for.
pub(super) const TICKET_LIST_FIELDS: &str = "number,title,body,labels,parent,issueType";

/// The fields the list of open tasks asks for: [`TICKET_LIST_FIELDS`] and when each opened, which
/// the scheduler counts a never-started ticket ready from.
pub(super) const OPEN_TICKET_FIELDS: &str = "number,title,body,labels,parent,issueType,createdAt";

/// An issue as `gh issue view --json` prints it.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueRow {
    pub number: u64,
    /// Absent from a list read only for numbers.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Where a person opens it; absent from a read that did not ask.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub labels: Vec<NameRow>,
    #[serde(default)]
    pub parent: Option<NumberRow>,
    #[serde(default)]
    pub state: String,
    /// Why a closed issue was closed; read only where the list asks for `stateReason`.
    #[serde(default, rename = "stateReason")]
    pub state_reason: Option<String>,
    #[serde(default)]
    pub comments: Vec<IssueCommentRow>,
    /// The issue's type; `null` when it has none, absent from a read that did not ask.
    #[serde(default, rename = "issueType")]
    pub issue_type: Option<NameRow>,
    /// When it was opened, RFC 3339; absent from a read that did not ask.
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
    /// When it closed, RFC 3339; null while it is open, absent from a read that did not ask.
    #[serde(default, rename = "closedAt")]
    pub closed_at: Option<String>,
}

/// The fields a blocker's standing is read from, and when it closed.
pub(super) const STANDING_FIELDS: &str = "number,state,stateReason,labels,url,closedAt";

/// An issue read only for where it stands as a blocker.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct StandingRow {
    pub number: u64,
    #[serde(default)]
    pub state: String,
    /// Why a closed issue was closed: `COMPLETED`, `NOT_PLANNED` or `DUPLICATE`; empty when
    /// GitHub kept no reason.
    #[serde(default, rename = "stateReason")]
    pub state_reason: Option<String>,
    #[serde(default)]
    pub labels: Vec<NameRow>,
    /// Holds `/pull/` for a pull request.
    #[serde(default)]
    pub url: String,
    /// When it closed, RFC 3339; null while it is open.
    #[serde(default, rename = "closedAt")]
    pub closed_at: Option<String>,
}

/// The fields the time an issue was opened is read from.
pub(super) const OPENED_FIELDS: &str = "number,url,createdAt";

/// An issue read only for when it was opened.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct OpenedRow {
    /// Holds `/pull/` for a pull request.
    #[serde(default)]
    pub url: String,
    /// When it was opened, RFC 3339.
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
}

/// A label, or anything else read only by name.
#[derive(Debug, Clone, Deserialize)]
pub struct NameRow {
    pub name: String,
}

/// An issue or pull request read only by number.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct NumberRow {
    pub number: u64,
}

/// A comment on an issue as `gh issue view --json comments` prints it.
#[derive(Debug, Clone, Deserialize)]
pub struct IssueCommentRow {
    pub body: String,
    /// Ends in `#issuecomment-<id>`, the id the REST API edits by.
    #[serde(default)]
    pub url: String,
    /// When it was posted, RFC 3339; absent from a read that did not ask for it.
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
    /// The login that posted it; none for a deleted account or a read that did not ask.
    #[serde(default, deserialize_with = "crate::adapters::issues::author_login")]
    pub author: Option<Login>,
}

/// A conversation comment as `repos/{repo}/issues/{n}/comments` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConversationCommentRow {
    pub id: u64,
    #[serde(default)]
    pub body: String,
}

/// A pull request read only by state.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct PrStateRow {
    pub state: String,
}

/// A pull request read for the times it opened and merged.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct PrTimesRow {
    pub state: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    #[serde(rename = "mergedAt", default)]
    pub merged_at: Option<String>,
    /// gh's `mergeable`: `MERGEABLE`, `CONFLICTING` or `UNKNOWN` (GitHub's `MergeableState`;
    /// `UNKNOWN` while GitHub is still working it out). Anything else, or none, is not said.
    #[serde(default)]
    pub mergeable: Option<String>,
    /// gh's `headRefOid`: the commit the head is at; empty or absent when gh says none.
    #[serde(rename = "headRefOid", default)]
    pub head_ref_oid: Option<String>,
}

/// A check run as `gh pr checks --json name,bucket,completedAt` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CheckRow {
    pub name: String,
    /// gh's grouping of the run's state and conclusion, the port's own words.
    pub bucket: CheckBucket,
    /// gh prints the zero time for a run that has not finished.
    #[serde(rename = "completedAt", default)]
    pub completed_at: Option<String>,
}

/// A branch as `repos/{repo}/branches/{branch}` prints it: only the commit it is at.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct BranchHeadRow {
    pub commit: BranchCommitRow,
}

impl BranchHeadRow {
    /// The time of the commit the branch is at (its committer's date), when GitHub gives one that
    /// reads.
    pub fn committed_at(&self) -> Option<std::time::SystemTime> {
        let text = self
            .commit
            .commit
            .as_ref()?
            .committer
            .as_ref()?
            .date
            .as_deref()?;
        humantime::parse_rfc3339(text).ok()
    }
}

/// The commit a branch is at.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct BranchCommitRow {
    pub sha: String,
    /// The commit itself, as git holds it.
    #[serde(default)]
    pub commit: Option<GitCommitRow>,
}

/// A commit as git holds it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct GitCommitRow {
    #[serde(default)]
    pub committer: Option<GitSignatureRow>,
}

/// Who made a commit, and when.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct GitSignatureRow {
    #[serde(default)]
    pub date: Option<String>,
}

/// One page of `repos/{repo}/commits/{ref}/check-runs`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CheckRunsPage {
    #[serde(default)]
    pub check_runs: Vec<CommitCheckRow>,
}

/// A check run of a commit as the REST API prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CommitCheckRow {
    pub name: String,
    /// `completed` once it has finished; `queued`, `in_progress` and the like until then.
    pub status: String,
    /// How it finished (`success`, `failure`, `skipped`, ...); none until it has.
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

impl CommitCheckRow {
    /// The bucket gh puts such a run in: not completed is pending; `success` passes, `skipped`
    /// and `neutral` block nothing, `cancelled` is cancelled, and any other conclusion fails.
    pub fn bucket(&self) -> CheckBucket {
        if self.status != "completed" {
            return CheckBucket::Pending;
        }
        match self.conclusion.as_deref() {
            Some("success") => CheckBucket::Pass,
            Some("skipped" | "neutral") => CheckBucket::Skipping,
            Some("cancelled") => CheckBucket::Cancel,
            _ => CheckBucket::Fail,
        }
    }
}

/// A user as the REST API prints it; absent for a deleted account.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct UserRow {
    pub login: String,
}

/// A line comment from `repos/{repo}/pulls/{n}/comments`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct LineCommentRow {
    pub user: Option<UserRow>,
    pub body: String,
    pub path: String,
    pub line: Option<u32>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub commit_id: Option<String>,
}

/// A review from `repos/{repo}/pulls/{n}/reviews`; a pending one has no `submitted_at`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReviewRow {
    pub user: Option<UserRow>,
    #[serde(default)]
    pub body: Option<String>,
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub commit_id: Option<String>,
}

/// A changed file from `repos/{repo}/pulls/{n}/files`, read by its path and, for a rename, the
/// path it had.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct FileRow {
    pub filename: String,
    #[serde(default)]
    pub previous_filename: Option<String>,
}

/// What one batched read of issues returns: each alias the query named, its issue or `null`.
#[derive(Debug, Deserialize)]
pub(super) struct BatchData {
    pub repository: Option<BTreeMap<String, Option<BatchIssue>>>,
}

/// A GraphQL connection: its nodes, and how many there are in all when asked.
#[derive(Debug, Deserialize)]
pub(super) struct BatchNodes<T> {
    pub nodes: Vec<T>,
    #[serde(default, rename = "totalCount")]
    pub total: Option<usize>,
}

/// An issue as the batched GraphQL read returns it: [`ISSUE_FIELDS`] in GraphQL's shape.
#[derive(Debug, Deserialize)]
pub(super) struct BatchIssue {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
    labels: BatchNodes<NameRow>,
    #[serde(default)]
    parent: Option<NumberRow>,
    #[serde(default, rename = "issueType")]
    issue_type: Option<NameRow>,
    comments: BatchNodes<IssueCommentRow>,
}

impl BatchIssue {
    /// The row `gh issue view` would print, or none when the read cut the comments or the labels
    /// short: the batch holds the first hundred of each, and a claim lives in the latest comment.
    pub(super) fn into_row(self) -> Option<IssueRow> {
        let cut = |total: Option<usize>, held: usize| total.is_some_and(|total| total > held);
        if cut(self.comments.total, self.comments.nodes.len())
            || cut(self.labels.total, self.labels.nodes.len())
        {
            return None;
        }
        Some(IssueRow {
            number: self.number,
            title: self.title,
            body: self.body,
            url: String::new(),
            labels: self.labels.nodes,
            parent: self.parent,
            state: self.state,
            state_reason: None,
            comments: self.comments.nodes,
            issue_type: self.issue_type,
            created_at: None,
            closed_at: None,
        })
    }
}
