//! A step's report read as findings, and the findings of every step deduplicated.
//!
//! A report line is `MUST: <path>:<line> <title>` or `NOTE: <path>:<line> <title>`;
//! every other line is prose and ignored. Two findings are one when they share
//! the path, sit within [`LINE_WINDOW`] lines of each other, and their titles
//! match once normalized.

use crate::domain::{Finding, FindingSource, Location, Severity};

/// How far apart two lines may be and still name one finding.
pub const LINE_WINDOW: u32 = 3;

/// Reads a report's `MUST:` and `NOTE:` lines as findings from `source`.
///
/// A line whose location does not parse keeps its whole text as the title and
/// no location, so a must-fix is never dropped for a typo in its path.
pub fn parse_report(source: &FindingSource, report: &str) -> Vec<Finding> {
    report
        .lines()
        .filter_map(|line| parse_line(source, line.trim()))
        .collect()
}

fn parse_line(source: &FindingSource, line: &str) -> Option<Finding> {
    let (severity, rest) = if let Some(rest) = line.strip_prefix("MUST:") {
        (Severity::MustFix, rest)
    } else {
        (Severity::Advisory, line.strip_prefix("NOTE:")?)
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let (head, title) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(head, title)| (head, title.trim()));
    let location = parse_location(head).filter(|_| !title.is_empty());
    let message = if location.is_some() { title } else { rest };
    let finding = Finding {
        source: source.clone(),
        severity,
        message: message.to_owned(),
        location,
    };
    Some(finding)
}

fn parse_location(text: &str) -> Option<Location> {
    let (path, line) = text.rsplit_once(':')?;
    let line = line.parse::<u32>().ok()?;
    if path.is_empty() {
        return None;
    }
    let location = Location {
        path: path.to_owned(),
        line: Some(line),
    };
    Some(location)
}

/// A finding after deduplication, with the other sources that reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFinding {
    /// The first report of it, its severity raised to the gravest duplicate's.
    pub finding: Finding,
    /// Every later source that reported the same finding, in step order.
    pub also_found_by: Vec<FindingSource>,
}

/// Merges findings that name the same thing; the first report of each is kept, in order.
pub fn dedupe(findings: Vec<Finding>) -> Vec<CheckedFinding> {
    let mut kept: Vec<CheckedFinding> = Vec::new();
    for finding in findings {
        let duplicate = kept
            .iter_mut()
            .find(|checked| same_finding(&checked.finding, &finding));
        match duplicate {
            Some(checked) => {
                checked.finding.severity = checked.finding.severity.min(finding.severity);
                let new_source = finding.source != checked.finding.source
                    && !checked.also_found_by.contains(&finding.source);
                if new_source {
                    checked.also_found_by.push(finding.source);
                }
            }
            None => kept.push(CheckedFinding {
                finding,
                also_found_by: Vec::new(),
            }),
        }
    }
    kept
}

fn same_finding(first: &Finding, second: &Finding) -> bool {
    let same_place = match (&first.location, &second.location) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            first.path == second.path
                && match (first.line, second.line) {
                    (Some(first), Some(second)) => first.abs_diff(second) <= LINE_WINDOW,
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => false,
    };
    same_place && normalize(&first.message) == normalize(&second.message)
}

/// Lowercase, every run of non-alphanumerics one space, trimmed.
fn normalize(title: &str) -> String {
    let lowered: String = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
