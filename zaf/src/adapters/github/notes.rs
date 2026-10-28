//! The comments the board adapter writes and reads back: claims, heartbeats,
//! status reports and rulings.
//!
//! Claims and heartbeats are plain lines a person can read:
//! `claimed by <instance> at <rfc3339>` and `heartbeat <instance> <rfc3339>`.
//! Reports and rulings open with an HTML comment marker, invisible on the page,
//! so the adapter finds its own comment again.

use std::fmt;
use std::str::FromStr;
use std::time::SystemTime;

use crate::domain::{Instance, InstanceNumber, IssueNumber, RoleName};

/// A comment that is not a claim or a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a claim or heartbeat line")]
pub(super) struct NotANote;

/// A lease line: who claimed or touched a task, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LeaseNote {
    /// `claimed by <instance> at <rfc3339>`.
    Claim {
        /// The new holder.
        instance: Instance,
        /// When.
        at: SystemTime,
    },
    /// `heartbeat <instance> <rfc3339>`.
    Heartbeat {
        /// The holder.
        instance: Instance,
        /// When.
        at: SystemTime,
    },
}

const CLAIM_PREFIX: &str = "claimed by ";
const CLAIM_TIME_SEPARATOR: &str = " at ";
const HEARTBEAT_PREFIX: &str = "heartbeat ";

impl fmt::Display for LeaseNote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claim { instance, at } => write!(
                formatter,
                "{CLAIM_PREFIX}{instance}{CLAIM_TIME_SEPARATOR}{}",
                humantime::format_rfc3339_seconds(*at)
            ),
            Self::Heartbeat { instance, at } => write!(
                formatter,
                "{HEARTBEAT_PREFIX}{instance} {}",
                humantime::format_rfc3339_seconds(*at)
            ),
        }
    }
}

impl FromStr for LeaseNote {
    type Err = NotANote;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let line = text.trim();
        if let Some(rest) = line.strip_prefix(CLAIM_PREFIX) {
            let (instance, at) = rest.rsplit_once(CLAIM_TIME_SEPARATOR).ok_or(NotANote)?;
            let claim = Self::Claim {
                instance: parse_instance(instance)?,
                at: parse_time(at)?,
            };
            return Ok(claim);
        }
        let rest = line.strip_prefix(HEARTBEAT_PREFIX).ok_or(NotANote)?;
        let (instance, at) = rest.rsplit_once(' ').ok_or(NotANote)?;
        let heartbeat = Self::Heartbeat {
            instance: parse_instance(instance)?,
            at: parse_time(at)?,
        };
        Ok(heartbeat)
    }
}

/// Reads an instance back from its display form, `<role> instance <n> on #<epic>`.
fn parse_instance(text: &str) -> Result<Instance, NotANote> {
    let (role, rest) = text.split_once(" instance ").ok_or(NotANote)?;
    let (number, epic) = rest.split_once(" on ").ok_or(NotANote)?;
    let number = number.parse::<u32>().map_err(|_| NotANote)?;
    let epic = epic.parse::<IssueNumber>().map_err(|_| NotANote)?;
    if role.is_empty() {
        return Err(NotANote);
    }
    let instance = Instance {
        role: RoleName::from(role),
        epic,
        number: InstanceNumber::from(number),
    };
    Ok(instance)
}

fn parse_time(text: &str) -> Result<SystemTime, NotANote> {
    humantime::parse_rfc3339(text.trim()).map_err(|_| NotANote)
}

/// The first line of an instance's status report on a task.
pub(super) fn report_marker(instance: &Instance) -> String {
    format!("<!-- zaf:report {instance} -->")
}

/// The first line of the ruling on a question.
pub(super) const RULING_MARKER: &str = "<!-- zaf:ruling -->";

/// A comment body that opens with `marker` on its own line.
pub(super) fn marked(marker: &str, body: &str) -> String {
    format!("{marker}\n{body}")
}

/// The text after `marker`, when the comment opens with it.
pub(super) fn unmarked<'a>(marker: &str, comment: &'a str) -> Option<&'a str> {
    let (first_line, rest) = comment.split_once('\n').unwrap_or((comment, ""));
    (first_line.trim_end() == marker).then_some(rest)
}
