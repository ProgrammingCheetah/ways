//! The plan record and the filing mark (I8): two comments zaf posts on an epic and reads back.
//!
//! A plan record is the plan as the planner wrote it, kept so a half-filed plan can be finished
//! (D17) and the graph read later (D18):
//!
//! ````text
//! <!-- zaf:plan-record 2 -->
//! design sha256:<64 lowercase hex digits>
//! dropped: #4 #7
//! ```
//! <the whole plan as written>
//! ```
//! ````
//!
//! The fence is three backticks, or one more than the longest run of backticks in the plan, so a
//! plan that holds a fence round-trips. `dropped:` is on its line with no tickets when none is.
//!
//! A filing mark says the record `k` was filed and what graph the epic then read back as:
//!
//! ```text
//! <!-- zaf:plan-filed 2 -->
//! graph sha256:<64 lowercase hex digits>
//! ```
//!
//! Each comment opens with its marker on a line of its own, as a signature does. A comment
//! without the marker is [`RecordError::NotMarked`]; one with it whose lines do not read is
//! [`RecordError::Unreadable`]. Both are "not a record" (or "not a mark") to a reader.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::domain::{BodyHash, TicketId};

const RECORD_MARKER: &str = "<!-- zaf:plan-record ";
const FILED_MARKER: &str = "<!-- zaf:plan-filed ";
const MARKER_END: &str = " -->";
const DESIGN: &str = "design sha256:";
const GRAPH: &str = "graph sha256:";
const DROPPED: &str = "dropped:";
/// The fewest backticks in a fence.
const MIN_FENCE: usize = 3;

/// Why a comment is not a plan record or a filing mark.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The comment does not open with the marker: it is somebody else's.
    #[error("not a zaf plan comment: it does not open with `{0}<k>{MARKER_END}`")]
    NotMarked(&'static str),
    /// The marker is there and a line after it does not read.
    #[error("line {line}: {reason}")]
    Unreadable {
        /// The 1-based line at fault.
        line: usize,
        /// What is wrong there.
        reason: String,
    },
    /// The text is not 64 lowercase hex digits.
    #[error("not a graph hash (64 lowercase hex digits): `{0}`")]
    GraphHash(String),
}

fn unreadable(line: usize, reason: impl Into<String>) -> RecordError {
    RecordError::Unreadable {
        line,
        reason: reason.into(),
    }
}

/// The sha256 of a graph, in 64 lowercase hex digits. `PlanGraph::hash` computes one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub struct GraphHash(String);

impl GraphHash {
    /// The sha256 of `canonical`, which a graph writes as its canonical form.
    pub(crate) fn of(canonical: &str) -> Self {
        let digest = Sha256::digest(canonical.as_bytes());
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for GraphHash {
    type Err = RecordError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digest =
            text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if digest {
            Ok(Self(text.to_owned()))
        } else {
            Err(RecordError::GraphHash(text.to_owned()))
        }
    }
}

/// A plan record: plan number `k` of an epic, the design it was written against, the tickets it
/// dropped, and the plan as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRecord {
    /// The record's number among the epic's records.
    pub k: u64,
    /// The hash of the design the plan was made from.
    pub design: BodyHash,
    /// The unstarted tickets this plan drops (D29).
    pub dropped: Vec<TicketId>,
    /// The whole plan, byte for byte.
    pub plan: String,
}

/// A filing mark: record `k` is filed, and the epic read back as this graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingMark {
    /// The number of the record filed.
    pub k: u64,
    /// The hash of the graph read back after filing.
    pub graph: GraphHash,
}

/// The number in a marker line; `NotMarked` when the line is not that marker at all.
fn marker_number(first: &str, marker: &'static str) -> Result<u64, RecordError> {
    let rest = first
        .strip_prefix(marker)
        .and_then(|rest| rest.strip_suffix(MARKER_END))
        .ok_or(RecordError::NotMarked(marker))?;
    let digits = !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
    rest.parse::<u64>()
        .ok()
        .filter(|_| digits)
        .ok_or_else(|| unreadable(1, format!("`{rest}` is not a record number")))
}

/// A line's text without its line ending.
fn content(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// The hash on line `number`, which reads `<prefix><hex>`.
fn hash_line<T: FromStr>(
    line: Option<&str>,
    number: usize,
    prefix: &str,
) -> Result<T, RecordError> {
    let text = line
        .and_then(|line| line.strip_prefix(prefix))
        .ok_or_else(|| unreadable(number, format!("expected a line `{prefix}<hex>`")))?;
    text.parse()
        .map_err(|_| unreadable(number, format!("`{text}` is not 64 lowercase hex digits")))
}

fn dropped_line(line: Option<&str>) -> Result<Vec<TicketId>, RecordError> {
    let list = line
        .and_then(|line| line.strip_prefix(DROPPED))
        .ok_or_else(|| unreadable(3, format!("expected a line `{DROPPED} #<n> ...`")))?;
    list.split_whitespace()
        .map(|word| {
            word.starts_with('#')
                .then(|| word.parse().ok())
                .flatten()
                .ok_or_else(|| unreadable(3, format!("`{word}` is not a ticket")))
        })
        .collect()
}

/// The longest run of backticks in `text`.
fn longest_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

impl fmt::Display for PlanRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fence = "`".repeat((longest_run(&self.plan) + 1).max(MIN_FENCE));
        writeln!(f, "{RECORD_MARKER}{}{MARKER_END}", self.k)?;
        writeln!(f, "{DESIGN}{}", self.design)?;
        write!(f, "{DROPPED}")?;
        for ticket in &self.dropped {
            write!(f, " {ticket}")?;
        }
        write!(f, "\n{fence}\n{}\n{fence}", self.plan)
    }
}

impl FromStr for PlanRecord {
    type Err = RecordError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut lines = text.splitn(5, '\n');
        let k = marker_number(content(lines.next().unwrap_or("")), RECORD_MARKER)?;
        let design = hash_line(lines.next().map(content), 2, DESIGN)?;
        let dropped = dropped_line(lines.next().map(content))?;
        let fence = lines.next().map(content).unwrap_or("");
        if fence.len() < MIN_FENCE || fence.bytes().any(|b| b != b'`') {
            return Err(unreadable(
                4,
                "expected the plan's opening fence of backticks",
            ));
        }
        let closing = format!("\n{fence}");
        let plan = lines
            .next()
            .unwrap_or("")
            .trim_end()
            .strip_suffix(closing.as_str())
            .ok_or_else(|| unreadable(5, "the plan's closing fence is missing"))?;
        Ok(Self {
            k,
            design,
            dropped,
            plan: plan.to_owned(),
        })
    }
}

impl fmt::Display for FilingMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{FILED_MARKER}{}{MARKER_END}", self.k)?;
        write!(f, "{GRAPH}{}", self.graph)
    }
}

impl FromStr for FilingMark {
    type Err = RecordError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut lines = text.lines();
        let k = marker_number(lines.next().unwrap_or(""), FILED_MARKER)?;
        let graph = hash_line(lines.next(), 2, GRAPH)?;
        match lines.find(|line| !line.trim().is_empty()) {
            Some(_) => Err(unreadable(3, "nothing follows the graph hash")),
            None => Ok(Self { k, graph }),
        }
    }
}

#[cfg(test)]
mod tests;
