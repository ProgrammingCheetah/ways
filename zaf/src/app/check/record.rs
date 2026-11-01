//! The result of a check as it is printed and stored: one line per checked thing.
//!
//! The record is what `--status` reads back, so a check prints the same text
//! (or `--json`) whether it just ran or ran earlier.

use std::fmt;

use super::engine::{CheckReport, StepStatus};
use super::task::{LineStatus, TaskCheck};
use super::{Target, Verdict};
use crate::domain::Severity;

/// One line of a check's result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecordLine {
    /// `pass`, `fail`, `manual`, `skipped`; `ran`, `failed`, `timed-out`; `must`, `note`.
    pub status: String,
    /// The line's kind or the finding's source.
    pub source: String,
    /// `path:line` or `path`, for a finding that gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// The subject, the reason or the finding's title.
    pub text: String,
    /// The other sources that reported the same finding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,
}

/// A whole check's result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckRecord {
    /// The target, as [`Target`] displays it.
    pub target: String,
    /// How it ended; its code is the exit code.
    pub verdict: Verdict,
    /// Every line, in the order checked.
    pub lines: Vec<RecordLine>,
}

impl CheckRecord {
    /// A task check's record.
    pub fn from_task(target: &Target, check: &TaskCheck) -> Self {
        let lines = check
            .lines
            .iter()
            .map(|line| {
                let (status, text) = match &line.status {
                    LineStatus::Pass => ("pass", line.subject.clone()),
                    LineStatus::Fail(why) => ("fail", format!("{}: {why}", line.subject)),
                    LineStatus::Manual => ("manual", line.subject.clone()),
                    LineStatus::Skipped(why) => ("skipped", format!("{}: {why}", line.subject)),
                };
                record_line(status, line.kind.to_string(), None, text)
            })
            .collect();
        Self {
            target: target.to_string(),
            verdict: check.verdict(),
            lines,
        }
    }

    /// A pull request check's record: the steps, then the findings.
    pub fn from_report(target: &Target, report: &CheckReport) -> Self {
        let steps = report.steps.iter().map(|result| {
            let (status, text) = match &result.status {
                StepStatus::Ran => ("ran", String::new()),
                StepStatus::Skipped => ("skipped", "the task is not security-flagged".to_owned()),
                StepStatus::Failed(why) => ("failed", why.clone()),
                StepStatus::TimedOut => ("timed-out", String::new()),
            };
            record_line(status, result.step.to_string(), None, text)
        });
        let findings = report.findings.iter().map(|checked| {
            let finding = &checked.finding;
            let status = match finding.severity {
                Severity::MustFix => "must",
                Severity::Advisory => "note",
            };
            let location = finding
                .location
                .as_ref()
                .map(|location| match location.line {
                    Some(line) => format!("{}:{line}", location.path),
                    None => location.path.clone(),
                });
            let mut line = record_line(
                status,
                finding.source.to_string(),
                location,
                finding.message.clone(),
            );
            line.also = checked
                .also_found_by
                .iter()
                .map(ToString::to_string)
                .collect();
            line
        });
        Self {
            target: target.to_string(),
            verdict: report.verdict(),
            lines: steps.chain(findings).collect(),
        }
    }
}

fn record_line(status: &str, source: String, location: Option<String>, text: String) -> RecordLine {
    RecordLine {
        status: status.to_owned(),
        source,
        location,
        text,
        also: Vec::new(),
    }
}

impl fmt::Display for RecordLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:<9} {}", self.status, self.source)?;
        if let Some(location) = &self.location {
            write!(formatter, " {location}")?;
        }
        if !self.text.is_empty() {
            write!(formatter, " {}", self.text)?;
        }
        if !self.also.is_empty() {
            write!(formatter, " (also {})", self.also.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Display for CheckRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in &self.lines {
            writeln!(formatter, "{line}")?;
        }
        write!(formatter, "{}: {}", self.target, self.verdict)
    }
}

#[cfg(test)]
mod tests;
