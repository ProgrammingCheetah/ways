//! The answer: what a keeper returns, one entry per question, checked before the asker sees it.
//!
//! ```text
//! q1: settled
//!     answer: An empty list exports the header row and nothing else.
//!     pages: 1001 (decided)
//!     quote: "an empty list exports the header row only"
//! q2: not settled
//!     next: question ticket on #12 (door: no)
//! q3: conflict
//!     pages: 1001 (decided), 1002 (current)
//!     next: question ticket on #12 (door: no)
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use super::text::word_count;
use super::{PageId, PageStatus, QuestionId, Quote};

/// An answer has at most this many words.
pub const ANSWER_WORD_LIMIT: usize = 60;

/// A cited page with the index status it had when cited: `1001 (decided)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{page} ({status})")]
pub struct Citation {
    /// The page.
    pub page: PageId,
    /// Its index status; only `decided` and `current` are citable.
    pub status: PageStatus,
}

impl FromStr for Citation {
    type Err = AnswerError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid_citation_error = AnswerError::Citation(text.to_owned());
        let (page_text, status_text) = text
            .trim()
            .strip_suffix(')')
            .and_then(|inner| inner.split_once(" ("))
            .ok_or_else(|| invalid_citation_error.clone())?;
        let page = page_text
            .trim()
            .parse::<PageId>()
            .map_err(|_| invalid_citation_error.clone())?;
        let status = status_text
            .trim()
            .parse::<PageStatus>()
            .map_err(|_| invalid_citation_error)?;
        Ok(Self { page, status })
    }
}

/// What the keeper found for one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The design answers it.
    Settled {
        /// The answer, sixty words at most.
        answer: String,
        /// The pages it rests on.
        pages: Vec<Citation>,
        /// Verbatim quotes from those pages.
        quotes: Vec<Quote>,
    },
    /// The design does not answer it.
    NotSettled {
        /// What happens next, usually a question ticket.
        next: String,
    },
    /// Two live pages disagree.
    Conflict {
        /// The pages that disagree, two or more.
        pages: Vec<Citation>,
        /// What happens next, usually a question ticket.
        next: String,
    },
}

/// The verdict on one question, under the question's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsweredQuestion {
    /// The id of the question answered.
    pub id: QuestionId,
    /// What the keeper found.
    pub verdict: Verdict,
}

/// A keeper's whole answer to one ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// One entry per question, in the ask's order.
    pub questions: Vec<AnsweredQuestion>,
}

/// A rejected answer, naming what broke the format or its rules.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    /// A line other than the one the format expects.
    #[error("line {line}: expected `{expected}`, got `{text}`")]
    Expected {
        /// The 1-based line number.
        line: usize,
        /// What the format expected.
        expected: &'static str,
        /// The line as written.
        text: String,
    },
    /// Not a `q<n>` id.
    #[error("`{0}` is not a question id")]
    QuestionId(String),
    /// A state other than `settled`, `not settled` or `conflict`.
    #[error("line {line}: `{text}` is not a verdict")]
    Verdict {
        /// The 1-based line number.
        line: usize,
        /// The state as written.
        text: String,
    },
    /// A body key outside `answer`, `pages`, `quote`, `next`.
    #[error("line {line}: unknown key `{key}`")]
    UnknownKey {
        /// The 1-based line number.
        line: usize,
        /// The key as written.
        key: String,
    },
    /// A single-valued key given twice.
    #[error("{id}: `{key}` appears twice")]
    DuplicateKey {
        /// The question.
        id: QuestionId,
        /// The key.
        key: &'static str,
    },
    /// A key the verdict needs, left out.
    #[error("{id}: `{key}` is missing")]
    MissingKey {
        /// The question.
        id: QuestionId,
        /// The key.
        key: &'static str,
    },
    /// A key the verdict does not take.
    #[error("{id}: `{key}` does not belong to this verdict")]
    UnexpectedKey {
        /// The question.
        id: QuestionId,
        /// The key.
        key: &'static str,
    },
    /// An answer past sixty words.
    #[error("{id}: the answer has {words} words; the limit is {ANSWER_WORD_LIMIT}")]
    AnswerTooLong {
        /// The question.
        id: QuestionId,
        /// Its word count.
        words: usize,
    },
    /// Not `<page id> (<status>)`.
    #[error("`{0}` is not a citation")]
    Citation(String),
    /// A cited page that is neither `decided` nor `current`.
    #[error("{id}: page {page} is {status} and cannot be cited")]
    UncitablePage {
        /// The question.
        id: QuestionId,
        /// The page.
        page: PageId,
        /// Its status.
        status: PageStatus,
    },
    /// A quote not wrapped in double quotes, or empty.
    #[error("line {line}: a quote is non-empty text in double quotes, got `{text}`")]
    Quote {
        /// The 1-based line number.
        line: usize,
        /// The value as written.
        text: String,
    },
    /// A conflict citing fewer than two pages.
    #[error("{0}: a conflict cites at least two pages")]
    ConflictNeedsTwoPages(QuestionId),
    /// Two entries for one question.
    #[error("{0} is answered twice")]
    DuplicateQuestion(QuestionId),
    /// No entries at all.
    #[error("the answer is empty")]
    Empty,
}

/// A failed quote check; the whole answer is discarded and re-asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuoteCheckError {
    /// The quote is in none of the pages the question cites.
    #[error("{question}: the quote is not in its cited pages: \"{quote}\"")]
    QuoteNotFound {
        /// The question.
        question: QuestionId,
        /// The quote.
        quote: Quote,
    },
    /// A cited page could not be read.
    #[error("{question}: page {page} could not be read")]
    PageUnavailable {
        /// The question.
        question: QuestionId,
        /// The page.
        page: PageId,
    },
}

impl Answer {
    /// Checks every quote of every settled entry against the text of the pages it cites;
    /// each quote must appear verbatim, whitespace aside, in at least one of them.
    ///
    /// `page_text` reads a page's text, `None` when the page cannot be read.
    pub fn check_quotes<F>(&self, page_text: F) -> Result<(), QuoteCheckError>
    where
        F: Fn(&PageId) -> Option<String>,
    {
        for entry in &self.questions {
            let Verdict::Settled { pages, quotes, .. } = &entry.verdict else {
                continue;
            };
            let texts = pages
                .iter()
                .map(|citation| {
                    page_text(&citation.page).ok_or_else(|| QuoteCheckError::PageUnavailable {
                        question: entry.id,
                        page: citation.page.clone(),
                    })
                })
                .collect::<Result<Vec<String>, QuoteCheckError>>()?;
            let missing_quote = quotes
                .iter()
                .find(|quote| !texts.iter().any(|text| quote.is_found_in(text)));
            if let Some(quote) = missing_quote {
                let quote_not_found_error = QuoteCheckError::QuoteNotFound {
                    question: entry.id,
                    quote: quote.clone(),
                };
                return Err(quote_not_found_error);
            }
        }
        Ok(())
    }
}

const SETTLED: &str = "settled";
const NOT_SETTLED: &str = "not settled";
const CONFLICT: &str = "conflict";
const ANSWER: &str = "answer";
const PAGES: &str = "pages";
const QUOTE: &str = "quote";
const NEXT: &str = "next";
const BODY_INDENT: &str = "    ";

/// The verdict state of an entry, before its body is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Settled,
    NotSettled,
    Conflict,
}

/// One entry's lines as read, before the verdict's rules apply.
struct EntryDraft {
    id: QuestionId,
    state: State,
    answer: Option<String>,
    pages: Option<Vec<Citation>>,
    quotes: Vec<Quote>,
    next: Option<String>,
}

impl EntryDraft {
    fn finish(self) -> Result<AnsweredQuestion, AnswerError> {
        let id = self.id;
        let verdict = match self.state {
            State::Settled => {
                reject_present(id, NEXT, self.next.is_some())?;
                let answer = self
                    .answer
                    .ok_or(AnswerError::MissingKey { id, key: ANSWER })?;
                let words = word_count(&answer);
                if words > ANSWER_WORD_LIMIT {
                    return Err(AnswerError::AnswerTooLong { id, words });
                }
                let pages = self
                    .pages
                    .ok_or(AnswerError::MissingKey { id, key: PAGES })?;
                if self.quotes.is_empty() {
                    return Err(AnswerError::MissingKey { id, key: QUOTE });
                }
                Verdict::Settled {
                    answer,
                    pages,
                    quotes: self.quotes,
                }
            }
            State::NotSettled => {
                reject_present(id, ANSWER, self.answer.is_some())?;
                reject_present(id, PAGES, self.pages.is_some())?;
                reject_present(id, QUOTE, !self.quotes.is_empty())?;
                let next = self.next.ok_or(AnswerError::MissingKey { id, key: NEXT })?;
                Verdict::NotSettled { next }
            }
            State::Conflict => {
                reject_present(id, ANSWER, self.answer.is_some())?;
                reject_present(id, QUOTE, !self.quotes.is_empty())?;
                let pages = self
                    .pages
                    .ok_or(AnswerError::MissingKey { id, key: PAGES })?;
                if pages.len() < 2 {
                    return Err(AnswerError::ConflictNeedsTwoPages(id));
                }
                let next = self.next.ok_or(AnswerError::MissingKey { id, key: NEXT })?;
                Verdict::Conflict { pages, next }
            }
        };
        Ok(AnsweredQuestion { id, verdict })
    }
}

fn reject_present(id: QuestionId, key: &'static str, present: bool) -> Result<(), AnswerError> {
    if present {
        return Err(AnswerError::UnexpectedKey { id, key });
    }
    Ok(())
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    id: QuestionId,
    key: &'static str,
) -> Result<(), AnswerError> {
    if slot.replace(value).is_some() {
        return Err(AnswerError::DuplicateKey { id, key });
    }
    Ok(())
}

fn parse_header(line: usize, text: &str) -> Result<EntryDraft, AnswerError> {
    let not_a_header_error = AnswerError::Expected {
        line,
        expected: "q<n>: settled | not settled | conflict",
        text: text.to_owned(),
    };
    let (id_text, state_text) = text.split_once(':').ok_or(not_a_header_error)?;
    let id = id_text
        .parse::<QuestionId>()
        .map_err(|_| AnswerError::QuestionId(id_text.to_owned()))?;
    let state = match state_text.trim() {
        SETTLED => State::Settled,
        NOT_SETTLED => State::NotSettled,
        CONFLICT => State::Conflict,
        other => {
            let unknown_verdict_error = AnswerError::Verdict {
                line,
                text: other.to_owned(),
            };
            return Err(unknown_verdict_error);
        }
    };
    let draft = EntryDraft {
        id,
        state,
        answer: None,
        pages: None,
        quotes: Vec::new(),
        next: None,
    };
    Ok(draft)
}

fn parse_citations(id: QuestionId, text: &str) -> Result<Vec<Citation>, AnswerError> {
    let citations = text
        .split(',')
        .map(str::parse::<Citation>)
        .collect::<Result<Vec<Citation>, AnswerError>>()?;
    let uncitable = citations
        .iter()
        .find(|citation| !citation.status.is_citable());
    if let Some(citation) = uncitable {
        let uncitable_page_error = AnswerError::UncitablePage {
            id,
            page: citation.page.clone(),
            status: citation.status,
        };
        return Err(uncitable_page_error);
    }
    Ok(citations)
}

fn parse_quote(line: usize, text: &str) -> Result<Quote, AnswerError> {
    let not_a_quote_error = AnswerError::Quote {
        line,
        text: text.to_owned(),
    };
    let inner = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .filter(|inner| !inner.trim().is_empty())
        .ok_or(not_a_quote_error)?;
    Ok(Quote::from(inner))
}

fn read_body_line(draft: &mut EntryDraft, line: usize, text: &str) -> Result<(), AnswerError> {
    let id = draft.id;
    let not_a_body_line_error = AnswerError::Expected {
        line,
        expected: "    <key>: <value>",
        text: text.to_owned(),
    };
    let (key, value) = text.trim().split_once(':').ok_or(not_a_body_line_error)?;
    let value = value.trim();
    match key.trim() {
        ANSWER => set_once(&mut draft.answer, value.to_owned(), id, ANSWER),
        PAGES => set_once(&mut draft.pages, parse_citations(id, value)?, id, PAGES),
        QUOTE => {
            draft.quotes.push(parse_quote(line, value)?);
            Ok(())
        }
        NEXT => set_once(&mut draft.next, value.to_owned(), id, NEXT),
        other => {
            let unknown_key_error = AnswerError::UnknownKey {
                line,
                key: other.to_owned(),
            };
            Err(unknown_key_error)
        }
    }
}

impl FromStr for Answer {
    type Err = AnswerError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut drafts: Vec<EntryDraft> = Vec::new();
        for (index, raw_line) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw_line.trim_end();
            if content.trim().is_empty() {
                continue;
            }
            if !content.starts_with(char::is_whitespace) {
                drafts.push(parse_header(line, content)?);
                continue;
            }
            let orphan_line_error = AnswerError::Expected {
                line,
                expected: "q<n>: <verdict>",
                text: content.to_owned(),
            };
            let draft = drafts.last_mut().ok_or(orphan_line_error)?;
            read_body_line(draft, line, content)?;
        }

        let mut seen = BTreeSet::new();
        let mut questions = Vec::new();
        for draft in drafts {
            if !seen.insert(draft.id) {
                return Err(AnswerError::DuplicateQuestion(draft.id));
            }
            questions.push(draft.finish()?);
        }
        if questions.is_empty() {
            return Err(AnswerError::Empty);
        }
        Ok(Self { questions })
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for entry in &self.questions {
            match &entry.verdict {
                Verdict::Settled {
                    answer,
                    pages,
                    quotes,
                } => {
                    writeln!(formatter, "{}: {SETTLED}", entry.id)?;
                    writeln!(formatter, "{BODY_INDENT}{ANSWER}: {answer}")?;
                    writeln!(formatter, "{BODY_INDENT}{PAGES}: {}", join_citations(pages))?;
                    for quote in quotes {
                        writeln!(formatter, "{BODY_INDENT}{QUOTE}: \"{quote}\"")?;
                    }
                }
                Verdict::NotSettled { next } => {
                    writeln!(formatter, "{}: {NOT_SETTLED}", entry.id)?;
                    writeln!(formatter, "{BODY_INDENT}{NEXT}: {next}")?;
                }
                Verdict::Conflict { pages, next } => {
                    writeln!(formatter, "{}: {CONFLICT}", entry.id)?;
                    writeln!(formatter, "{BODY_INDENT}{PAGES}: {}", join_citations(pages))?;
                    writeln!(formatter, "{BODY_INDENT}{NEXT}: {next}")?;
                }
            }
        }
        Ok(())
    }
}

fn join_citations(pages: &[Citation]) -> String {
    let rendered: Vec<String> = pages.iter().map(ToString::to_string).collect();
    rendered.join(", ")
}

#[cfg(test)]
mod tests;
