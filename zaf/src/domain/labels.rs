//! Closed vocabularies the board and the design source share with zaf.

/// The state label of a task on the board; a task carries exactly one.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum TaskLabel {
    /// Open for a claim.
    Ready,
    /// Held by an instance under a lease.
    Claimed,
    /// Waiting on another task.
    Blocked,
    /// Waiting on the Engineer.
    Question,
    /// Merged.
    Done,
}

/// The status of a design page in the source's index.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum PageStatus {
    /// A recorded decision.
    Decided,
    /// A living page that is in force.
    Current,
    /// Proposed, not yet in force.
    Proposed,
    /// Replaced by another page; never cited.
    Superseded,
}

impl PageStatus {
    /// Whether an answer may cite a page with this status: only `decided` and `current` pages.
    pub fn is_citable(self) -> bool {
        matches!(self, Self::Decided | Self::Current)
    }
}

#[cfg(test)]
mod tests;
