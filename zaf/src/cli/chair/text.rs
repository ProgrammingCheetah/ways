//! Text cut to a width as a terminal shows it: a CJK character takes two cells, a combining mark
//! none. A string cut short ends with `…`; a paragraph is wrapped at spaces, a word longer than
//! the width broken where it must be.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The cells `text` takes.
pub fn width(text: &str) -> usize {
    text.width()
}

/// `text` in at most `cells` cells: whole when it fits, else cut and ended with `…`.
pub fn fit(text: &str, cells: usize) -> String {
    if text.width() <= cells {
        return text.to_owned();
    }
    if cells == 0 {
        return String::new();
    }
    let mut taken = String::new();
    let mut used = 0;
    for character in text.chars() {
        let size = character.width().unwrap_or(0);
        if used + size > cells - 1 {
            break;
        }
        taken.push(character);
        used += size;
    }
    taken.push('…');
    taken
}

/// `text` padded with spaces, or cut, to exactly `cells` cells.
pub fn pad(text: &str, cells: usize) -> String {
    let fitted = fit(text, cells);
    let gap = cells.saturating_sub(fitted.width());
    format!("{fitted}{}", " ".repeat(gap))
}

/// `text` wrapped to lines of at most `cells` cells, each of its own lines kept apart and an
/// empty one kept; tabs read as a space.
pub fn wrap(text: &str, cells: usize) -> Vec<String> {
    let cells = cells.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph
            .replace('\t', " ")
            .split(' ')
            .filter(|word| !word.is_empty())
        {
            let gap = usize::from(!line.is_empty());
            if line.width() + gap + word.width() <= cells {
                if gap == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for character in word.chars() {
                let size = character.width().unwrap_or(0);
                if line.width() + size > cells && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(character);
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests;
