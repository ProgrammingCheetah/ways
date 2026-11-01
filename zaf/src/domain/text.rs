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

#[cfg(test)]
mod tests;
