//! Instances: one run of a role, numbered on its epic.

use std::str::FromStr;

use super::{IdError, IssueNumber};
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

impl FromStr for Instance {
    type Err = IdError;

    /// Reads an instance back from its display form, `<role> instance <n> on #<epic>`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let rejected = || IdError::Instance(text.to_owned());
        let (role, rest) = text.split_once(" instance ").ok_or_else(rejected)?;
        let (number, epic) = rest.split_once(" on ").ok_or_else(rejected)?;
        let number = number.parse::<u32>().map_err(|_| rejected())?;
        let epic = epic.parse::<IssueNumber>().map_err(|_| rejected())?;
        if role.is_empty() {
            return Err(rejected());
        }
        Ok(Self {
            role: RoleName::from(role),
            epic,
            number: InstanceNumber::from(number),
        })
    }
}

#[cfg(test)]
mod tests;
