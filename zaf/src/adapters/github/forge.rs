//! The forge on GitHub: forge kind `github`.

use std::time::SystemTime;

use super::GhCommand;
use super::command::number_from_url;
use super::rows::{
    CheckBucket, CheckRow, LineCommentRow, NumberRow, PrStateRow, ReviewRow, UserRow,
};
use crate::domain::Location;
use crate::ports::{
    Author, Check, CheckName, CheckState, Forge, PortError, PrMerge, PrNumber, PrRequest,
    ReviewComment,
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
    pr.to_string().trim_start_matches('#').to_owned()
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

impl Forge for GithubForge {
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        let open: Vec<NumberRow> = self.gh.json(&[
            "pr",
            "list",
            "--head",
            request.slice.as_ref(),
            "--state",
            "open",
            "--json",
            "number",
        ])?;
        if let Some(existing) = open.first() {
            return Ok(PrNumber::from(existing.number));
        }
        let stdout = self.gh.text(&[
            "pr",
            "create",
            "--head",
            request.slice.as_ref(),
            "--base",
            request.base.as_ref(),
            "--title",
            &request.title,
            "--body",
            &request.body,
        ])?;
        number_from_url(&stdout).map(PrNumber::from)
    }

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
            };
            dated.push((parse_time(&line.created_at)?, comment));
        }
        dated.sort_by_key(|(at, _)| *at);
        Ok(dated.into_iter().map(|(_, comment)| comment).collect())
    }

    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError> {
        let number = bare(pr);
        let pull: PrStateRow = self.gh.json(&["pr", "view", &number, "--json", "state"])?;
        match pull.state.as_str() {
            "MERGED" => Ok(PrMerge::AlreadyMerged),
            "CLOSED" => Err(PortError::Rejected(format!("{pr} is closed unmerged"))),
            _ => {
                self.gh.text(&["pr", "merge", &number, "--merge"])?;
                Ok(PrMerge::Merged)
            }
        }
    }
}
