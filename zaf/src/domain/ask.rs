//! The ask: one record per `zaf ask` call, up to five questions to a keeper.
//!
//! ```text
//! from: team#3 / programmer otter     # who, for the log
//! for: #12                            # scopes the search
//! context: Members export their list; implementing the CSV writer
//! questions:
//!   q1: rule  | Is an empty list exported as a header row only?
//!   q2: term  | What is a reading list?
//! ```
//!
//! A trailing `#` after two or more spaces is an annotation and is dropped.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use super::IssueNumber;
use super::text::{strip_comment, word_count};
use crate::newtype::text_newtype;

text_newtype! {
    /// Who asks, for the log (`team#3 / programmer otter`).
    Asker
}

/// The most questions one ask carries.
pub const MAX_QUESTIONS: usize = 5;
/// The most lines of context one ask carries.
pub const MAX_CONTEXT_LINES: usize = 2;
/// A question has fewer words than this.
pub const QUESTION_WORD_LIMIT: usize = 30;

/// What a question asks for; it steers where the keeper looks.
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
pub enum AskKind {
    /// What the design says.
    Fact,
    /// What is allowed, required or forbidden.
    Rule,
    /// An entity or a word.
    Term,
    /// Whether there is a decision.
    Exists,
    /// Which page.
    Where,
}

/// A question's id inside one ask: `q1` to `q5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("q{_0}")]
pub struct QuestionId(u8);

impl From<u8> for QuestionId {
    fn from(number: u8) -> Self {
        Self(number)
    }
}

impl FromStr for QuestionId {
    type Err = AskError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid_id_error = AskError::QuestionId(text.to_owned());
        let digits = text
            .trim()
            .strip_prefix('q')
            .ok_or_else(|| invalid_id_error.clone())?;
        let all_digits = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
        let number = digits
            .parse::<u8>()
            .ok()
            .filter(|number| all_digits && *number >= 1)
            .ok_or(invalid_id_error)?;
        Ok(Self(number))
    }
}

/// One question of an ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// Its id, which the answer echoes.
    pub id: QuestionId,
    /// What it asks for.
    pub kind: AskKind,
    /// The question: one sentence, under thirty words.
    pub text: String,
}

/// One ask: who, for which task, a line or two of context and the questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// Who asks.
    pub from: Asker,
    /// The task whose named pages are searched first.
    pub for_task: IssueNumber,
    /// One or two lines of context.
    pub context: Vec<String>,
    /// One to five questions with distinct ids.
    pub questions: Vec<Question>,
}

/// A rejected ask, naming what broke the format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AskError {
    /// A line other than the one the format expects next.
    #[error("line {line}: expected `{expected}`, got `{text}`")]
    Expected {
        /// The 1-based line number.
        line: usize,
        /// What the format expected.
        expected: &'static str,
        /// The line as written.
        text: String,
    },
    /// The text ended before the format did.
    #[error("the ask ends before `{0}`")]
    Truncated(&'static str),
    /// `from:` is empty.
    #[error("`from:` is empty")]
    EmptyAsker,
    /// `for:` is not an issue number.
    #[error("`for:` is not an issue number: `{0}`")]
    Task(String),
    /// More context lines than the format allows.
    #[error("context runs past {MAX_CONTEXT_LINES} lines")]
    ContextTooLong,
    /// Not a `q<n>` id.
    #[error("`{0}` is not a question id")]
    QuestionId(String),
    /// A question kind outside the vocabulary.
    #[error("`{0}` is not a question kind")]
    Kind(String),
    /// A question line without `| text`.
    #[error("{0} has no question text")]
    EmptyQuestion(QuestionId),
    /// A question of thirty words or more.
    #[error("{id} has {words} words; a question is under {QUESTION_WORD_LIMIT}")]
    QuestionTooLong {
        /// The question.
        id: QuestionId,
        /// Its word count.
        words: usize,
    },
    /// Two questions with one id.
    #[error("{0} appears twice")]
    DuplicateQuestion(QuestionId),
    /// No questions, or more than five.
    #[error("an ask carries 1 to {MAX_QUESTIONS} questions, this one {0}")]
    QuestionCount(usize),
}

const FROM: &str = "from:";
const FOR: &str = "for:";
const CONTEXT: &str = "context:";
const QUESTIONS: &str = "questions:";
/// The indent that aligns a second context line under the first.
const CONTEXT_INDENT: &str = "         ";
/// The width a kind is padded to before `|`, the width of the longest kind.
const KIND_WIDTH: usize = 6;

impl FromStr for Ask {
    type Err = AskError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut lines = text
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, strip_comment(line)))
            .filter(|(_, line)| !line.trim().is_empty())
            .peekable();

        let (line, from_line) = lines.next().ok_or(AskError::Truncated(FROM))?;
        let from = expect_field(line, from_line, FROM)?;
        if from.is_empty() {
            return Err(AskError::EmptyAsker);
        }

        let (line, for_line) = lines.next().ok_or(AskError::Truncated(FOR))?;
        let for_value = expect_field(line, for_line, FOR)?;
        let for_task = for_value
            .parse::<IssueNumber>()
            .map_err(|_| AskError::Task(for_value.to_owned()))?;

        let (line, context_line) = lines.next().ok_or(AskError::Truncated(CONTEXT))?;
        let mut context = vec![expect_field(line, context_line, CONTEXT)?.to_owned()];
        while let Some((_, continuation)) =
            lines.next_if(|(_, next)| next.starts_with(char::is_whitespace))
        {
            context.push(continuation.trim().to_owned());
        }
        if context.len() > MAX_CONTEXT_LINES {
            return Err(AskError::ContextTooLong);
        }

        let (line, questions_line) = lines.next().ok_or(AskError::Truncated(QUESTIONS))?;
        let questions_rest = expect_field(line, questions_line, QUESTIONS)?;
        if !questions_rest.is_empty() {
            let inline_questions_error = AskError::Expected {
                line,
                expected: QUESTIONS,
                text: questions_line.to_owned(),
            };
            return Err(inline_questions_error);
        }

        let mut questions = Vec::new();
        let mut seen = BTreeSet::new();
        for (line, question_line) in lines {
            let question = parse_question(line, question_line)?;
            if !seen.insert(question.id) {
                return Err(AskError::DuplicateQuestion(question.id));
            }
            questions.push(question);
        }
        if questions.is_empty() || questions.len() > MAX_QUESTIONS {
            return Err(AskError::QuestionCount(questions.len()));
        }

        let ask = Self {
            from: Asker::from(from),
            for_task,
            context,
            questions,
        };
        Ok(ask)
    }
}

impl fmt::Display for Ask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{FROM} {}", self.from)?;
        writeln!(formatter, "{FOR} {}", self.for_task)?;
        let mut context_lines = self.context.iter();
        if let Some(first) = context_lines.next() {
            writeln!(formatter, "{CONTEXT} {first}")?;
        }
        for continuation in context_lines {
            writeln!(formatter, "{CONTEXT_INDENT}{continuation}")?;
        }
        writeln!(formatter, "{QUESTIONS}")?;
        for question in &self.questions {
            let kind: &'static str = question.kind.into();
            writeln!(
                formatter,
                "  {}: {kind:<KIND_WIDTH$}| {}",
                question.id, question.text
            )?;
        }
        Ok(())
    }
}

fn expect_field<'line>(
    line: usize,
    text: &'line str,
    label: &'static str,
) -> Result<&'line str, AskError> {
    let unexpected_line_error = AskError::Expected {
        line,
        expected: label,
        text: text.to_owned(),
    };
    text.strip_prefix(label)
        .map(str::trim)
        .ok_or(unexpected_line_error)
}

fn parse_question(line: usize, text: &str) -> Result<Question, AskError> {
    let not_a_question_error = AskError::Expected {
        line,
        expected: "  q<n>: <kind> | <question>",
        text: text.to_owned(),
    };
    if !text.starts_with(char::is_whitespace) {
        return Err(not_a_question_error);
    }
    let (id_text, rest) = text.trim().split_once(':').ok_or(not_a_question_error)?;
    let id = id_text.parse::<QuestionId>()?;
    let (kind_text, question_text) = rest.split_once('|').ok_or(AskError::EmptyQuestion(id))?;
    let kind = kind_text
        .trim()
        .parse::<AskKind>()
        .map_err(|_| AskError::Kind(kind_text.trim().to_owned()))?;
    let question_text = question_text.trim();
    if question_text.is_empty() {
        return Err(AskError::EmptyQuestion(id));
    }
    let words = word_count(question_text);
    if words >= QUESTION_WORD_LIMIT {
        return Err(AskError::QuestionTooLong { id, words });
    }
    let question = Question {
        id,
        kind,
        text: question_text.to_owned(),
    };
    Ok(question)
}

#[cfg(test)]
mod tests;
