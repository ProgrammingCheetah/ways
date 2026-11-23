//! The role-override note of `zaf doctor` (release 0.5.5, D15, M10): a project's own
//! `.zaf/roles/*.md` that tells a model to type `zaf` in Bash.
//!
//! D15's rule in every prompt keeps such an override working, so the note is informational and
//! never fails the report. Doctor only reads: it never edits an override.

use std::fs;
use std::path::Path;

use super::paint::{bold, piece, plain};
use crate::app::spawn::ROLES_DIR;
use crate::composition::STATE_DIR;
use crate::domain::screen::{Line, Meaning};

/// The tool every `zaf` verb is reached through; a line that names it is already in tool form.
const TOOL_PREFIX: &str = "mcp__zaf__";

/// One role override that tells a model to type `zaf` in Bash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RoleOverrideNote {
    /// The override, as `.zaf/roles/<name>`.
    pub file: String,
    /// The first line of it that does (1-based).
    pub line: usize,
}

impl RoleOverrideNote {
    /// `role override <file>:<line> tells a model to type zaf in Bash; …`, one line.
    pub fn line(&self) -> String {
        plain(&self.painted())
    }

    /// [`RoleOverrideNote::line`] as pieces: the place a noun, the rest an aside.
    pub fn painted(&self) -> Line {
        vec![
            bold("role override "),
            piece(Meaning::Noun, format!("{}:{}", self.file, self.line)),
            piece(
                Meaning::Dim,
                " tells a model to type zaf in Bash; instances call zaf's MCP tools (mcp__zaf__<verb>)",
            ),
        ]
    }
}

/// The 1-based lines of `text` that tell a model to type `zaf` in Bash: a line with an inline code
/// span that starts with `zaf ` and the word `Bash`, unless it names a `mcp__zaf__` tool or says
/// `never` in any case.
pub fn bash_zaf_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            !line.contains(TOOL_PREFIX)
                && !line.to_lowercase().contains("never")
                && has_word(line, "Bash")
                && code_spans(line).any(|span| span.starts_with("zaf "))
        })
        .map(|(index, _)| index + 1)
        .collect()
}

/// The inline code spans of one line: the text between a backtick and the next.
fn code_spans(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').skip(1).step_by(2)
}

/// Whether `word` stands in `line` as a whole word.
fn has_word(line: &str, word: &str) -> bool {
    line.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|part| part == word)
}

/// One note per `<root>/.zaf/roles/*.md` with a firing line, in name order; none when the folder
/// is missing or empty. Reads only.
pub fn check(root: &Path) -> Vec<RoleOverrideNote> {
    let Ok(entries) = fs::read_dir(root.join(STATE_DIR).join(ROLES_DIR)) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let line = *bash_zaf_lines(&text).first()?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(RoleOverrideNote {
                file: format!("{STATE_DIR}/{ROLES_DIR}/{name}"),
                line,
            })
        })
        .collect()
}
