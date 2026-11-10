//! The forge on GitHub: forge kind `github`.

use std::time::SystemTime;

use serde::Deserialize;

use super::GhCommand;
use super::rows::{
    CheckBucket, CheckRow, FileRow, LineCommentRow, PrHeadRow, PrStateRow, PrTimesRow, ReviewRow,
    UserRow,
};
use crate::domain::Location;
use crate::ports::{
    Author, Check, CheckName, CheckState, CommitId, Forge, PortError, PrNumber, PullRequest,
    ReviewComment, RuleId, ScanningAlert,
};

/// The author shown for a comment whose account was deleted, as GitHub shows it.
const GHOST: &str = "ghost";
/// What gh prints when a pull request has no check runs at all.
const NO_CHECKS: &str = "no checks reported";

/// The pull requests of one GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct GithubForge {
    gh: GhCommand,
}

/// The bare number gh takes, without the `#` a [`PrNumber`] displays with.
fn bare(pr: PrNumber) -> String {
    pr.get().to_string()
}

impl From<CheckBucket> for CheckState {
    fn from(bucket: CheckBucket) -> Self {
        match bucket {
            CheckBucket::Pass | CheckBucket::Skipping => Self::Passed,
            CheckBucket::Fail | CheckBucket::Cancel => Self::Failed,
            CheckBucket::Pending => Self::Pending,
        }
    }
}

fn author(user: Option<UserRow>) -> Author {
    user.map_or_else(|| Author::from(GHOST), |user| Author::from(user.login))
}

/// The zero time gh prints for a run that has not finished.
const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

/// The time gh gave for `what` on a pull request; a time gh left out, or printed as the zero
/// time or in a form that does not parse, is an error naming the pull request.
fn time_of(pr: PrNumber, what: &str, text: Option<&str>) -> Result<SystemTime, PortError> {
    let text = text.filter(|text| !text.is_empty() && *text != ZERO_TIME);
    let Some(text) = text else {
        return Err(PortError::Backend(format!(
            "pull request {pr}: gh gave no time for {what}"
        )));
    };
    humantime::parse_rfc3339(text).map_err(|error| {
        PortError::Backend(format!(
            "pull request {pr}: gh printed a bad time `{text}` for {what}: {error}"
        ))
    })
}

/// An alert as `repos/{repo}/code-scanning/alerts` prints it.
#[derive(Debug, Deserialize)]
struct AlertRow {
    rule: RuleRow,
    #[serde(default)]
    most_recent_instance: Option<InstanceRow>,
}

#[derive(Debug, Deserialize)]
struct RuleRow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct InstanceRow {
    #[serde(default)]
    location: Option<AlertLocationRow>,
}

#[derive(Debug, Deserialize)]
struct AlertLocationRow {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    start_line: Option<u32>,
}

impl From<AlertRow> for ScanningAlert {
    fn from(row: AlertRow) -> Self {
        let location = row
            .most_recent_instance
            .and_then(|instance| instance.location)
            .and_then(|location| {
                let path = location.path?;
                Some(Location {
                    path,
                    line: location.start_line,
                })
            });
        Self {
            rule: RuleId::from(row.rule.id),
            location,
        }
    }
}

impl Forge for GithubForge {
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        let number = bare(pr);
        let rows: Vec<CheckRow> =
            match self
                .gh
                .json(&["pr", "checks", &number, "--json", "name,bucket,completedAt"])
            {
                Err(PortError::Backend(stderr)) if stderr.contains(NO_CHECKS) => Vec::new(),
                other => other?,
            };
        rows.into_iter()
            .map(|row| {
                let state = CheckState::from(row.bucket);
                // A running check has no finish time, whatever gh prints in its place.
                let finished_at = match state {
                    CheckState::Pending => None,
                    CheckState::Passed | CheckState::Failed => {
                        let what = format!("the finish of check `{}`", row.name);
                        Some(time_of(pr, &what, row.completed_at.as_deref())?)
                    }
                };
                Ok(Check {
                    name: CheckName::from(row.name),
                    state,
                    finished_at,
                })
            })
            .collect()
    }

    fn pull_request(&self, pr: PrNumber) -> Result<PullRequest, PortError> {
        let number = bare(pr);
        let row: PrTimesRow =
            self.gh
                .json(&["pr", "view", &number, "--json", "state,createdAt,mergedAt"])?;
        let opened_at = time_of(pr, "its opening", row.created_at.as_deref())?;
        let merged_at = if row.state == "MERGED" {
            Some(time_of(pr, "its merge", row.merged_at.as_deref())?)
        } else {
            None
        };
        Ok(PullRequest {
            opened_at,
            merged_at,
        })
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        let number = bare(pr);
        let lines: Vec<LineCommentRow> = self.gh.api_list(&format!("pulls/{number}/comments"))?;
        let reviews: Vec<ReviewRow> = self.gh.api_list(&format!("pulls/{number}/reviews"))?;
        let mut comments: Vec<ReviewComment> = Vec::new();
        for review in reviews {
            let (Some(submitted_at), Some(body)) = (review.submitted_at, review.body) else {
                continue;
            };
            if body.trim().is_empty() {
                continue;
            }
            let comment = ReviewComment {
                author: author(review.user),
                body,
                location: None,
                head: review.commit_id.map(CommitId::from),
                posted_at: time_of(pr, "a review's posting", Some(&submitted_at))?,
            };
            comments.push(comment);
        }
        for line in lines {
            let location = Location {
                path: line.path,
                line: line.line,
            };
            let comment = ReviewComment {
                author: author(line.user),
                body: line.body,
                location: Some(location),
                head: line.commit_id.map(CommitId::from),
                posted_at: time_of(pr, "a comment's posting", line.created_at.as_deref())?,
            };
            comments.push(comment);
        }
        comments.sort_by_key(|comment| comment.posted_at);
        Ok(comments)
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let number = bare(pr);
        let pull: PrHeadRow = self
            .gh
            .json(&["pr", "view", &number, "--json", "headRefOid"])?;
        Ok(pull.head_ref_oid.filter(|head| !head.is_empty()))
    }

    fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
        let number = bare(pr);
        let pull: PrStateRow = self.gh.json(&["pr", "view", &number, "--json", "state"])?;
        Ok(pull.state == "MERGED")
    }

    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let number = bare(pr);
        let files: Vec<FileRow> = self.gh.api_list(&format!("pulls/{number}/files"))?;
        Ok(files.into_iter().next_back().map(|file| file.filename))
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        let number = bare(pr);
        let endpoint = format!("code-scanning/alerts?ref=refs/pull/{number}/head&state=open");
        let rows: Vec<AlertRow> = self.gh.api_list(&endpoint)?;
        Ok(rows.into_iter().map(ScanningAlert::from).collect())
    }
}
