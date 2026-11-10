//! Epics: one feature or concern, grouping its tasks, and where it stands.
//!
//! An epic's stage is never stored. It is derived on every read from what the board holds
//! (ruling 48): signed while the hash `zaf epic sign` stored still matches its body, planned
//! once it has tasks, challenged once the challenger's report is on it, and done once every
//! task is done or closed.

use std::str::FromStr;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::{Standing, TicketBody, TicketId, TicketState, TicketTitle, TicketType};
use crate::newtype::text_newtype;

/// The role whose report on an epic means the epic was challenged.
pub const CHALLENGER_ROLE: &str = "challenger";

text_newtype! {
    /// The name in a task's `group:<name>` label.
    GroupName
}

/// A task of an epic as the epic lists it, closed ones included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTask {
    /// Its id.
    pub number: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Its state, if it has one.
    pub state: Option<TicketState>,
    /// Where it stands: open, done (labelled so, or closed as completed), or closed as not
    /// planned (folded into another task or dropped).
    pub standing: Standing,
    /// Its group, if it has one.
    pub group: Option<GroupName>,
    /// Its type, the forge's issue type; none when the issue has no type.
    pub kind: Option<TicketType>,
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
    /// Whether the challenger's report is on it.
    pub challenged: bool,
}

impl Epic {
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

    /// Where it stands: the first stage whose condition it does not yet meet.
    pub fn stage(&self) -> EpicStage {
        if !self.signed() {
            EpicStage::Unsigned
        } else if self.tasks.is_empty() {
            EpicStage::Signed
        } else if !self.challenged {
            EpicStage::Planned
        } else if !self.tasks.iter().all(EpicTask::finished) {
            EpicStage::Challenged
        } else {
            EpicStage::Done
        }
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
