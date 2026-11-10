//! The forge on GitHub: forge kind `github`.

use std::time::SystemTime;

use serde::Deserialize;

use super::GhCommand;
use super::notes::{HANDOFF_MARKER, marked, unmarked};
use super::rows::{
    CheckBucket, CheckRow, ConversationCommentRow, FileRow, LineCommentRow, PrHeadRow, PrStateRow,
    ReviewRow, UserRow,
};
use crate::domain::Location;
use crate::ports::{
    Author, Check, CheckName, CheckState, Forge, PortError, PrNumber, ReviewComment, RuleId,
    ScanningAlert,
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

fn parse_time(text: &str) -> Result<SystemTime, PortError> {
    humantime::parse_rfc3339(text)
        .map_err(|error| PortError::Backend(format!("gh printed a bad time `{text}`: {error}")))
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
                .json(&["pr", "checks", &number, "--json", "name,bucket"])
            {
                Err(PortError::Backend(stderr)) if stderr.contains(NO_CHECKS) => Vec::new(),
                other => other?,
            };
        let checks = rows
            .into_iter()
            .map(|row| Check {
                name: CheckName::from(row.name),
                state: CheckState::from(row.bucket),
            })
            .collect();
        Ok(checks)
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        let number = bare(pr);
        let lines: Vec<LineCommentRow> = self.gh.api_list(&format!("pulls/{number}/comments"))?;
        let reviews: Vec<ReviewRow> = self.gh.api_list(&format!("pulls/{number}/reviews"))?;
        let mut dated: Vec<(SystemTime, ReviewComment)> = Vec::new();
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
                head: review.commit_id,
            };
            dated.push((parse_time(&submitted_at)?, comment));
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
                head: line.commit_id,
            };
            dated.push((parse_time(&line.created_at)?, comment));
        }
        dated.sort_by_key(|(at, _)| *at);
        Ok(dated.into_iter().map(|(_, comment)| comment).collect())
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

    fn handoff(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let comments = self.conversation(pr)?;
        let latest = comments
            .iter()
            .rev()
            .find_map(|comment| unmarked(HANDOFF_MARKER, &comment.body));
        Ok(latest.map(str::to_owned))
    }

    fn set_handoff(&self, pr: PrNumber, text: &str) -> Result<(), PortError> {
        let handoff = marked(HANDOFF_MARKER, text);
        let comments = self.conversation(pr)?;
        let existing = comments
            .iter()
            .rev()
            .find(|comment| unmarked(HANDOFF_MARKER, &comment.body).is_some());
        match existing {
            Some(comment) if comment.body == handoff => Ok(()),
            Some(comment) => {
                let endpoint = format!("issues/comments/{}", comment.id);
                let field = format!("body={handoff}");
                self.gh
                    .api_text(&endpoint, &["--method", "PATCH", "-f", &field])
                    .map(drop)
            }
            None => self
                .gh
                .text(&["pr", "comment", &bare(pr), "--body", &handoff])
                .map(drop),
        }
    }
}

impl GithubForge {
    /// The pull request's conversation comments, oldest first.
    fn conversation(&self, pr: PrNumber) -> Result<Vec<ConversationCommentRow>, PortError> {
        self.gh.api_list(&format!("issues/{}/comments", bare(pr)))
    }
}
