//! What Forgejo's REST API answers, as far as the adapters read it, and the times it writes.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Deserializer};

/// The state Forgejo gives an open issue.
pub(super) const OPEN: &str = "open";

/// An issue as `GET /repos/{owner}/{repo}/issues/{index}` and the issue lists print it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct IssueRow {
    pub number: u64,
    #[serde(default, deserialize_with = "or_default")]
    pub title: String,
    #[serde(default, deserialize_with = "or_default")]
    pub body: String,
    #[serde(default, deserialize_with = "or_default")]
    pub labels: Vec<LabelRow>,
    /// `open` or `closed`.
    #[serde(default, deserialize_with = "or_default")]
    pub state: String,
    /// The page a person opens.
    #[serde(default, deserialize_with = "or_default")]
    pub html_url: String,
    /// Set for a pull request, `null` for an issue.
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

impl IssueRow {
    /// Whether it carries the label `name`.
    pub fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|label| label.name == name)
    }

    /// Its labels' names, in the order Forgejo lists them.
    pub fn label_names(&self) -> impl Iterator<Item = &str> {
        self.labels.iter().map(|label| label.name.as_str())
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.state == OPEN
    }

    /// Whether it is a pull request.
    pub fn is_pull(&self) -> bool {
        self.pull_request.is_some()
    }
}

/// A label, on an issue or in a repository's list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct LabelRow {
    pub id: u64,
    pub name: String,
    /// Whether it is exclusive within its `scope/`.
    #[serde(default)]
    pub exclusive: bool,
}

/// A comment on an issue as `GET /repos/{owner}/{repo}/issues/{index}/comments` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CommentRow {
    /// The id the API edits it by.
    pub id: u64,
    #[serde(default, deserialize_with = "or_default")]
    pub body: String,
    /// When it was posted, RFC 3339 with the instance's offset.
    #[serde(default)]
    pub created_at: Option<String>,
}

impl CommentRow {
    /// When it was posted, when Forgejo said in a form that reads.
    pub fn posted(&self) -> Option<SystemTime> {
        self.created_at.as_deref().and_then(parse_time)
    }
}

/// The repository as `GET /repos/{owner}/{repo}` prints it, read only for what its caller may do.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RepositoryRow {
    #[serde(default)]
    pub permissions: Option<PermissionRow>,
}

/// What the caller may do in a repository.
#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) struct PermissionRow {
    #[serde(default)]
    pub pull: bool,
    #[serde(default)]
    pub push: bool,
}

/// What `GET /version` prints.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct VersionRow {
    pub version: String,
}

/// A pull request as `GET /repos/{owner}/{repo}/pulls/{index}` and the pull request list print
/// it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct PullRow {
    pub number: u64,
    /// `open` or `closed`; a merged pull request is `closed` and `merged`.
    #[serde(default, deserialize_with = "or_default")]
    pub state: String,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub merged_at: Option<String>,
    #[serde(default)]
    pub head: Option<BranchRow>,
    #[serde(default)]
    pub base: Option<BranchRow>,
}

impl PullRow {
    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.state == OPEN
    }

    /// The commit its head is at, when Forgejo says.
    pub fn head_sha(&self) -> Option<&str> {
        self.head
            .as_ref()
            .map(|head| head.sha.as_str())
            .filter(|sha| !sha.is_empty())
    }

    /// Whether its head is the branch `name` of the repository it merges into, not a fork's.
    pub fn is_from(&self, name: &str) -> bool {
        let Some(head) = &self.head else {
            return false;
        };
        let same_repo = match (
            head.repo_id,
            self.base.as_ref().and_then(|base| base.repo_id),
        ) {
            (Some(head), Some(base)) => head == base,
            _ => true,
        };
        head.name == name && same_repo
    }
}

/// A pull request's head or base.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct BranchRow {
    /// The branch's name.
    #[serde(rename = "ref", default, deserialize_with = "or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "or_default")]
    pub sha: String,
    #[serde(default)]
    pub repo_id: Option<u64>,
}

/// A commit status as `GET /repos/{owner}/{repo}/commits/{ref}/statuses` prints it: one row per
/// post, so a context's latest row is where it stands.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct StatusRow {
    pub id: u64,
    /// `pending`, `success`, `error`, `failure` or `warning`.
    #[serde(default, deserialize_with = "or_default")]
    pub status: String,
    /// The check's name; Forgejo Actions posts `<workflow> / <job> (<event>)`.
    #[serde(default, deserialize_with = "or_default")]
    pub context: String,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A user as the API prints one; a deleted account prints none.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct UserRow {
    pub login: String,
}

/// A review as `GET /repos/{owner}/{repo}/pulls/{index}/reviews` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReviewRow {
    pub id: u64,
    #[serde(default)]
    pub user: Option<UserRow>,
    #[serde(default, deserialize_with = "or_default")]
    pub body: String,
    /// `APPROVED`, `COMMENT`, `REQUEST_CHANGES`, `REQUEST_REVIEW` or `PENDING` (a draft).
    #[serde(default, deserialize_with = "or_default")]
    pub state: String,
    #[serde(default)]
    pub commit_id: Option<String>,
    #[serde(default)]
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub comments_count: u64,
}

/// A line comment of a review as `GET …/pulls/{index}/reviews/{id}/comments` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReviewCommentRow {
    #[serde(default)]
    pub user: Option<UserRow>,
    #[serde(default, deserialize_with = "or_default")]
    pub body: String,
    #[serde(default, deserialize_with = "or_default")]
    pub path: String,
    /// The line on the new side of the diff; 0 for a comment on a removed line.
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub commit_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A changed file as `GET /repos/{owner}/{repo}/pulls/{index}/files` prints it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChangedFileRow {
    pub filename: String,
}

/// What `GET /settings/api` prints, read only for the page size.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ApiSettingsRow {
    pub max_response_items: u64,
}

/// A field Forgejo may print as `null` read as its default, as when it is absent.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// An RFC 3339 time as Forgejo writes it, in UTC (`…Z`) or with the instance's offset
/// (`…-07:00`); `None` when it does not read.
pub(super) fn parse_time(text: &str) -> Option<SystemTime> {
    let text = text.trim();
    if text.ends_with('Z') {
        return humantime::parse_rfc3339(text).ok();
    }
    let split = text.len().checked_sub(6)?;
    let (local, offset) = (text.get(..split)?, text.get(split..)?);
    let (sign, hours_minutes) = offset.split_at(1);
    let (hours, minutes) = hours_minutes.split_once(':')?;
    let seconds = (hours.parse::<u64>().ok()? * 60 + minutes.parse::<u64>().ok()?) * 60;
    let as_utc = humantime::parse_rfc3339(&format!("{local}Z")).ok()?;
    match sign {
        // Local time ahead of UTC: UTC is earlier.
        "+" => as_utc.checked_sub(Duration::from_secs(seconds)),
        "-" => as_utc.checked_add(Duration::from_secs(seconds)),
        _ => None,
    }
}
