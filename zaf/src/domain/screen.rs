//! The words a screen shows, and what each one means.
//!
//! No codes live here: the app builds these shapes and the CLI draws them, painting each
//! [`Meaning`] in its own colour only where colour is allowed (`cli::screen`).

use crate::newtype::text_newtype;

/// What a word on a screen is, so the CLI can paint it and a plain terminal can still read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Meaning {
    /// The word `zaf` naming the program.
    Program,
    /// A verb: the thing zaf is about to do or did.
    Verb,
    /// A noun: what the verb acts on (a file, a folder, a branch).
    Noun,
    /// A step that went through.
    Done,
    /// A step that failed.
    Failed,
    /// Something that waits on the Engineer.
    Waiting,
    /// A reason or aside, to read second.
    Dim,
    /// A heading or emphasis.
    Bold,
    /// Words with no meaning of their own: drawn as they are, in any terminal.
    Plain,
}

text_newtype! {
    /// A verb of a plan row: what zaf will do (`write`, `make`, `move`).
    Verb
}

text_newtype! {
    /// A line of prose on a screen: a noun, a reason, a problem or a step's outcome.
    Text
}

text_newtype! {
    /// A command the Engineer can run, as they would type it.
    Command
}

/// `text` with each control character (C0, DEL, C1 and so ESC) a space: the board's words are
/// not trusted to move the terminal's cursor, set its modes or reach its clipboard. Every piece
/// of board text a screen draws goes through here.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// One line of a screen: pieces of text, each with what it means, left to right. The pieces hold
/// every space the line shows, so the plain line is the pieces joined.
pub type Line = Vec<(Meaning, Text)>;

/// `lines` as plain text: each line's pieces joined, every line ended by `\n`. What a screen
/// without colour draws.
pub fn plain(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        for (_, text) in line {
            out.push_str(text.as_str());
        }
        out.push('\n');
    }
    out
}

/// One line of a plan: a verb, the noun it acts on and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    /// What zaf will do.
    pub verb: Verb,
    /// What it acts on.
    pub noun: Text,
    /// Why, in a few words.
    pub why: Text,
}

/// What the Engineer has to do themself: the problem, the command that fixes it when there is
/// one, and why it matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What is wrong.
    pub problem: Text,
    /// The command to run, when one fixes it.
    pub command: Option<Command>,
    /// Why it matters.
    pub why: Text,
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepEnd {
    /// It went through, with a word on what it left.
    Done(Text),
    /// It failed, with the reason.
    Failed(Text),
}
