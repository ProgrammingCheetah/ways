//! `[requires]` entries: a version requirement or a pinned release tag.

use std::fmt;
use std::str::FromStr;

/// A rejected `[requires]` value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{value}` is neither a version requirement (`*`, `>=2.1`, `2.0.0`) nor a release tag (`<name>-v<version>`)"
)]
pub struct RequirementError {
    /// The value as written.
    pub value: String,
}

/// What a project requires of one tool.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Requirement {
    /// A semver requirement: `*`, `>=2.1`, `2.0.0` (caret, as Cargo reads it).
    Version(semver::VersionReq),
    /// A release tag pinned exactly: `toolkit-v2.0.0`.
    Tag {
        /// The tag's name part.
        name: String,
        /// The exact version.
        version: semver::Version,
    },
}

impl Requirement {
    /// Whether an installed version satisfies the requirement.
    pub fn is_met_by(&self, installed: &semver::Version) -> bool {
        match self {
            Self::Version(requirement) => requirement.matches(installed),
            Self::Tag { version, .. } => version == installed,
        }
    }
}

impl FromStr for Requirement {
    type Err = RequirementError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        if let Ok(requirement) = semver::VersionReq::parse(trimmed) {
            return Ok(Self::Version(requirement));
        }
        let not_a_requirement_error = RequirementError {
            value: text.to_owned(),
        };
        let (name, version_text) = trimmed
            .rsplit_once("-v")
            .filter(|(name, _)| !name.is_empty())
            .ok_or_else(|| not_a_requirement_error.clone())?;
        let version = semver::Version::parse(version_text).map_err(|_| not_a_requirement_error)?;
        let tag = Self::Tag {
            name: name.to_owned(),
            version,
        };
        Ok(tag)
    }
}

impl TryFrom<String> for Requirement {
    type Error = RequirementError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(requirement) => write!(formatter, "{requirement}"),
            Self::Tag { name, version } => write!(formatter, "{name}-v{version}"),
        }
    }
}

#[cfg(test)]
mod tests;
