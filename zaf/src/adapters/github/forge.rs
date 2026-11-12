//! The forge on GitHub: forge kind `github`.
//!
//! Every call is gh's, through [`GhCommand`]. The writes `zaf forge` hands over (opening a pull
//! request, merging one, listing its checks and a failed job's log) are the gh calls the standard
//! actions made themselves before they moved behind the port, argument for argument.

use std::time::SystemTime;

use serde::Deserialize;

use super::GhCommand;
use super::rows::{
    CheckRow, ConversationCommentRow, FileRow, LineCommentRow, PrStateRow, PrTimesRow, ReviewRow,
    UserRow,
};
use crate::adapters::notes::{HANDOFF_MARKER, marked, unmarked};
use crate::domain::Location;
use crate::ports::{
    Author, Check, CheckBucket, CheckLink, CheckLog, CheckName, CheckRun, CheckState, CommitId,
    Forge, JobId, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PortError, PrNumber,
    PullRequest, ReviewComment, RuleId, ScanningAlert,
};

/// The author shown for a comment whose account was deleted, as GitHub shows it.
const GHOST: &str = "ghost";
/// What gh prints when a pull request has no check runs at all.
const NO_CHECKS: &str = "no checks reported";
/// What gh's refusal says, in any case, when the merge may go through if tried again: another
/// merge moved the base, or GitHub is still working out whether it merges.
const NOT_YET: &[&str] = &["base branch was modified", "not mergeable"];
/// What separates an Actions job's id from the rest of a check's link.
const JOB_SEGMENT: &str = "/job/";
/// The jq program that prints one tab-separated line per check: bucket, name and link.
const CHECKS_JQ: &str = ".[] | [.bucket, .name, .link] | @tsv";

/// The pull requests of one GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct GithubForge {
    gh: GhCommand,
}

/// The bare number gh takes, without the `#` a [`PrNumber`] displays with.
fn bare(pr: PrNumber) -> String {
    pr.get().to_string()
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
                let state = row.bucket.state();
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

    /// The head as `ci-wait` read it: `pr view --json headRefOid --jq .headRefOid`, one line.
    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        let number = bare(pr);
        let stdout = self.gh.text(&[
            "pr",
            "view",
            &number,
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])?;
        let head = stdout.trim();
        Ok((!head.is_empty() && head != "null").then(|| head.to_owned()))
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

    /// `pr list --head <head> --state open`, then `pr create` when it lists none: the number is
    /// the last part of the URL gh prints last.
    fn open_pull_request(&self, request: &NewPullRequest) -> Result<Opened, PortError> {
        let head = request.head.as_str();
        let listed = self
            .gh
            .text(&[
                "pr",
                "list",
                "--head",
                head,
                "--state",
                "open",
                "--json",
                "number",
                "--jq",
                ".[].number",
            ])
            .map_err(PortError::into_backend)?;
        if let Some(first) = listed.split_whitespace().next() {
            let number = first.parse::<u64>().map_err(|_| {
                PortError::Backend(format!("gh listed {first}, not a pull request number"))
            })?;
            return Ok(Opened::AlreadyOpen(PrNumber::from(number)));
        }
        let created = self
            .gh
            .text(&[
                "pr",
                "create",
                "--head",
                head,
                "--base",
                request.base.as_str(),
                "--title",
                request.title.as_str(),
                "--body",
                request.body.as_str(),
            ])
            .map_err(PortError::into_backend)?;
        let url = created
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim();
        let number = url
            .rsplit('/')
            .next()
            .and_then(|last| last.parse::<u64>().ok())
            .ok_or_else(|| {
                PortError::Backend(format!(
                    "gh printed no pull request number: {}",
                    created.trim()
                ))
            })?;
        Ok(Opened::New(PrNumber::from(number)))
    }

    /// `pr view --json state --jq .state`, then `pr merge <n> --merge|--squash
    /// [--match-head-commit <sha>]`; gh's words decide a refusal: a moved base or a pull request
    /// not mergeable is one for now.
    fn merge_pull_request(
        &self,
        pr: PrNumber,
        strategy: MergeStrategy,
        head: Option<&CommitId>,
    ) -> Result<MergeOutcome, PortError> {
        let number = bare(pr);
        let state = self
            .gh
            .text(&["pr", "view", &number, "--json", "state", "--jq", ".state"])
            .map_err(PortError::into_backend)?;
        match state.trim() {
            "MERGED" => return Ok(MergeOutcome::AlreadyMerged),
            "CLOSED" => {
                return Err(PortError::Rejected(format!(
                    "pull request {pr} is closed unmerged"
                )));
            }
            "" => {
                return Err(PortError::Backend(format!(
                    "gh printed no state for pull request {pr}"
                )));
            }
            _ => {}
        }
        let strategy_flag = format!("--{strategy}");
        let mut argv = vec!["pr", "merge", number.as_str(), strategy_flag.as_str()];
        if let Some(head) = head {
            argv.extend(["--match-head-commit", head.as_str()]);
        }
        let run = self.gh.run(&argv)?;
        if run.success {
            return Ok(MergeOutcome::Merged);
        }
        let said = [run.stdout.trim(), run.stderr.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let lower = said.to_lowercase();
        if NOT_YET.iter().any(|words| lower.contains(words)) {
            return Err(PortError::Conflict(format!(
                "pull request {pr} is not mergeable yet: gh pr merge: {said}"
            )));
        }
        Err(PortError::Rejected(format!(
            "pull request {pr}: gh pr merge refused: {said}"
        )))
    }

    /// `pr checks --json bucket,name,link` through jq, one tab-separated line per check. gh exits
    /// non-zero while a check is pending or failed, so its words decide, not its code: the lines
    /// it printed are the checks, and `no checks reported` is none.
    fn check_runs(&self, pr: PrNumber) -> Result<Vec<CheckRun>, PortError> {
        let number = bare(pr);
        let run = self.gh.run(&[
            "pr",
            "checks",
            &number,
            "--json",
            "bucket,name,link",
            "--jq",
            CHECKS_JQ,
        ])?;
        let lines = if run.success {
            run.stdout
        } else if run.stderr.contains(NO_CHECKS) {
            String::new()
        } else if run.stdout.trim().is_empty() {
            return Err(PortError::Backend(format!("gh pr checks: {}", run.stderr)));
        } else {
            run.stdout
        };
        lines
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(check_run)
            .collect()
    }

    /// An Actions job's link (`…/job/<id>`) is read with `run view --job <id> --log-failed`; any
    /// other link names no job gh can read.
    fn check_log(&self, link: &CheckLink) -> Result<CheckLog, PortError> {
        let Some(job) = job_of(link) else {
            return Ok(CheckLog::NoJob);
        };
        let run = self
            .gh
            .run(&["run", "view", "--job", job.as_str(), "--log-failed"])?;
        if !run.success {
            return Err(PortError::Backend(format!(
                "gh run view --job {job}: {}",
                run.stderr
            )));
        }
        let mut lines = run.stdout;
        if !run.stderr.is_empty() {
            lines.push_str(&run.stderr);
            lines.push('\n');
        }
        Ok(CheckLog::Failed { job, lines })
    }
}

/// One line of `pr checks` through [`CHECKS_JQ`]: bucket, name and link, tab-separated.
fn check_run(line: &str) -> Result<CheckRun, PortError> {
    let mut fields = line.splitn(3, '\t');
    let word = fields.next().unwrap_or_default().trim();
    let bucket = word
        .parse::<CheckBucket>()
        .map_err(|_| PortError::Backend(format!("gh printed an unknown check bucket `{word}`")))?;
    let name = fields.next().unwrap_or_default();
    let link = fields.next().map(str::trim).filter(|link| !link.is_empty());
    Ok(CheckRun {
        bucket,
        name: CheckName::from(name),
        link: link.map(CheckLink::from),
    })
}

/// The Actions job a check's link names: the digits after its `/job/`.
fn job_of(link: &CheckLink) -> Option<JobId> {
    let (_, after) = link.as_str().split_once(JOB_SEGMENT)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| JobId::from(digits))
}

impl GithubForge {
    /// The pull request's conversation comments, oldest first.
    fn conversation(&self, pr: PrNumber) -> Result<Vec<ConversationCommentRow>, PortError> {
        self.gh.api_list(&format!("issues/{}/comments", bare(pr)))
    }
}
