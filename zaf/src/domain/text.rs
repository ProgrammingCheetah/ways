//! Small text rules shared by the line formats.

/// Counts whitespace-separated words.
pub(crate) fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Joins the words of `text` with single spaces, dropping leading and trailing whitespace.
pub(crate) fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drops a trailing annotation: a `#` preceded by at least two whitespace characters.
///
/// One space before `#` is content (`for: #12`); two or more start a comment
/// (`for: #12    # scopes the search`).
pub(crate) fn strip_comment(line: &str) -> &str {
    let characters: Vec<(usize, char)> = line.char_indices().collect();
    let comment_start =
        characters
            .iter()
            .enumerate()
            .find_map(|(position, &(offset, character))| {
                let preceded_by_two_spaces = position >= 2
                    && characters[position - 1].1.is_whitespace()
                    && characters[position - 2].1.is_whitespace();
                (character == '#' && preceded_by_two_spaces).then_some(offset)
            });
    let content = comment_start.map_or(line, |offset| &line[..offset]);
    content.trim_end()
}

/// `text` with every control character dropped but the newline and the tab: what a screen may be
/// handed from a board, a file or a process, so none of it moves the cursor, clears the screen
/// or changes a terminal's state. C0 (`\r` included), DEL and C1 all go, and so does every
/// character that draws nothing yet is text all the same ([`is_hidden_format`]): what is ruled
/// or signed is then what was shown.
pub fn printable(text: &str) -> String {
    text.chars()
        .filter(|character| {
            (!character.is_control() || matches!(character, '\n' | '\t'))
                && !is_hidden_format(*character)
        })
        .collect()
}

/// Whether `character` is one that draws nothing and can carry or change text all the same: every
/// format character (Unicode's category Cf, as of Unicode 16.0: the bidi controls, the
/// zero-width ones, the soft hyphen, the tag characters that can spell a sentence no one sees)
/// and the variation selectors (U+FE00 to U+FE0F, U+E0100 to U+E01EF).
pub(crate) fn is_hidden_format(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

#[cfg(test)]
mod tests;
