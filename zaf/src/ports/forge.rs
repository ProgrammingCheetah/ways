//! The forge: pull requests, their checks and review comments, and the merge.

use super::PortError;
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

/// The forge that hosts the code and its reviews; zaf reads it, and writes one comment on it:
/// the hand-off.
pub trait Forge {
    /// The pull request's check runs.
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError>;

    /// The pull request's review comments, oldest first.
    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError>;

    /// The commit id the pull request's head is at now, when the forge reports one.
    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// Whether the pull request has merged; a read, it changes nothing. A pull request that is
    /// open or closed unmerged is not merged.
    fn merged(&self, pr: PrNumber) -> Result<bool, PortError>;

    /// The last file of the pull request's diff, in the order the forge lists the
    /// changed files; none when the pull request changes no file.
    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// The open code scanning alerts for the pull request's head ref.
    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError>;

    /// The hand-off the pull request carries: the text of its hand-off comment, the latest when
    /// a forge somehow holds several; none when it has none.
    fn handoff(&self, pr: PrNumber) -> Result<Option<String>, PortError>;

    /// Writes the pull request's hand-off comment, replacing the one it has: after it the
    /// pull request carries exactly one, holding `text`.
    fn set_handoff(&self, pr: PrNumber, text: &str) -> Result<(), PortError>;
}

#[cfg(test)]
mod tests;
