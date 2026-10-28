//! The quote check: an answer's quote must appear verbatim in the page it cites.

use super::text::normalize_whitespace;

/// A quote an answer gives from a cited page.
///
/// Verbatim means the same words in the same order with the same punctuation
/// and case; only whitespace may differ (line breaks, indentation, runs of spaces).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, derive_more::Display, derive_more::From, derive_more::AsRef,
)]
#[as_ref(str)]
pub struct Quote(String);

impl From<&str> for Quote {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl Quote {
    /// Whether the quote appears verbatim in `page_text`, whitespace aside,
    /// starting and ending on word boundaries. An empty quote is never found.
    pub fn is_found_in(&self, page_text: &str) -> bool {
        let quote = normalize_whitespace(&self.0);
        if quote.is_empty() {
            return false;
        }
        let page = normalize_whitespace(page_text);
        page.match_indices(quote.as_str()).any(|(start, matched)| {
            let end = start + matched.len();
            let starts_on_boundary = page[..start]
                .chars()
                .next_back()
                .is_none_or(|before| !before.is_alphanumeric());
            let ends_on_boundary = page[end..]
                .chars()
                .next()
                .is_none_or(|after| !after.is_alphanumeric());
            starts_on_boundary && ends_on_boundary
        })
    }
}

#[cfg(test)]
mod tests;
