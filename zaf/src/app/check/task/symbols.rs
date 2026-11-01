//! The symbol search: does a file or a folder of files mention a name as a whole word?

use std::path::Path;

/// Folders a search never enters: build output and version-control state.
const SKIPPED_DIRS: &[&str] = &["target", ".git", ".jj", ".zaf", "node_modules"];

/// Whether any text file at or under `path` holds `symbol` as a whole word.
pub(super) fn defines(path: &Path, symbol: &str) -> bool {
    if symbol.is_empty() {
        return false;
    }
    if path.is_file() {
        return std::fs::read_to_string(path).is_ok_and(|text| holds_word(&text, symbol));
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        // A symlink is never followed, so a link loop cannot hang the search.
        let Ok(file_type) = entry.file_type() else {
            return false;
        };
        let skipped = file_type.is_symlink()
            || (file_type.is_dir()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIPPED_DIRS.contains(&name)));
        !skipped && defines(&entry.path(), symbol)
    })
}

/// Whether `word` occurs in `text` with no identifier character on either side.
pub(super) fn holds_word(text: &str, word: &str) -> bool {
    let is_word_char = |character: char| character.is_alphanumeric() || character == '_';
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}
