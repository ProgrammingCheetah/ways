//! Epics: one feature or concern, grouping its tasks.

use super::IssueNumber;

/// An epic on the board. It has no code of its own; its tasks do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epic {
    /// The epic's issue.
    pub number: IssueNumber,
    /// The epic's title.
    pub title: String,
    /// Whether the Engineer approved it (the `signed` label).
    pub signed: bool,
    /// Its tasks, in board order.
    pub tasks: Vec<IssueNumber>,
}
