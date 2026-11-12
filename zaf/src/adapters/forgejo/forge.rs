//! The forge on Forgejo: forge kind `forgejo`.
//!
//! Pull requests, their reviews and the hand-off comment are read and written over the REST API
//! through [`ForgejoApi`]. A check is a commit status of the pull request's head: Forgejo Actions
//! posts one per job, and a context's latest status is where that check stands. Forgejo has no
//! code scanning, so a pull request has no scanning alerts, and no API for a job's log, so a
//! failed check's log is its link. The hand-off is the GitHub forge's comment, written and read
//! by the same shared code.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde_json::json;

use super::access::{self, Write};
use super::api::{ForgejoApi, Method};
use super::rows::{
    ChangedFileRow, CommentRow, PullRow, ReviewCommentRow, ReviewRow, StatusRow, UserRow,
    parse_time,
};
use crate::adapters::notes::{HANDOFF_MARKER, marked, unmarked};
use crate::domain::Location;
use crate::ports::{
    AccessCheck, Author, Check, CheckBucket, CheckLink, CheckLog, CheckName, CheckRun, CheckState,
    CommitId, Forge, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PortError, PrNumber,
    PullRequest, ReviewComment, ScanningAlert,
};

/// The author shown for a comment whose account was deleted.
const GHOST: &str = "ghost";

/// The zero time a Go server prints for a time it does not have.
const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

/// The state of a review left as a draft: not posted, so nobody else sees it.
const DRAFT_REVIEW: &str = "PENDING";

/// The status Forgejo answers a merge it cannot make yet (`Please try again later`: its
/// mergeability is still being worked out), among other refusals.
const NOT_ALLOWED: u16 = 405;

/// The status Forgejo answers a merge whose push found the base moved (`merge push out of
/// date`), among other conflicts.
const CONFLICT: u16 = 409;

/// The pull requests of one Forgejo repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoForge {
    api: ForgejoApi,
    beside_board: bool,
}

impl From<ForgejoApi> for ForgejoForge {
    fn from(api: ForgejoApi) -> Self {
        Self {
            api,
            beside_board: false,
        }
    }
}

impl ForgejoForge {
    /// The forge on the same instance, repository and token as the project's Forgejo board:
    /// `zaf doctor` then asks it only what the board's checks do not, whether the token may
    /// write pull requests.
    pub fn beside_board(api: ForgejoApi) -> Self {
        Self {
            api,
            beside_board: true,
        }
    }

    fn pull(&self, pr: PrNumber) -> Result<PullRow, PortError> {
        self.api.get(&format!("pulls/{}", pr.get()))
    }

    /// The commit the pull request's head is at; one Forgejo gives none for is an error.
    fn head_of(&self, pr: PrNumber) -> Result<String, PortError> {
        self.pull(pr)?
            .head_sha()
            .map(str::to_owned)
            .ok_or_else(|| PortError::Backend(format!("pull request {pr}: Forgejo gave no head")))
    }

    /// The latest status of each context at `sha`, in name order: one per check.
    fn statuses(&self, sha: &str) -> Result<Vec<StatusRow>, PortError> {
        let rows: Vec<StatusRow> = self.api.list(&format!("commits/{sha}/statuses"))?;
        let mut latest: BTreeMap<String, StatusRow> = BTreeMap::new();
        for row in rows {
            let newer = latest.get(&row.context).is_none_or(|seen| seen.id < row.id);
            if newer {
                latest.insert(row.context.clone(), row);
            }
        }
        Ok(latest.into_values().collect())
    }

    /// The pull request's conversation comments, oldest first: a pull request is an issue.
    fn conversation(&self, pr: PrNumber) -> Result<Vec<CommentRow>, PortError> {
        let mut comments: Vec<CommentRow> =
            self.api.get(&format!("issues/{}/comments", pr.get()))?;
        comments.sort_by_key(|comment| comment.id);
        Ok(comments)
    }
}

/// The bucket a commit status falls in: `warning` blocks nothing, `error` is a failure.
fn bucket(pr: PrNumber, row: &StatusRow) -> Result<CheckBucket, PortError> {
    match row.status.as_str() {
        "pending" => Ok(CheckBucket::Pending),
        "success" | "warning" => Ok(CheckBucket::Pass),
        "failure" | "error" => Ok(CheckBucket::Fail),
        other => Err(PortError::Backend(format!(
            "pull request {pr}: Forgejo gave check `{}` the unknown status `{other}`",
            row.context
        ))),
    }
}

/// A status's link as a person opens it: a path (`/owner/repo/actions/runs/4/jobs/0`) is put
/// after the scheme and host of the instance at `url`; anything else is kept as given.
fn whole_link(url: &str, link: &str) -> String {
    if !link.starts_with('/') || link.starts_with("//") {
        return link.to_owned();
    }
    let host_from = url.find("://").map_or(0, |at| at + 3);
    let origin = url[host_from..]
        .find('/')
        .map_or(url, |at| &url[..host_from + at]);
    format!("{origin}{link}")
}

fn author(user: Option<UserRow>) -> Author {
    user.map_or_else(|| Author::from(GHOST), |user| Author::from(user.login))
}

/// The time Forgejo gave for `what` on a pull request; one it left out, printed as the zero
/// time or in a form that does not read is an error naming the pull request.
fn time_of(pr: PrNumber, what: &str, text: Option<&str>) -> Result<SystemTime, PortError> {
    let text = text.filter(|text| !text.is_empty() && *text != ZERO_TIME);
    let Some(text) = text else {
        return Err(PortError::Backend(format!(
            "pull request {pr}: Forgejo gave no time for {what}"
        )));
    };
    parse_time(text).ok_or_else(|| {
        PortError::Backend(format!(
            "pull request {pr}: Forgejo printed a bad time `{text}` for {what}"
        ))
    })
}

impl Forge for ForgejoForge {
    /// The latest status of each context at the head, finished at the time it was posted.
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        let head = self.head_of(pr)?;
        self.statuses(&head)?
            .into_iter()
            .map(|row| {
                let state = bucket(pr, &row)?.state();
                let finished_at = match state {
                    CheckState::Pending => None,
                    CheckState::Passed | CheckState::Failed => {
                        let what = format!("the finish of check `{}`", row.context);
                        Some(time_of(pr, &what, row.created_at.as_deref())?)
                    }
                };
                Ok(Check {
                    name: CheckName::from(row.context),
                    state,
                    finished_at,
                })
            })
            .collect()
    }

    /// Each posted review's body, then each of its line comments, oldest first; a draft review
    /// and an empty body are no comment.
    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        let number = pr.get();
        let reviews: Vec<ReviewRow> = self.api.list(&format!("pulls/{number}/reviews"))?;
        let mut comments = Vec::new();
        for review in reviews {
            if review.state == DRAFT_REVIEW {
                continue;
            }
            if review.comments_count > 0 {
                let endpoint = format!("pulls/{number}/reviews/{}/comments", review.id);
                let lines: Vec<ReviewCommentRow> = self.api.get(&endpoint)?;
                for line in lines {
                    let location = Location {
                        path: line.path,
                        line: line.position.filter(|line| *line > 0),
                    };
                    comments.push(ReviewComment {
                        author: author(line.user),
                        body: line.body,
                        location: Some(location),
                        head: line
                            .commit_id
                            .filter(|id| !id.is_empty())
                            .map(CommitId::from),
                        posted_at: time_of(pr, "a comment's posting", line.created_at.as_deref())?,
                    });
                }
            }
            if review.body.trim().is_empty() {
                continue;
            }
            comments.push(ReviewComment {
                author: author(review.user),
                body: review.body,
                location: None,
                head: review
                    .commit_id
                    .filter(|id| !id.is_empty())
                    .map(CommitId::from),
                posted_at: time_of(pr, "a review's posting", review.submitted_at.as_deref())?,
            });
        }
        comments.sort_by_key(|comment| comment.posted_at);
        Ok(comments)
    }

    fn pull_request(&self, pr: PrNumber) -> Result<PullRequest, PortError> {
        let row = self.pull(pr)?;
        let opened_at = time_of(pr, "its opening", row.created_at.as_deref())?;
        let merged_at = if row.merged {
            Some(time_of(pr, "its merge", row.merged_at.as_deref())?)
        } else {
            None
        };
        Ok(PullRequest {
            opened_at,
            merged_at,
        })
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        Ok(self.pull(pr)?.head_sha().map(str::to_owned))
    }

    fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
        Ok(self.pull(pr)?.merged)
    }

    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let files: Vec<ChangedFileRow> = self.api.list(&format!("pulls/{}/files", pr.get()))?;
        Ok(files.into_iter().next_back().map(|file| file.filename))
    }

    /// Forgejo has no code scanning.
    fn scanning_alerts(&self, _pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        Ok(Vec::new())
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
        let body = json!({ "body": handoff });
        match existing {
            Some(comment) if comment.body == handoff => Ok(()),
            Some(comment) => {
                let endpoint = format!("issues/comments/{}", comment.id);
                self.api
                    .send::<serde_json::Value>(Method::Patch, &endpoint, Some(&body))
                    .map(drop)
            }
            None => {
                let endpoint = format!("issues/{}/comments", pr.get());
                self.api
                    .send::<serde_json::Value>(Method::Post, &endpoint, Some(&body))
                    .map(drop)
            }
        }
    }

    /// The open pull requests are listed and the one whose head is the branch, in this
    /// repository, is handed back; with none, one is created.
    fn open_pull_request(&self, request: &NewPullRequest) -> Result<Opened, PortError> {
        let open: Vec<PullRow> = self
            .api
            .list("pulls?state=open")
            .map_err(PortError::into_backend)?;
        let head = request.head.as_str();
        if let Some(pull) = open.iter().find(|pull| pull.is_from(head)) {
            return Ok(Opened::AlreadyOpen(PrNumber::from(pull.number)));
        }
        let body = json!({
            "head": head,
            "base": request.base.as_str(),
            "title": request.title.as_str(),
            "body": request.body.as_str(),
        });
        let created: PullRow = self
            .api
            .send(Method::Post, "pulls", Some(&body))
            .map_err(PortError::into_backend)?;
        Ok(Opened::New(PrNumber::from(created.number)))
    }

    /// The pull request is read first: merged is quiet, closed or at another head is refused
    /// before anything is sent. Forgejo's refusal decides the rest: a merge it cannot make yet
    /// (405, try again later) or whose push found the base moved (409, out of date) is one for
    /// now; any other is final.
    fn merge_pull_request(
        &self,
        pr: PrNumber,
        strategy: MergeStrategy,
        head: Option<&CommitId>,
    ) -> Result<MergeOutcome, PortError> {
        let pull = self.pull(pr).map_err(PortError::into_backend)?;
        if pull.merged {
            return Ok(MergeOutcome::AlreadyMerged);
        }
        if !pull.is_open() {
            return Err(PortError::Rejected(format!(
                "pull request {pr} is closed unmerged"
            )));
        }
        if let Some(pin) = head {
            let at = pull.head_sha().unwrap_or("no commit");
            if at != pin.as_str() {
                return Err(PortError::Rejected(format!(
                    "pull request {pr}'s head is {at}, not {pin}"
                )));
            }
        }
        let mut body = json!({ "Do": strategy.to_string() });
        if let Some(pin) = head {
            body["head_commit_id"] = json!(pin.as_str());
        }
        let endpoint = format!("pulls/{}/merge", pr.get());
        let reply = self.api.probe(Method::Post, &endpoint, Some(&body))?;
        if reply.success() {
            return Ok(MergeOutcome::Merged);
        }
        // Another merge of the same pull request may have won meanwhile.
        if self.pull(pr).is_ok_and(|pull| pull.merged) {
            return Ok(MergeOutcome::AlreadyMerged);
        }
        let said = format!("Forgejo answered {} {}", reply.status, reply.message());
        let message = reply.message().to_lowercase();
        let not_yet = (reply.status == NOT_ALLOWED && message.contains("try again later"))
            || (reply.status == CONFLICT
                && message.contains("out of date")
                && !message.contains("head out of date"));
        if not_yet {
            return Err(PortError::Conflict(format!(
                "pull request {pr} is not mergeable yet: {said}"
            )));
        }
        Err(PortError::Rejected(format!(
            "pull request {pr}: Forgejo refused the merge: {said}"
        )))
    }

    /// The latest status of each context at the head, its link the status's target. Forgejo
    /// Actions writes that as a path on the instance, which is made a whole URL.
    fn check_runs(&self, pr: PrNumber) -> Result<Vec<CheckRun>, PortError> {
        let head = self.head_of(pr)?;
        self.statuses(&head)?
            .into_iter()
            .map(|row| {
                Ok(CheckRun {
                    bucket: bucket(pr, &row)?,
                    link: row
                        .target_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|link| !link.is_empty())
                        .map(|link| CheckLink::from(whole_link(self.api.url().as_str(), link))),
                    name: CheckName::from(row.context),
                })
            })
            .collect()
    }

    /// Forgejo has no API for a job's log: a check with a link is read there.
    fn check_log(&self, link: &CheckLink) -> Result<CheckLog, PortError> {
        if link.as_str().trim().is_empty() {
            return Ok(CheckLog::NoJob);
        }
        Ok(CheckLog::AtLink)
    }

    /// The instance's version, then the token: whether it reads the repository and may write
    /// its pull requests and issues (the hand-off is an issue comment). Beside the project's
    /// Forgejo board, only the pull requests: the board asked the rest.
    fn access(&self) -> Vec<AccessCheck> {
        let repo = self.api.repo();
        if self.beside_board {
            let granted = format!("writes {repo}'s pull requests");
            return vec![access::writes_only(
                &self.api,
                &[Write::PullRequests],
                &granted,
            )];
        }
        let granted = format!("reads and writes {repo} and its pull requests");
        access::checks(&self.api, &[Write::PullRequests, Write::Issues], &granted)
    }
}
