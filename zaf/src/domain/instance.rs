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

impl Instance {
    /// The instance as one folder name: `team-instance-12-on-16` for `team instance 12 on #16`.
    pub fn slug(&self) -> String {
        Self::slug_of(&self.to_string())
    }

    /// The folder name for an instance written as text, which need not parse as an [`Instance`]
    /// (`local`, for a run by hand): lowercase, spaces, `#` and anything else a path
    /// would not want become `-`, runs of `-` collapse, and nothing is left at the ends.
    /// Text with nothing left is `unnamed`.
    pub fn slug_of(text: &str) -> String {
        let mut slug = String::new();
        for character in text.chars().flat_map(char::to_lowercase) {
            let keep = character.is_ascii_alphanumeric() || matches!(character, '_' | '.');
            match (keep, character) {
                (true, _) => slug.push(character),
                (false, _) if slug.ends_with('-') => {}
                (false, _) => slug.push('-'),
            }
        }
        let slug = slug.trim_matches(|character| matches!(character, '-' | '.'));
        if slug.is_empty() {
            "unnamed".to_owned()
        } else {
            slug.to_owned()
        }
    }
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
