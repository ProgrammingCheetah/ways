//! Identifiers that cross every layer: board issue numbers and design page ids.

use std::str::FromStr;

/// A rejected identifier, naming the text that failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// The text is not an issue number such as `#12` or `12`.
    #[error("`{0}` is not an issue number")]
    IssueNumber(String),
    /// The text is not a page id: empty, or holding whitespace, a comma or a parenthesis.
    #[error("`{0}` is not a page id")]
    PageId(String),
}

/// The number of an issue on the board: an epic, a task or a question.
///
/// Parses from `#12` or `12` and displays as `#12`.
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
pub struct IssueNumber(u64);

impl IssueNumber {
    /// The bare number, for adapters that address issues numerically.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl FromStr for IssueNumber {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        let all_digits = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
        if !all_digits {
            return Err(IdError::IssueNumber(text.to_owned()));
        }
        digits
            .parse::<u64>()
            .map(Self)
            .map_err(|_| IdError::IssueNumber(text.to_owned()))
    }
}

/// The id of a page in the project's design source.
///
/// Opaque to zaf: whatever the design adapter uses, as long as it is one token
/// (no whitespace, comma or parenthesis, which the answer format uses as separators).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display, derive_more::AsRef,
)]
#[as_ref(str)]
pub struct PageId(String);

impl FromStr for PageId {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let is_separator =
            |character: char| character.is_whitespace() || matches!(character, ',' | '(' | ')');
        if text.is_empty() || text.contains(is_separator) {
            return Err(IdError::PageId(text.to_owned()));
        }
        Ok(Self(text.to_owned()))
    }
}

#[cfg(test)]
mod tests;
