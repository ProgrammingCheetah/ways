//! Instances: one run of a role, numbered on its epic.

use super::IssueNumber;
use crate::newtype::text_newtype;

text_newtype! {
    /// The name of a role (`programmer`, `team`, …), as the manifest declares it.
    RoleName
}

/// The number of an instance on its epic (`instance 3`).
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
pub struct InstanceNumber(u32);

/// One run of a role on an epic; it leaves exactly one status comment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{role} instance {number} on {epic}")]
pub struct Instance {
    /// The role it runs.
    pub role: RoleName,
    /// The epic it works on.
    pub epic: IssueNumber,
    /// Its number on that epic.
    pub number: InstanceNumber,
}
