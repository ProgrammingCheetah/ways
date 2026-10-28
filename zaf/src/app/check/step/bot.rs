//! `bot:<login>`: wait until the login has posted on the pull request, then read
//! its line comments as findings.
//!
//! A comment is a must-fix when a line of its body mentions a "potential issue"
//! or "security" marker (case ignored), a note otherwise. Its title is the first
//! other non-blank line, stripped of markdown emphasis; its place is the
//! comment's own. A comment with no place (a review's summary) only proves the
//! bot has reviewed and yields no finding.

use std::thread;
use std::time::Instant;

use super::StepContext;
use crate::app::check::StepPorts;
use crate::domain::{Finding, FindingSource, Severity};
use crate::ports::{Author, PortError, ReviewComment};

/// The phrases that mark a bot comment as a must-fix.
const MARKERS: [&str; 2] = ["potential issue", "security"];

pub(super) fn run(
    source: &FindingSource,
    login: &Author,
    ports: &dyn StepPorts,
    context: &StepContext,
) -> Result<Vec<Finding>, PortError> {
    let forge = ports.forge()?;
    loop {
        let comments: Vec<ReviewComment> = forge
            .review_comments(context.pr)?
            .into_iter()
            .filter(|comment| comment.author.as_ref().eq_ignore_ascii_case(login.as_ref()))
            .collect();
        if !comments.is_empty() {
            let findings = comments
                .into_iter()
                .filter_map(|comment| finding(source, comment))
                .collect();
            return Ok(findings);
        }
        let left = context.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(PortError::Unavailable(format!(
                "{login} has not reviewed {} yet",
                context.pr
            )));
        }
        thread::sleep(context.poll.min(left));
    }
}

fn finding(source: &FindingSource, comment: ReviewComment) -> Option<Finding> {
    let location = comment.location?;
    let (marker_lines, other_lines): (Vec<&str>, Vec<&str>) = comment
        .body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .partition(|line| is_marker(line));
    let severity = if marker_lines.is_empty() {
        Severity::Advisory
    } else {
        Severity::MustFix
    };
    let line = other_lines.first().or(marker_lines.first())?;
    let stripped = strip_markup(line);
    let title = if stripped.is_empty() {
        (*line).to_owned()
    } else {
        stripped
    };
    Some(Finding {
        source: source.clone(),
        severity,
        message: title,
        location: Some(location),
    })
}

fn is_marker(line: &str) -> bool {
    let lowered = line.to_lowercase();
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

fn strip_markup(line: &str) -> String {
    line.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '*' | '_' | '#' | '`')
    })
    .to_owned()
}

#[cfg(test)]
mod tests;
