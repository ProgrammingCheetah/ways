//! The forge: pull requests, their checks and review comments, and the merge.

use super::PortError;
use super::{BaseName, SliceName};
use crate::domain::Location;
use crate::newtype::text_newtype;

/// The number of a pull request on the forge.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::From,
    derive_more::Display,
)]
#[display("#{_0}")]
pub struct PrNumber(u64);

impl PrNumber {
    /// The bare number, for adapters that address pull requests numerically.
    pub fn get(self) -> u64 {
        self.0
    }
}

text_newtype! {
    /// The name of a check run (`ci`, a scanner, a bot).
    CheckName
}

text_newtype! {
    /// Who wrote a review comment.
    Author
}

/// Where a check run stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum CheckState {
    /// Queued or running.
    Pending,
    /// Finished green.
    Passed,
    /// Finished red.
    Failed,
}

/// One check run on a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Its name.
    pub name: CheckName,
    /// Where it stands.
    pub state: CheckState,
}

/// One review comment on a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    /// Who wrote it.
    pub author: Author,
    /// What it says.
    pub body: String,
    /// Where it points, when it is on a line.
    pub location: Option<Location>,
    /// The commit it was made on, when the forge says.
    pub head: Option<String>,
}

text_newtype! {
    /// The id of the rule a code scanner flagged (`js/sql-injection`).
    RuleId
}

/// One open code scanning alert on a pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanningAlert {
    /// The rule that fired.
    pub rule: RuleId,
    /// Where it fired, when the scanner said.
    pub location: Option<Location>,
}

/// What a pull request is opened from and into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRequest {
    /// The slice it carries.
    pub slice: SliceName,
    /// The line it targets.
    pub base: BaseName,
    /// Its title.
    pub title: String,
    /// Its body.
    pub body: String,
}

/// What merging a pull request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrMerge {
    /// It merged now.
    Merged,
    /// It had merged before; nothing changed.
    AlreadyMerged,
}

/// The forge that hosts the code and its reviews.
pub trait Forge {
    /// Opens a pull request for a slice; a slice with an open one gets that one back.
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError>;

    /// The pull request's check runs.
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError>;

    /// The pull request's review comments, oldest first.
    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError>;

    /// The commit id the pull request's head is at now, when the forge reports one.
    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// The open code scanning alerts for the pull request's head ref.
    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError>;

    /// Merges the pull request; one already merged succeeds with [`PrMerge::AlreadyMerged`].
    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError>;
}

#[cfg(test)]
mod tests;
