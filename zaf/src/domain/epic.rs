//! Epics: one feature or concern, grouping its tasks, and where it stands.
//!
//! An epic's stage is never stored. It is derived on every read from what the board holds
//! (ruling 48): signed while the hash `zaf epic sign` stored still matches its body, planned
//! once it has tasks, challenged while a challenger's report holds (see [`Epic::challenged`]),
//! and done once every task is finished and at least one is done (an epic whose every task was
//! dropped is not).

use std::str::FromStr;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::plan::read::PlanGraph;
use super::plan::record::{FilingMark, GraphHash, PlanRecord};
use super::{Standing, TicketBody, TicketId, TicketState, TicketTitle, TicketType, TicketUrl};
use crate::newtype::text_newtype;

/// The role whose report on an epic means the epic was challenged.
pub const CHALLENGER_ROLE: &str = "challenger";

/// The graph hash a status report ends in: its last non-blank line, `graph sha256:<hex>`.
///
/// Interface: `report_graph(report)`, which the epic's notes keep for each challenger report.
pub fn report_graph(report: &str) -> Option<GraphHash> {
    let last = report.lines().rfind(|line| !line.trim().is_empty())?;
    last.trim().strip_prefix("graph sha256:")?.parse().ok()
}

/// The role that turns a signed epic into tasks.
pub const PLANNING_ROLE: &str = "planning";

text_newtype! {
    /// The name in a task's `group:<name>` label.
    GroupName
}

/// What the first line of an epic's body opens with when the epic is of a release.
pub const RELEASE_PREFIX: &str = "Release: ";

/// The version an epic's body names: the text after `Release: ` on its first line, and only
/// there. A body whose first line is anything else, that has the line second, or whose
/// version holds a control character (the board's words are not trusted with the terminal), is
/// of no version.
///
/// Interface: `release_of(body)`, the function [`Epic::release`] reads through.
pub fn release_of(body: &str) -> Option<&str> {
    let first = body.lines().next()?;
    let version = first.strip_prefix(RELEASE_PREFIX)?;
    (!version.is_empty() && !version.chars().any(char::is_control)).then_some(version)
}

/// `body` with the `Release: <version>` line as its first line, a blank line, then `body`.
pub fn with_release(version: &str, body: &str) -> String {
    format!("{RELEASE_PREFIX}{version}\n\n{body}")
}

/// The epics of `epics` that are of `version` (see [`Epic::release`]), in the order given.
///
/// Interface: `of_version(epics, version)`; consumed by the phase-due and `zaf begin` tasks.
pub fn of_version<'a>(epics: &'a [Epic], version: &'a str) -> impl Iterator<Item = &'a Epic> + 'a {
    epics
        .iter()
        .filter(move |epic| epic.release() == Some(version))
}

/// Whether at least one open epic is of `version` and every one of them is signed (a signature
/// edited since does not count).
///
/// Interface: `all_signed(epics, version)`; consumed by the release stage and epic signing.
pub fn all_signed(epics: &[Epic], version: &str) -> bool {
    of_version(epics, version).next().is_some() && of_version(epics, version).all(Epic::signed)
}

/// A task of an epic as the epic lists it, closed ones included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTask {
    /// Its id.
    pub number: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Its body, as the epic's read held it.
    pub body: TicketBody,
    /// Where a person opens it.
    pub url: TicketUrl,
    /// Its state, if it has one.
    pub state: Option<TicketState>,
    /// Where it stands: open, done (labelled so, or closed as completed), or closed as not
    /// planned (folded into another task or dropped).
    pub standing: Standing,
    /// Its group, if it has one.
    pub group: Option<GroupName>,
    /// Its type, the forge's issue type; none when the issue has no type.
    pub kind: Option<TicketType>,
    /// When its issue closed; none while it is open, when the board does not say, and for a task
    /// done by its label alone.
    pub closed_at: Option<SystemTime>,
}

impl EpicTask {
    /// Whether it is done: labelled so, or closed as completed.
    pub fn done(&self) -> bool {
        self.standing == Standing::Done
    }

    /// Whether it counts toward its epic: it is open or done, not closed as not planned.
    pub fn counts(&self) -> bool {
        matches!(self.standing, Standing::Open | Standing::Done)
    }

    /// Whether its work is over: it is done, or closed as not planned.
    pub fn finished(&self) -> bool {
        self.standing != Standing::Open
    }
}

/// Why a signature's text does not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// The hash is not 64 hex digits.
    #[error("not a sha256 digest in hex: `{0}`")]
    Hash(String),
}

/// The sha256 of an epic's body, in lowercase hex. Line endings are read as `\n` and
/// trailing whitespace is dropped first, so saving a body unchanged through a web form
/// keeps its hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
pub struct BodyHash(String);

impl BodyHash {
    /// The hash of `body`.
    pub fn of(body: &TicketBody) -> Self {
        let normal = body.as_str().replace("\r\n", "\n");
        let digest = Sha256::digest(normal.trim_end().as_bytes());
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for BodyHash {
    type Err = SignatureError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let hex = text.trim().to_ascii_lowercase();
        let digest = hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        if digest {
            Ok(Self(hex))
        } else {
            Err(SignatureError::Hash(text.to_owned()))
        }
    }
}

/// The Engineer's signature on an epic: the hash of the body they signed, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The hash of the body as it was signed.
    pub hash: BodyHash,
    /// When it was signed.
    pub at: SystemTime,
}

impl Signature {
    /// A signature on `body` now.
    pub fn of(body: &TicketBody, at: SystemTime) -> Self {
        Self {
            hash: BodyHash::of(body),
            at,
        }
    }
}

/// Where an epic stands, derived from the board on every read; each stage holds the ones
/// before it.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    strum::Display,
    strum::IntoStaticStr,
    serde::Serialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EpicStage {
    /// Never signed, or edited since it was: nothing is planned or built.
    #[default]
    Unsigned,
    /// Signed, with no tasks yet: the planning instance's turn.
    Signed,
    /// It has tasks the challenger has not read: the challenger's turn.
    Planned,
    /// Challenged, with tasks still to finish: the teams' turn.
    Challenged,
    /// Every task is finished.
    Done,
}

/// A plan record posted on an epic, when it was posted and the filing mark posted for it.
///
/// Interface: `EpicRecord { record, posted_at, mark }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicRecord {
    /// The record, as posted.
    pub record: PlanRecord,
    /// When it was posted; `None` when the board does not time its comments.
    pub posted_at: Option<SystemTime>,
    /// The latest filing mark posted after it for the same number, when there is one.
    pub mark: Option<FilingMark>,
}

/// A comment that reads as a plan record or a filing mark. Whether to believe it is the
/// reader's to settle from its author: a comment anyone can write reads the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordComment {
    /// It reads as a plan record.
    Record(PlanRecord),
    /// It reads as a filing mark.
    Mark(FilingMark),
}

impl RecordComment {
    /// What `body` reads as, when it is a plan record or a filing mark.
    pub fn read(body: &str) -> Option<Self> {
        if let Ok(record) = body.parse::<PlanRecord>() {
            return Some(Self::Record(record));
        }
        body.parse::<FilingMark>().ok().map(Self::Mark)
    }

    /// Files this comment, posted at `posted`, into `records`, which hold the epic's records so
    /// far in posting order: a record is added at the end; a mark goes on the latest record of
    /// its number, and one for a record not posted yet is dropped.
    pub fn file_into(self, records: &mut Vec<EpicRecord>, posted: Option<SystemTime>) {
        match self {
            Self::Record(record) => records.push(EpicRecord {
                record,
                posted_at: posted,
                mark: None,
            }),
            Self::Mark(mark) => {
                if let Some(filed) = records.iter_mut().rfind(|filed| filed.record.k == mark.k) {
                    filed.mark = Some(mark);
                }
            }
        }
    }
}

/// An open epic on the board, with everything its stage is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epic {
    /// Its id.
    pub id: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Its body, as it reads now.
    pub body: TicketBody,
    /// Its tasks, in id order, closed ones included.
    pub tasks: Vec<EpicTask>,
    /// The latest signature on it, whether or not it still matches the body.
    pub signature: Option<Signature>,
    /// When the latest planning instance's exit marker was posted on it: the moment planning
    /// ended, which no other fact records (the ruling on #459). `None` when no such marker is
    /// on it, or the board does not time its comments.
    pub planned_at: Option<SystemTime>,
    /// The challenger reports on it, oldest first, each as its last line's hash: `Some` for a
    /// report ending in `graph sha256:<hex>`, `None` for one with no such line (D22).
    ///
    /// Interface: `Epic::challengers: Vec<Option<GraphHash>>`; [`Epic::challenged`] reads it.
    pub challengers: Vec<Option<GraphHash>>,
    /// The plan records a trusted login posted on it, in the order they were posted, each with
    /// its filing mark when one was posted (D17, D18, I8).
    ///
    /// Interface: `Epic::records: Vec<EpicRecord>`.
    pub records: Vec<EpicRecord>,
}

impl Epic {
    /// The release it is of: the version its body's first line names (see [`release_of`]).
    ///
    /// Interface: `Epic::release() -> Option<&str>`.
    pub fn release(&self) -> Option<&str> {
        release_of(self.body.as_str())
    }

    /// Whether it is signed: its latest signature holds the hash of its body as it reads now.
    pub fn signed(&self) -> bool {
        self.signature
            .as_ref()
            .is_some_and(|signature| signature.hash == BodyHash::of(&self.body))
    }

    /// Whether it was signed and edited since, so the signature no longer holds.
    pub fn signature_void(&self) -> bool {
        self.signature.is_some() && !self.signed()
    }

    /// Whether a challenger's report holds (D22, Q13). An epic with a plan record is challenged
    /// while one of its challenger reports ends in the hash `PlanGraph::hash` gives its graph as
    /// it reads now, so a body edit voids the challenge and a closed ticket does not. An epic
    /// with no plan record is challenged once any challenger report is on it.
    ///
    /// The graph is read from the epic alone: its hash holds no wait that another epic resolves.
    ///
    /// Interface: `Epic::challenged() -> bool`; consumed by the once-per-graph launch and the
    /// voided state.
    pub fn challenged(&self) -> bool {
        if self.challengers.is_empty() {
            return false;
        }
        if self.records.is_empty() {
            return true;
        }
        let now = PlanGraph::read(self, &[]).hash();
        self.challengers.iter().flatten().any(|hash| *hash == now)
    }

    /// Whether a change voided its challenge: it has a plan record and a challenger's report, and
    /// none carries the hash of its graph as it reads now (D22, Q13). An epic never challenged is
    /// not voided, and neither is one with no plan record, whose challenge no change can void.
    ///
    /// Interface: `Epic::voided() -> bool`; read into `EpicFacts::voided`.
    pub fn voided(&self) -> bool {
        !self.records.is_empty() && !self.challengers.is_empty() && !self.challenged()
    }

    /// Where it stands: the first stage whose condition it does not yet meet.
    pub fn stage(&self) -> EpicStage {
        if !self.signed() {
            EpicStage::Unsigned
        } else if !self.plan_filed() {
            EpicStage::Signed
        } else if !self.challenged() {
            EpicStage::Planned
        } else if !self.finished_done() {
            EpicStage::Challenged
        } else {
            EpicStage::Done
        }
    }

    /// Whether its plan is filed: it has tasks, and when it has plan records its last one has
    /// its filing mark (D17, D19).
    ///
    /// Interface: `Epic::plan_filed() -> bool`; consumed by the stage and by the scheduler's
    /// gather, for a wait on the epic for a name (D13, Q10).
    pub fn plan_filed(&self) -> bool {
        !self.tasks.is_empty() && !self.half_filed()
    }

    /// Whether its plan was posted and not filed: its last plan record has no filing mark, so
    /// the filing stopped part way (D17, D19). An epic with no record is not half filed.
    ///
    /// Interface: `Epic::half_filed() -> bool`; consumed by the chair's half-filed task.
    pub fn half_filed(&self) -> bool {
        self.records
            .last()
            .is_some_and(|filed| filed.mark.is_none())
    }

    /// Whether its work is over and some of it was done: no task is open and at least one is
    /// done. An epic whose every task was dropped has nothing done, so it is not finished.
    ///
    /// Interface: `Epic::finished_done() -> bool`; consumed by the blocker helper, `is_built`
    /// and the every-task-dropped problem.
    pub fn finished_done(&self) -> bool {
        self.tasks.iter().all(EpicTask::finished) && self.tasks.iter().any(EpicTask::done)
    }

    /// Whether it has tasks and every one was dropped: all are closed as not planned, so none is
    /// open and none done. An epic with no tasks has not begun, so it is not this.
    ///
    /// Interface: `Epic::every_task_dropped() -> bool`; consumed by `zaf status` and the hub's
    /// every-task-dropped problem, which read this one predicate.
    pub fn every_task_dropped(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(EpicTask::finished) && !self.finished_done()
    }

    /// Its tasks that are done, and the tasks that count toward it (see [`EpicTask::counts`]).
    pub fn tally(&self) -> (usize, usize) {
        let counted = self.tasks.iter().filter(|task| task.counts());
        counted.fold((0, 0), |(done, total), task| {
            (done + usize::from(task.done()), total + 1)
        })
    }
}

#[cfg(test)]
mod tests;
