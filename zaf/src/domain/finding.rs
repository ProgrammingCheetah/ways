//! Findings: what a check step reports, tagged by the source that found it.

use crate::newtype::text_newtype;

text_newtype! {
    /// The check step that produced a finding (`ci`, `bot:<name>`, `runner:<name>:<role>`, …),
    /// kept so findings can be counted by source.
    FindingSource
}

/// How a finding weighs on a merge.
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
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum Severity {
    /// Blocks the merge until fixed.
    MustFix,
    /// Worth reading; does not block.
    Advisory,
}

/// Where in the tree a finding points.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    /// The file, relative to the repository root.
    pub path: String,
    /// The line, when the source gave one.
    pub line: Option<u32>,
}

/// One deduplicated finding from a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The step that found it.
    pub source: FindingSource,
    /// Whether it blocks.
    pub severity: Severity,
    /// What is wrong, in the source's words.
    pub message: String,
    /// Where, when the source said.
    pub location: Option<Location>,
}
