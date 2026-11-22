//! The pieces a doctor line is built from: each word with what it means, so `zaf doctor` can paint
//! it. The plain line is the pieces joined (see [`plain`]); no word lives in two places.

use crate::domain::screen::{Line, Meaning, Text};

/// One piece of a line.
pub(super) fn piece(meaning: Meaning, text: impl Into<String>) -> (Meaning, Text) {
    (meaning, Text::from(text.into()))
}

/// Prose that means nothing in particular: the words between the meaningful ones.
pub(super) fn bold(text: impl Into<String>) -> (Meaning, Text) {
    piece(Meaning::Bold, text)
}

/// `text` in `meaning`, with each span between a pair of backticks (the backticks kept, in
/// `meaning`) a command to run, [`Meaning::Verb`]. An unpaired backtick stays as it is.
pub(super) fn with_commands(meaning: Meaning, text: &str) -> Line {
    let mut pieces = Line::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let Some(len) = rest[open + 1..].find('`') else {
            break;
        };
        let close = open + 1 + len;
        pieces.push(piece(meaning, &rest[..=open]));
        pieces.push(piece(Meaning::Verb, &rest[open + 1..close]));
        rest = &rest[close..];
    }
    if !rest.is_empty() {
        pieces.push(piece(meaning, rest));
    }
    pieces
}

/// The line as plain text: its pieces joined.
pub(crate) fn plain(line: &Line) -> String {
    line.iter().map(|(_, text)| text.as_str()).collect()
}
