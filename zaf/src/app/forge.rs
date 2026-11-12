//! `zaf forge open|merge|checks|head|log`: the forge's writes and the reads around them that the
//! standard actions hand to zaf (`pr-open`, `pr-merge`, `ci-wait`), so one set of actions serves
//! every forge the manifest can name.
//!
//! Each use case checks what it is given, then makes one port call; the waiting and the retries
//! stay in the actions.

use crate::ports::{
    BranchName, CheckLink, CheckLog, CheckName, CheckRun, CommitId, Forge, MergeOutcome,
    MergeStrategy, NewPullRequest, Opened, PortError, PrNumber,
};

/// The most lines of a failed job's log `zaf forge log` prints: its last ones, where the failure
/// is.
pub const LOG_TAIL: usize = 80;

/// Why a forge verb did not run its call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ForgeError {
    /// A head or base the forge cannot take as a branch: empty, or read as an option.
    #[error("{what} `{name}` is not a usable branch name")]
    Branch {
        /// Which of the two it is: `head` or `base`.
        what: &'static str,
        /// The name as given.
        name: BranchName,
    },
    /// A pull request with no title.
    #[error("a pull request needs a title")]
    NoTitle,
    /// A head to pin the merge to that is no commit id: lowercase hex only.
    #[error("`{0}` is not a commit id")]
    NotACommit(String),
    /// The forge's call failed.
    #[error(transparent)]
    Port(#[from] PortError),
}

/// `zaf forge open`: opens the pull request, or hands back the one open for its head.
pub fn open(forge: &dyn Forge, request: &NewPullRequest) -> Result<Opened, ForgeError> {
    for (what, name) in [("head", &request.head), ("base", &request.base)] {
        if !name.is_usable() {
            return Err(ForgeError::Branch {
                what,
                name: name.clone(),
            });
        }
    }
    if request.title.as_str().is_empty() {
        return Err(ForgeError::NoTitle);
    }
    Ok(forge.open_pull_request(request)?)
}

/// `zaf forge merge`: merges the pull request with `strategy`, at `head` when one is named.
pub fn merge(
    forge: &dyn Forge,
    pr: PrNumber,
    strategy: MergeStrategy,
    head: Option<&str>,
) -> Result<MergeOutcome, ForgeError> {
    let head = head.map(commit).transpose()?;
    Ok(forge.merge_pull_request(pr, strategy, head.as_ref())?)
}

/// A commit id as `--match-head-commit` takes it: lowercase hex, as `jj` and `git` print one.
fn commit(text: &str) -> Result<CommitId, ForgeError> {
    let hex = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !hex {
        return Err(ForgeError::NotACommit(text.to_owned()));
    }
    Ok(CommitId::from(text))
}

/// `zaf forge checks`: the checks of the pull request's head.
pub fn checks(forge: &dyn Forge, pr: PrNumber) -> Result<Vec<CheckRun>, PortError> {
    forge.check_runs(pr)
}

/// One check as `zaf forge checks` prints it: bucket, name and link, tab-separated; the link is
/// empty when the forge gives none.
pub fn check_line(run: &CheckRun) -> String {
    let link = run.link.as_ref().map_or("", CheckLink::as_str);
    format!("{}\t{}\t{link}", run.bucket, run.name)
}

/// `zaf forge head`: the commit the pull request's head is at, when the forge reports one.
pub fn head(forge: &dyn Forge, pr: PrNumber) -> Result<Option<String>, PortError> {
    forge.head(pr)
}

/// `zaf forge log`: what a person reads of a failed check's log, a header line naming the check
/// first. A job's failed lines are cut to their last [`LOG_TAIL`]; a forge with no log API
/// gives the check's link instead; a check whose link names no job gives nothing.
pub fn log(
    forge: &dyn Forge,
    name: &CheckName,
    link: &CheckLink,
) -> Result<Option<String>, PortError> {
    let text = match forge.check_log(link)? {
        CheckLog::Failed { job, lines } => {
            let all: Vec<&str> = lines.lines().collect();
            let tail = &all[all.len().saturating_sub(LOG_TAIL)..];
            let mut text = format!("--- failed log lines of {name} (job {job})\n");
            for line in tail {
                text.push_str(line);
                text.push('\n');
            }
            text
        }
        CheckLog::AtLink => {
            format!("--- {name}: the forge has no API for a job's log; read it at {link}\n")
        }
        CheckLog::NoJob => return Ok(None),
    };
    Ok(Some(text))
}

#[cfg(test)]
mod tests;
