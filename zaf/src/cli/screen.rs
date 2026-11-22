//! The screen: a meaning turned into its terminal code, only where colour is allowed.
//!
//! Colour is on when stdout is a terminal, `NO_COLOR` is unset or empty (no-color.org: "when
//! present and not an empty string") and `TERM` is not `dumb`. Redraw needs a terminal and `TERM`
//! not `dumb`. Everything else gets the plain text, with no escape byte.

use std::io::{BufRead, IsTerminal, Write};
use std::os::fd::RawFd;
use std::process::{Command, Stdio};

use crate::adapters::process;
use crate::domain::screen::{Line, Meaning};

/// The rows and columns taken when the terminal's size cannot be read.
const DEFAULT_SIZE: (u16, u16) = (24, 80);

/// The program's name as a screen shows it.
const PROGRAM: &str = "zaf";

/// What decides colour and redraw, read once: tests pass these in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment<'a> {
    /// Whether the stream drawn on is a terminal: stdout for [`Screen::detect`], stderr for
    /// [`Screen::detect_stderr`].
    pub stdout_is_terminal: bool,
    /// `NO_COLOR`, when set.
    pub no_color: Option<&'a str>,
    /// `TERM`, when set.
    pub term: Option<&'a str>,
}

/// How a screen draws: in colour or not, redrawing in place or not, and the window's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    /// Whether words are painted.
    pub colour: bool,
    /// Whether a line may be redrawn in place.
    pub live: bool,
    /// The window's rows.
    pub rows: u16,
    /// The window's columns.
    pub columns: u16,
}

impl Screen {
    /// The screen of this process: its stdout, `NO_COLOR` and `TERM`, and the window's size
    /// when stdout is a terminal.
    pub fn detect() -> Self {
        let no_color = std::env::var("NO_COLOR").ok();
        let term = std::env::var("TERM").ok();
        let stdout_is_terminal = std::io::stdout().is_terminal();
        let mut screen = Self::from_environment(Environment {
            stdout_is_terminal,
            no_color: no_color.as_deref(),
            term: term.as_deref(),
        });
        if stdout_is_terminal && let Some((rows, columns)) = window_size() {
            screen.rows = rows;
            screen.columns = columns;
        }
        screen
    }

    /// The screen of this process's stderr: its own terminal check, the same `NO_COLOR` and `TERM`
    /// rules as [`Screen::detect`], and the default size. Stdout being a pipe or a terminal does
    /// not change it.
    pub fn detect_stderr() -> Self {
        let no_color = std::env::var("NO_COLOR").ok();
        let term = std::env::var("TERM").ok();
        Self::from_environment(Environment {
            stdout_is_terminal: std::io::stderr().is_terminal(),
            no_color: no_color.as_deref(),
            term: term.as_deref(),
        })
    }

    /// The screen the three inputs give, at the default size. Pure.
    pub fn from_environment(environment: Environment<'_>) -> Self {
        let dumb = environment.term == Some("dumb");
        let colour_allowed = environment.no_color.is_none_or(str::is_empty);
        Self {
            colour: environment.stdout_is_terminal && colour_allowed && !dumb,
            live: environment.stdout_is_terminal && !dumb,
            rows: DEFAULT_SIZE.0,
            columns: DEFAULT_SIZE.1,
        }
    }

    /// `text` in the colour of `meaning`, closed with a reset; unchanged when colour is off.
    pub fn paint(&self, meaning: Meaning, text: &str) -> String {
        if !self.colour || meaning == Meaning::Plain {
            return text.to_owned();
        }
        format!("\x1b[{}m{text}\x1b[0m", code(meaning))
    }

    /// `lines` as one string: each line's pieces left to right, each painted by [`Screen::paint`],
    /// every line ended by `\n`. With colour off it is the pieces' text and holds no escape byte.
    pub fn draw(&self, lines: &[Line]) -> String {
        let mut out = String::new();
        for line in lines {
            for (meaning, text) in line {
                out.push_str(&self.paint(*meaning, text.as_ref()));
            }
            out.push('\n');
        }
        out
    }

    /// An error or warning `line` whose leading `zaf` is painted as [`Meaning::Program`]; the
    /// rest of the line keeps its text. A line that does not start with the program's name, or a
    /// screen with no colour, gives the line as it is.
    pub fn error_line(&self, line: &str) -> String {
        match line.strip_prefix(PROGRAM) {
            Some(rest) if names_program(line, 0, PROGRAM.len()) => {
                format!("{}{rest}", self.program(PROGRAM))
            }
            _ => line.to_owned(),
        }
    }

    /// `text` with each whole word `zaf` naming the program painted as [`Meaning::Program`].
    /// `zaf` inside a path (`~/.zaf`, `.zaf.toml`, `zaf/src`) or a longer word (`zafx`) is not
    /// the program and stays plain.
    pub fn program(&self, text: &str) -> String {
        if !self.colour {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = 0;
        for (at, _) in text.match_indices(PROGRAM) {
            let end = at + PROGRAM.len();
            if at < rest || !names_program(text, at, end) {
                continue;
            }
            out.push_str(&text[rest..at]);
            out.push_str(&self.paint(Meaning::Program, PROGRAM));
            rest = end;
        }
        out.push_str(&text[rest..]);
        out
    }
}

/// Writes `line` to stderr, its leading `zaf` painted when stderr is a colour terminal, and a
/// newline. [`eprintln_zaf!`] is the call sites' way in.
pub fn eprint_error_line(line: &str) {
    eprintln!("{}", Screen::detect_stderr().error_line(line));
}

/// `eprintln!` for an error or warning line that starts with `zaf`: the same arguments, the
/// program's name painted on a colour terminal and the line otherwise byte for byte.
#[macro_export]
macro_rules! eprintln_zaf {
    ($($arg:tt)*) => {
        $crate::cli::screen::eprint_error_line(&format!($($arg)*))
    };
}

/// The SGR parameters of a meaning, from the mocks: the chair builds its styles from them too.
pub(crate) fn code(meaning: Meaning) -> &'static str {
    match meaning {
        Meaning::Program => "1;38;5;201",
        Meaning::Verb => "1;36",
        Meaning::Noun => "33",
        Meaning::Done => "32",
        Meaning::Failed => "31",
        Meaning::Waiting => "1;38;5;141",
        Meaning::Dim => "2",
        Meaning::Bold => "1",
        Meaning::Plain => "0",
    }
}

/// `text` without any SGR sequence (`ESC [ <digits and ;> m`): what a coloured draw looks like
/// plain. Any other escape, and an SGR sequence cut short, stays as it is. Only the tests that run
/// a screen twice, in colour and plain, use it.
#[cfg(test)]
pub(crate) fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let params = after
            .bytes()
            .take_while(|b| b.is_ascii_digit() || matches!(b, b';' | b':'))
            .count();
        if after.as_bytes().get(params) == Some(&b'm') {
            rest = &after[params + 1..];
        } else {
            out.push_str("\x1b[");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Whether the `zaf` at `text[start..end]` is the program's name: no word or path character
/// touches it. A `.` after it ends a sentence unless a name follows (`zaf.toml`).
fn names_program(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    if before
        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | '\\'))
    {
        return false;
    }
    let mut after = text[end..].chars();
    match after.next() {
        None => true,
        Some(c) if c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '\\') => false,
        Some('.') => !after.next().is_some_and(char::is_alphanumeric),
        Some(_) => true,
    }
}

/// The window's `(rows, columns)` from `stty size`, when it answers.
pub(super) fn window_size() -> Option<(u16, u16)> {
    let output = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut numbers = text.split_whitespace();
    let rows = numbers.next()?.parse().ok().filter(|n| *n > 0)?;
    let columns = numbers.next()?.parse().ok().filter(|n| *n > 0)?;
    Some((rows, columns))
}

/// What a yes-or-no question answers when the Engineer presses Enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAnswer {
    /// Enter agrees: the prompt shows `[Y/n]`.
    Yes,
    /// Enter declines: the prompt shows `[y/N]`.
    No,
}

/// Asks a yes-or-no question on a terminal: writes `<prompt> [Y/n] ` (or `[y/N]`, the default's
/// letter capital, the brackets painted as [`Meaning::Done`] when `screen` has colour) and reads
/// one line. `y` and `yes` in any case give true; an answer starting with `n` or `N` (`n`, `no`,
/// `nope`, `nah`) gives false; an empty line gives the default; anything else asks again. Closed
/// input, or a line that cannot be read, gives false whatever the default, so a pipe never agrees
/// to anything.
///
/// On a terminal (stdin), what was typed before the question is shown is discarded first, so an
/// Enter pressed during a long step does not answer it. With no terminal nothing is flushed.
///
/// It never asks with `ZAF_INSTANCE` set: the caller checks that and does not call this.
pub fn ask_yes<R: BufRead, W: Write>(
    screen: &Screen,
    prompt: &str,
    default: DefaultAnswer,
    input: &mut R,
    output: &mut W,
) -> bool {
    ask_yes_typed_on(screen, prompt, default, input, output, 0)
}

/// [`ask_yes`] with the terminal whose typed-ahead input is dropped given as descriptor `typed`.
fn ask_yes_typed_on<R: BufRead, W: Write>(
    screen: &Screen,
    prompt: &str,
    default: DefaultAnswer,
    input: &mut R,
    output: &mut W,
    typed: RawFd,
) -> bool {
    // SAFETY: `isatty` only reads whether `typed` is a terminal, and fails with 0 when it is not
    // open.
    if unsafe { libc::isatty(typed) } == 1 {
        process::flush_typed_on(typed);
    }
    let choices = match default {
        DefaultAnswer::Yes => "[Y/n]",
        DefaultAnswer::No => "[y/N]",
    };
    loop {
        let shown = write!(output, "{prompt} {} ", screen.paint(Meaning::Done, choices))
            .and_then(|()| output.flush());
        let mut line = String::new();
        let read = input.read_line(&mut line);
        // A closed input, or one that ended mid-line, leaves the question's line open: end it.
        if !line.ends_with('\n') {
            let _ = writeln!(output);
        }
        let answered = matches!(read, Ok(bytes) if bytes > 0);
        if shown.is_err() || !answered {
            return false;
        }
        let answer = line.trim().to_lowercase();
        match answer.as_str() {
            "" => return default == DefaultAnswer::Yes,
            "y" | "yes" => return true,
            _ if answer.starts_with('n') => return false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
