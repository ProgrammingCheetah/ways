//! What `gh` prints, as far as the adapters read it.

use serde::Deserialize;

/// The fields every issue read asks `gh issue view` and `gh issue list` for.
pub(super) const ISSUE_FIELDS: &str = "number,title,body,labels,parent,state,comments";

/// An issue as `gh issue view --json` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct IssueRow {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<NameRow>,
    #[serde(default)]
    pub parent: Option<NumberRow>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub comments: Vec<IssueCommentRow>,
}

/// A label, or anything else read only by name.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct NameRow {
    pub name: String,
}

/// An issue or pull request read only by number.
#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) struct NumberRow {
    pub number: u64,
}

/// A comment on an issue as `gh issue view --json comments` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct IssueCommentRow {
    pub body: String,
    /// Ends in `#issuecomment-<id>`, the id the REST API edits by.
    #[serde(default)]
    pub url: String,
}

/// A pull request read only by state.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct PrStateRow {
    pub state: String,
}

/// A check run as `gh pr checks --json name,bucket` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CheckRow {
    pub name: String,
    pub bucket: CheckBucket,
}

/// gh's grouping of a check run's state and conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
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
    pub created_at: String,
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

/// A pull request read only by its head commit.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct PrHeadRow {
    #[serde(rename = "headRefOid", default)]
    pub head_ref_oid: Option<String>,
}
