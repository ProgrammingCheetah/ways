//! What the profiles' `PostToolUse` hook reports (docs/zaf-actions.md §5): one general word for
//! a tool call and who made it. The hook reads a Claude Code payload and leaves a report; the
//! command a Bash call ran is read to choose the word and is never kept.

use std::path::{Path, PathBuf};

use crate::domain::IssueNumber;

/// The worker of a call made inside a slice workspace.
pub const PROGRAMMER: &str = "programmer";
/// The worker of a call made anywhere else.
pub const TEAM: &str = "team";

/// The word of a Read, Grep or Glob call.
pub const READING: &str = "reading";
/// The word of an Edit or Write call.
pub const WRITING: &str = "writing";
/// The word of a test, a gate or a task check.
pub const TESTING: &str = "testing";
/// The word of a pull request check.
pub const WAITING_ON_REVIEW: &str = "waiting on review";
/// The word of any other call.
pub const WORKING: &str = "working";

/// The few fields of a `PostToolUse` payload the hook reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCall {
    /// `tool_name`.
    pub tool: String,
    /// `tool_input.command`, for a Bash call.
    pub command: Option<String>,
    /// `cwd`: where the session was.
    pub cwd: Option<PathBuf>,
}

impl ToolCall {
    /// Reads a payload. Anything that is not a JSON object with those fields gives an empty call,
    /// which reports `working`: the hook never refuses what it is given.
    pub fn parse(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            return Self::default();
        };
        let text = |value: Option<&serde_json::Value>| {
            value.and_then(serde_json::Value::as_str).map(str::to_owned)
        };
        Self {
            tool: text(value.get("tool_name")).unwrap_or_default(),
            command: text(value.pointer("/tool_input/command")),
            cwd: text(value.get("cwd"))
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from),
        }
    }

    /// The general word for the call.
    pub fn word(&self) -> &'static str {
        match self.tool.as_str() {
            "Read" | "Grep" | "Glob" => READING,
            "Edit" | "Write" => WRITING,
            "Bash" => self.command.as_deref().map_or(WORKING, bash_word),
            _ => WORKING,
        }
    }
}

/// The word for a Bash command: a pull request check waits on review; a test, a gate or a task
/// check is testing; anything else is working.
fn bash_word(command: &str) -> &'static str {
    let words = words_of(command);
    let checks: Vec<&str> = words
        .windows(3)
        .filter(|window| window[0] == "zaf" && window[1] == "check")
        .map(|window| window[2])
        .collect();
    if checks
        .iter()
        .any(|target| *target == "pr" || target.starts_with("pr:"))
    {
        return WAITING_ON_REVIEW;
    }
    let is_task_check = checks
        .iter()
        .any(|target| is_number(target.trim_start_matches('#')));
    let is_verify = words
        .windows(2)
        .any(|window| window[0] == "zaf" && window[1] == "verify");
    let is_test = words.iter().any(|word| TEST_WORDS.contains(word));
    if is_task_check || is_verify || is_test {
        TESTING
    } else {
        WORKING
    }
}

/// The words that make a command a test or a gate.
const TEST_WORDS: [&str; 10] = [
    "test", "tests", "nextest", "pytest", "jest", "vitest", "rspec", "clippy", "gate", "tox",
];

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// The command's words outside quotes, with the punctuation around each one cut off, so a
/// commit message that says `test` does not make a commit a test.
fn words_of(command: &str) -> Vec<&str> {
    let mut outside = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, c) in command.char_indices() {
        match quote {
            Some(open) if c == open => {
                quote = None;
                start = at + c.len_utf8();
            }
            Some(_) => {}
            None if c == '"' || c == '\'' => {
                outside.push(&command[start..at]);
                quote = Some(c);
            }
            None => {}
        }
    }
    if quote.is_none() {
        outside.push(&command[start..]);
    }
    outside
        .into_iter()
        .flat_map(|part| part.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|')))
        .map(|word| word.trim_matches(|c: char| !(c.is_alphanumeric() || c == ':' || c == '#')))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The task of the slice workspace `cwd` is in, when it is in one: `cwd` under `workspaces`, its
/// first components naming a slice by the `task_pattern` (`feature/{version}-{epic}-{task}`).
/// A slice name holds `/`, so each prefix of the path under `workspaces` is tried, shortest first.
pub fn task_in_workspace(cwd: &Path, workspaces: &Path, task_pattern: &str) -> Option<IssueNumber> {
    let relative = cwd.strip_prefix(workspaces).ok()?;
    let parts: Vec<String> = relative
        .components()
        .filter_map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect();
    let pieces = pieces_of(task_pattern);
    (1..=parts.len())
        .find_map(|count| walk(&pieces, &parts[..count].join("/"), None))
        .map(IssueNumber::from)
}

/// A branch pattern as literals and placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece<'a> {
    Literal(&'a str),
    Version,
    Epic,
    Task,
}

fn pieces_of(pattern: &str) -> Vec<Piece<'_>> {
    const MARKS: [(&str, Piece<'static>); 3] = [
        ("{version}", Piece::Version),
        ("{epic}", Piece::Epic),
        ("{task}", Piece::Task),
    ];
    let mut pieces = Vec::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let next = MARKS
            .iter()
            .filter_map(|(mark, piece)| rest.find(mark).map(|at| (at, *mark, *piece)))
            .min_by_key(|(at, _, _)| *at);
        match next {
            Some((at, mark, piece)) => {
                if at > 0 {
                    pieces.push(Piece::Literal(&rest[..at]));
                }
                pieces.push(piece);
                rest = &rest[at + mark.len()..];
            }
            None => {
                pieces.push(Piece::Literal(rest));
                break;
            }
        }
    }
    pieces
}

/// Matches `text` whole against `pieces`, backtracking over how much each placeholder takes; the
/// task number is the one `{task}` took.
fn walk(pieces: &[Piece], text: &str, task: Option<u64>) -> Option<u64> {
    let Some((first, rest)) = pieces.split_first() else {
        return if text.is_empty() { task } else { None };
    };
    match first {
        Piece::Literal(literal) => text
            .strip_prefix(literal)
            .and_then(|after| walk(rest, after, task)),
        Piece::Epic | Piece::Task => {
            let digits = text.bytes().take_while(u8::is_ascii_digit).count();
            (1..=digits).rev().find_map(|count| {
                let value = text[..count].parse::<u64>().ok()?;
                let task = if *first == Piece::Task {
                    Some(value)
                } else {
                    task
                };
                walk(rest, &text[count..], task)
            })
        }
        Piece::Version => (1..=text.len())
            .filter(|count| text.is_char_boundary(*count))
            .take_while(|count| !text[..*count].contains('/'))
            .find_map(|count| walk(rest, &text[count..], task)),
    }
}
