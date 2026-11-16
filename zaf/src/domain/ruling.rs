//! The check on a ruling typed at a command line: does it look as if a shell ate part of it?
//!
//! Inside double quotes a shell runs text in backticks and expands text after `$`, and what is
//! left is the ruling with holes in it: ``except the name. `judge` makes`` arrives as
//! `except the name.  makes`. The sign is a gap: on one line, two or more spaces in a row with a
//! character that is not a space before and after them. Leading indentation and trailing spaces
//! are not gaps.
//!
//! Only `zaf board ruling` asks this; text typed into the hub never passed a shell.

/// How many characters of the line an excerpt shows on each side of the gap.
const EXCERPT_SIDE: usize = 16;

/// Where a typed ruling has a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulingGap {
    /// The 1-based line of the ruling the gap is on.
    pub line: usize,
    /// The 1-based column, in characters, of the gap's first space.
    pub column: usize,
    /// The gap with the text around it; `…` marks a side that was cut.
    pub excerpt: String,
    /// Whether the ruling has more than one line, so the line is worth naming.
    pub of_many_lines: bool,
}

/// Why a typed ruling is not sent to the board.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypedRulingError {
    /// The ruling has no words.
    #[error("the answer is empty; nothing was written")]
    Empty,
    /// The ruling has a gap, as text a shell ate leaves one.
    #[error(
        "the answer has a gap at {}column {} (\"{}\"): a shell eats text in backticks and after $ inside double quotes; nothing was written",
        line_words(.0), .0.column, .0.excerpt
    )]
    Gap(RulingGap),
}

impl TypedRulingError {
    /// How to go on after this refusal, when there is more to say than the refusal itself.
    pub fn way_on(&self) -> Option<&'static str> {
        match self {
            Self::Empty => None,
            Self::Gap(_) => Some(
                "quote the answer in single quotes, or pass --as-written to send it exactly as it is",
            ),
        }
    }
}

/// `line <n>, ` for a gap in a ruling of several lines, nothing for a ruling of one.
fn line_words(gap: &RulingGap) -> String {
    if gap.of_many_lines {
        format!("line {}, ", gap.line)
    } else {
        String::new()
    }
}

/// Checks a ruling as it came from a command line: one with no words is refused, and so is one
/// with a gap, unless it is sent `as_written`.
pub fn check_typed_ruling(text: &str, as_written: bool) -> Result<(), TypedRulingError> {
    if text.trim().is_empty() {
        return Err(TypedRulingError::Empty);
    }
    match first_gap(text) {
        Some(gap) if !as_written => Err(TypedRulingError::Gap(gap)),
        _ => Ok(()),
    }
}

/// The first gap in `text`, reading line by line; none when no line has one.
pub fn first_gap(text: &str) -> Option<RulingGap> {
    let of_many_lines = text.trim_end_matches(['\n', '\r']).contains('\n');
    text.lines().enumerate().find_map(|(index, line)| {
        let characters: Vec<char> = line.chars().collect();
        let (start, end) = gap_in(&characters)?;
        Some(RulingGap {
            line: index + 1,
            column: start + 1,
            excerpt: excerpt(&characters, start, end),
            of_many_lines,
        })
    })
}

/// The first run of two or more spaces in a line that has a character that is not whitespace
/// before it and after it, as the positions of its first space and of the character after its
/// last.
fn gap_in(line: &[char]) -> Option<(usize, usize)> {
    let first_word = line.iter().position(|c| !c.is_whitespace())?;
    let last_word = line.iter().rposition(|c| !c.is_whitespace())?;
    let mut at = first_word;
    while at < last_word {
        if line[at] != ' ' {
            at += 1;
            continue;
        }
        let end = (at..last_word)
            .find(|&next| line[next] != ' ')
            .unwrap_or(last_word);
        if end - at >= 2 {
            return Some((at, end));
        }
        at = end.max(at + 1);
    }
    None
}

/// The gap from `start` to `end` with up to [`EXCERPT_SIDE`] characters on each side.
fn excerpt(line: &[char], start: usize, end: usize) -> String {
    let from = start.saturating_sub(EXCERPT_SIDE);
    let to = (end + EXCERPT_SIDE).min(line.len());
    let mut shown = String::new();
    if from > 0 {
        shown.push('…');
    }
    shown.extend(&line[from..to]);
    if to < line.len() {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests;
