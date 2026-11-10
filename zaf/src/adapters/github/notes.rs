//! The comments the board adapter writes and reads back: claims, heartbeats,
//! status reports, rulings and signatures.
//!
//! Claims and heartbeats are plain lines a person can read:
//! `claimed by <instance> at <rfc3339>` and `heartbeat <instance> <rfc3339>`.
//! Reports, rulings and signatures open with an HTML comment marker, invisible on the page,
//! so the adapter finds its own comment again.

use std::fmt;
use std::str::FromStr;
use std::time::SystemTime;

use crate::domain::{Instance, Signature};

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

/// Reads an instance back from its display form.
fn parse_instance(text: &str) -> Result<Instance, NotANote> {
    text.parse().map_err(|_| NotANote)
}

fn parse_time(text: &str) -> Result<SystemTime, NotANote> {
    humantime::parse_rfc3339(text.trim()).map_err(|_| NotANote)
}

/// The comment that records `instance` letting go of a task: `released from <instance> at <rfc3339>`.
pub(super) fn release_line(instance: &Instance, at: SystemTime) -> String {
    format!(
        "released from {instance} at {}",
        humantime::format_rfc3339_seconds(at)
    )
}

/// The first line of an instance's status report on a task.
pub(super) fn report_marker(instance: &Instance) -> String {
    format!("<!-- zaf:report {instance} -->")
}

/// The instance whose status report a comment is, when it opens with a report marker.
pub(super) fn report_author(comment: &str) -> Option<Instance> {
    let first_line = comment.lines().next()?.trim_end();
    let instance = first_line
        .strip_prefix("<!-- zaf:report ")?
        .strip_suffix(" -->")?;
    instance.parse().ok()
}

/// What a signature comment says under its marker.
pub(super) const SIGNATURE_TEXT: &str = "Signed: zaf plans and builds this epic as its body reads now. \
     An edit to the body voids this signature until the epic is signed again.";

/// The first line of a signature: `<!-- zaf:signature sha256=<hex> at=<rfc3339> -->`.
pub(super) fn signature_marker(signature: &Signature) -> String {
    format!(
        "<!-- zaf:signature sha256={} at={} -->",
        signature.hash,
        humantime::format_rfc3339_seconds(signature.at)
    )
}

/// The signature a comment records, when it opens with a signature marker that reads.
pub(super) fn read_signature(comment: &str) -> Option<Signature> {
    let first_line = comment.lines().next()?.trim_end();
    let inner = first_line
        .strip_prefix("<!-- zaf:signature ")?
        .strip_suffix(" -->")?;
    let (hash, at) = inner.split_once(' ')?;
    Some(Signature {
        hash: hash.strip_prefix("sha256=")?.parse().ok()?,
        at: parse_time(at.strip_prefix("at=")?).ok()?,
    })
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
