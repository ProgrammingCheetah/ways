//! The phase pane (F8, D10): a release phase's Claude session on a pseudo-terminal, in a pane laid
//! over Detail and Live while it runs.
//!
//! `o`, or Enter on the inbox's "a release phase is due" item, runs that phase's action as
//! `zaf begin` does, on a pseudo-terminal the chair holds (`session.rs` carries it out). While the
//! pane has the focus every key goes to Claude ([`route`]) but the `Ctrl-w` prefix, so `Ctrl-w c`
//! leaves it and `Ctrl-w h/j/k/l` move away. The session is told `ZAF_READY`; once its file
//! appears the status line says "Ready for Exit.", the chair types `/exit` and Enter, hangs up
//! after 5 s if the session has not ended ([`Ending`]), closes the pane and asks `Sign it? [Y/n]`.
//! What the Engineer types to Claude while the session is being ended is dropped, and once the
//! pane closes on its own no key counts for [`QUIET_FOR`] while the status line says "session
//! ended", so the end of a sentence meant for Claude never becomes the chair's keys or signs (R1).
//!
//! This file holds what is pure: the session as the chair sees it ([`Session`]), what the chair
//! asks of the runtime ([`PhaseCommand`]) and hears back ([`PhaseEvent`]), the keys as bytes, and
//! the pane itself, which draws the session's screen with tui-term. The chair's moves on them are
//! in `model/phase.rs`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use tui_term::vt100;
use tui_term::widget::{Cursor as TermCursor, PseudoTerminal};

use super::{Context, Cursor, Effect, Hint, Pane, Title, Tone, View, hint};
use crate::app::chair::{ChairState, InboxItem, ReleaseVersion};
use crate::app::hub::next::NextKind;
use crate::cli::chair::keys::Key;
use crate::cli::chair::theme::Theme;
use crate::domain::screen::Meaning;

/// What clears Claude's whole input before `/exit` is typed, so what the Engineer typed ahead is
/// never sent with it as a message: Ctrl-L, Claude Code's `chat:clearInput` in its Chat keys
/// (read from 2.1.328's default bindings).
pub const CLEAR_INPUT: &[u8] = b"\x0c";
/// Then Ctrl-U, which Claude Code's input takes as "delete to the line's start" and a terminal's
/// line discipline as "erase the line": the line is empty either way, a remapped Ctrl-L too.
pub const CLEAR_LINE: &[u8] = b"\x15";
/// The least time between two of the chair's keys while it ends a session: each goes in a write
/// of its own, never with the next, which Claude's input could take as one paste.
pub const KEY_GAP: Duration = Duration::from_millis(100);
/// How long after `/exit` the chair presses Enter: typed at once, Claude's input may take the two
/// as one paste and keep the Enter as a new line.
pub const ENTER_AFTER: Duration = Duration::from_millis(400);
/// How long after the ending began a session that has not ended is hung up.
pub const HANG_UP_AFTER: Duration = Duration::from_secs(5);
/// How long after the ending began a session that a hang-up did not end is killed.
pub const KILL_AFTER: Duration = Duration::from_secs(10);

/// What the chair types after [`CLEAR_INPUT`] to end a session, in order, each at least its time
/// after the one before.
const TYPING: [(&[u8], Duration); 3] = [
    (CLEAR_LINE, KEY_GAP),
    (b"/exit", KEY_GAP),
    (b"\r", ENTER_AFTER),
];
/// How long the chair takes no key once the session's pane closed on its own (R1): what the
/// Engineer was typing to Claude must not land on the chair's keys, nor answer `Sign it? [Y/n]`.
pub const QUIET_FOR: Duration = Duration::from_secs(1);

/// What the session is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The pane is open and the runtime works out the phase due and starts its action.
    Starting,
    /// The phase's action runs in the pane.
    Running,
    /// Its input was cleared and `/exit` is being typed: the session is being ended.
    Ending(Ending),
    /// The action has ended and the pane is closed; the signature, or the word that there is none,
    /// is on its way.
    Closed,
}

/// How far ending a session has gone: Claude's input cleared at `since` ([`CLEAR_INPUT`]), then
/// [`TYPING`] one key at a time (the line cleared again, `/exit`, Enter), then a hang-up and a
/// kill, each once, at [`HANG_UP_AFTER`] and [`KILL_AFTER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ending {
    /// When the ending began: the input was cleared.
    pub since: Instant,
    /// How many of [`TYPING`] were typed.
    pub typed: usize,
    /// When the last key was typed.
    pub last: Instant,
    /// Whether the session was hung up.
    pub hung_up: bool,
    /// Whether it was killed.
    pub killed: bool,
}

impl Ending {
    /// The input cleared at `now`.
    pub fn at(now: Instant) -> Self {
        Self {
            since: now,
            typed: 0,
            last: now,
            hung_up: false,
            killed: false,
        }
    }

    /// What is due at `now`, each step once, and the ending with it marked done. At most one key
    /// a call, so a loop that wakes late never types two in one write.
    #[must_use]
    pub fn step(mut self, now: Instant) -> (Self, Vec<PhaseCommand>) {
        let passed = now.saturating_duration_since(self.since);
        let mut due = Vec::new();
        if let Some((bytes, gap)) = TYPING.get(self.typed)
            && now.saturating_duration_since(self.last) >= *gap
        {
            self.typed += 1;
            self.last = now;
            due.push(PhaseCommand::Type(bytes.to_vec()));
        }
        if !self.hung_up && passed >= HANG_UP_AFTER {
            self.hung_up = true;
            due.push(PhaseCommand::HangUp);
        }
        if !self.killed && passed >= KILL_AFTER {
            self.killed = true;
            due.push(PhaseCommand::Kill);
        }
        (self, due)
    }
}

/// A question the chair asks on the status line about the session; every key answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `Sign it? [Y/n]`, `complete_phase`'s question about the document at that path.
    Sign(String),
    /// `Ctrl-w c` while the phase is not ready: end the session?
    Leave,
    /// `q` while the session runs: it ends with the chair.
    Quit,
}

impl Ask {
    /// The question the status line asks, about `phase` (`objectives`, `design`).
    pub fn question(&self, phase: &str) -> Question {
        let (words, yes, aside) = match self {
            Self::Sign(path) => ("Sign it?".to_owned(), true, Some(path.clone())),
            Self::Leave => (
                format!("Claude has not marked the {phase} ready. End the session?"),
                false,
                None,
            ),
            Self::Quit => (
                "Quit? The Claude session ends with zaf.".to_owned(),
                false,
                None,
            ),
        };
        Question { words, yes, aside }
    }
}

/// A question about the session, as the status line asks it; as plain words,
/// `Sign it? [Y/n]  <path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The question.
    pub words: String,
    /// What Enter answers: yes for the signature, no for leaving and quitting.
    pub yes: bool,
    /// What it is about, after the answers: the document to sign.
    pub aside: Option<String>,
}

impl std::fmt::Display for Question {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let answers = if self.yes { "[Y/n]" } else { "[y/N]" };
        write!(formatter, "{} {answers}", self.words)?;
        match &self.aside {
            Some(aside) => write!(formatter, "  {aside}"),
            None => Ok(()),
        }
    }
}

/// The status line while the chair asks about the session, as it asks before a control: the
/// question, then `[Y/n]` with the answer Enter gives in capitals and colour, then what it is
/// about.
pub fn spans(question: &Question, theme: Theme) -> Vec<Span<'static>> {
    let paint = |meaning: Meaning, text: &str| Span::styled(text.to_owned(), theme.style(meaning));
    let mut spans = vec![
        paint(Meaning::Waiting, "? "),
        Span::styled(question.words.clone(), theme.bold()),
        Span::raw(" ["),
    ];
    if question.yes {
        spans.extend([paint(Meaning::Done, "Y"), Span::raw("/n]")]);
    } else {
        spans.extend([Span::raw("y/"), paint(Meaning::Done, "N"), Span::raw("]")]);
    }
    if let Some(aside) = &question.aside {
        spans.extend([Span::raw("  "), paint(Meaning::Noun, aside)]);
    }
    spans
}

/// The release phase the chair runs, from `o` until its signature is settled.
#[derive(Debug, Clone)]
pub struct Session {
    /// The project's root.
    pub root: PathBuf,
    /// The version begun.
    pub version: ReleaseVersion,
    /// The phase run (`objectives`, `design`), once the runtime has worked it out.
    pub phase: Option<String>,
    /// What it is doing.
    pub stage: Stage,
    /// Whether the session wrote its ready file.
    pub ready: bool,
    /// Whether the Engineer chose to end it (`Ctrl-w c`, the palette): its pane then closes as
    /// asked, not on its own.
    pub left: bool,
    /// The question on the status line about it, while one is asked.
    pub ask: Option<Ask>,
    /// What the session's terminal shows, as of its last output.
    pub screen: Option<vt100::Screen>,
}

impl Session {
    /// A session of `version` in the project at `root`, starting.
    pub fn new(root: PathBuf, version: ReleaseVersion) -> Self {
        Self {
            root,
            version,
            phase: None,
            stage: Stage::Starting,
            ready: false,
            left: false,
            ask: None,
            screen: None,
        }
    }

    /// Whether its action may still be running: from `o` until the action has ended.
    pub fn live(&self) -> bool {
        !matches!(self.stage, Stage::Closed)
    }

    /// The phase as said, `phase` until the runtime has worked it out.
    pub fn phase_word(&self) -> &str {
        self.phase.as_deref().unwrap_or("phase")
    }
}

/// The chair's phase: the session, while there is one, and whether the next read should select
/// the release phase due next (the design, once the objectives are signed).
#[derive(Debug, Clone, Default)]
pub struct PhaseState {
    /// The session, from `o` until its signature is settled.
    pub session: Option<Session>,
    /// Whether the next read selects the phase item of the inbox.
    pub follow: bool,
    /// Until when no key counts, once the session's pane closed on its own ([`QUIET_FOR`]).
    pub quiet_until: Option<Instant>,
}

/// What the chair asks of the runtime about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseCommand {
    /// Open a pseudo-terminal of `rows` by `columns` and run the release phase due of `version`
    /// in the project at `root` on it, as `zaf begin` does.
    Start {
        /// The project's root.
        root: PathBuf,
        /// The version to begin.
        version: ReleaseVersion,
        /// The pane's rows below its title.
        rows: u16,
        /// The pane's columns.
        columns: u16,
    },
    /// Write these bytes to the session, as typed.
    Type(Vec<u8>),
    /// Hang the session up: SIGHUP to its process group.
    HangUp,
    /// Kill it: SIGKILL to its process group.
    Kill,
    /// The line that answers the sign question, newline included.
    Answer(String),
}

/// What the runtime tells the chair about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseEvent {
    /// The phase due is this one, and its action has started.
    Started(String),
    /// The session wrote its ready file.
    Ready,
    /// Nothing ran: why, in a line.
    Refused(String),
    /// The action has ended: whether the ready file is there, and whether that, or its exit 0,
    /// leads on to the signature.
    Exited {
        /// Whether the ready file is there.
        ready: bool,
        /// Whether the signature follows.
        leads: bool,
    },
    /// `complete_phase` asks this (`Sign <path>? [Y/n] `) and waits for the answer.
    Asked(String),
    /// It is settled: what to say, and the phase due next once a document was signed.
    Done {
        /// How it reads.
        tone: Tone,
        /// What to say.
        words: String,
        /// The phase due next (`design`), when one is.
        next: Option<String>,
    },
}

/// Where a key typed while the pane has the focus goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// `Ctrl-w`: the chair's window prefix, the one key Claude never gets.
    Window,
    /// These bytes, to Claude.
    Bytes(Vec<u8>),
    /// Nowhere: a release, or a key with no bytes.
    Nothing,
}

/// Where `event` goes while the pane has the focus: `Ctrl-w` to the chair, every other key to
/// Claude as a terminal sends it (the cursor keys as the application asked, `application_cursor`).
pub fn route(event: &KeyEvent, application_cursor: bool) -> Routed {
    if event.kind == KeyEventKind::Release {
        return Routed::Nothing;
    }
    let modifiers = event.modifiers;
    let control = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    if control && matches!(event.code, KeyCode::Char('w' | 'W')) {
        return Routed::Window;
    }
    let code = 1
        + u8::from(modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(alt)
        + 4 * u8::from(control);
    let cursor = |letter: char| {
        if code > 1 {
            format!("\x1b[1;{code}{letter}").into_bytes()
        } else if application_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |number: u8| {
        if code > 1 {
            format!("\x1b[{number};{code}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let mut bytes = match event.code {
        KeyCode::Char(c) if control => match control_byte(c) {
            Some(byte) => vec![byte],
            None => return Routed::Nothing,
        },
        KeyCode::Char(c) => c.to_string().into_bytes(),
        // Shift-Enter and Alt-Enter are a new line in Claude's input, as its terminal set-up has it.
        KeyCode::Enter if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
            return Routed::Bytes(b"\x1b\r".to_vec());
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Up => return Routed::Bytes(cursor('A')),
        KeyCode::Down => return Routed::Bytes(cursor('B')),
        KeyCode::Right => return Routed::Bytes(cursor('C')),
        KeyCode::Left => return Routed::Bytes(cursor('D')),
        KeyCode::Home => return Routed::Bytes(cursor('H')),
        KeyCode::End => return Routed::Bytes(cursor('F')),
        KeyCode::Insert => return Routed::Bytes(tilde(2)),
        KeyCode::Delete => return Routed::Bytes(tilde(3)),
        KeyCode::PageUp => return Routed::Bytes(tilde(5)),
        KeyCode::PageDown => return Routed::Bytes(tilde(6)),
        KeyCode::F(number) => match function_key(number) {
            Some(bytes) => return Routed::Bytes(bytes),
            None => return Routed::Nothing,
        },
        _ => return Routed::Nothing,
    };
    if alt {
        bytes.insert(0, 0x1b);
    }
    Routed::Bytes(bytes)
}

/// The byte `Ctrl` with `c` sends: `Ctrl-a` 1 to `Ctrl-z` 26, and the few others a terminal has.
fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        letter @ 'a'..='z' => Some(letter as u8 - b'a' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// The bytes of function key `number`, as xterm sends them.
fn function_key(number: u8) -> Option<Vec<u8>> {
    let bytes: &[u8] = match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(bytes.to_vec())
}

/// The bytes the chair's own `key` stands for, typed into the session: what reaches Claude when a
/// key comes through the chair rather than straight from the terminal (the palette, a test).
pub fn bytes_of(key: Key) -> Vec<u8> {
    match key {
        Key::Char(c) => c.to_string().into_bytes(),
        Key::Enter => b"\r".to_vec(),
        Key::Esc => b"\x1b".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Backspace => b"\x7f".to_vec(),
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
        Key::CtrlW => vec![0x17],
        Key::CtrlU => vec![0x15],
        Key::CtrlC => vec![0x03],
    }
}

/// The bytes a paste of `text` sends to the session: wrapped in `ESC[200~` and `ESC[201~` when its
/// terminal asked for bracketed paste (`bracketed`), so Claude takes a newline in it as text, else
/// as it came, as a terminal would send it. Every control character but the tab and the line ends
/// is dropped first, the escape with them: no end marker can be in the text or be made of what is
/// left, so the paste never ends early and turns the rest into keys.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text: String = text
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .collect();
    if bracketed {
        format!("{PASTE_START}{text}{PASTE_END}").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// What opens a bracketed paste.
const PASTE_START: &str = "\x1b[200~";
/// What closes it.
const PASTE_END: &str = "\x1b[201~";

/// The version the inbox's phase item `item` is due for: its title's (`Release 0.5.3: its design
/// is due`), else the one its command begins (`… zaf begin --version 0.5.3`).
pub fn version_of(item: &InboxItem) -> Option<ReleaseVersion> {
    let from_title = item
        .title
        .strip_prefix("Release ")
        .and_then(|rest| rest.split_once(':'))
        .map(|(version, _)| version);
    let from_command = || {
        item.command
            .as_deref()?
            .rsplit_once("zaf begin --version ")
            .map(|(_, version)| version)
    };
    from_title
        .or_else(from_command)
        .map(str::trim)
        .filter(|version| !version.is_empty() && !version.contains(char::is_whitespace))
        .map(ReleaseVersion::from)
}

/// The version `o` runs the phase of in `state`: its inbox's first release phase item's.
pub fn first_due(state: &ChairState) -> Option<ReleaseVersion> {
    state
        .inbox
        .iter()
        .filter(|item| item.kind == NextKind::Phase)
        .find_map(version_of)
}

/// The phase pane.
pub struct Phase;

impl Pane for Phase {
    fn title(&self, view: &View<'_>) -> Title {
        let Some(session) = view.session else {
            return Title::new("claude");
        };
        let right = match session.stage {
            Stage::Starting => view.paint(Meaning::Dim, "starting"),
            Stage::Running if session.ready => view.paint(Meaning::Done, "ready"),
            Stage::Running => view.paint(Meaning::Dim, "^w c leaves"),
            Stage::Ending(_) => view.paint(Meaning::Dim, "ending"),
            Stage::Closed => view.paint(Meaning::Dim, "ended"),
        };
        Title {
            text: "claude".to_owned(),
            after: vec![
                view.paint(Meaning::Verb, session.phase_word().to_owned()),
                View::plain(" "),
                view.paint(Meaning::Noun, session.version.to_string()),
            ],
            right: vec![right],
        }
    }

    fn body(&self, view: &View<'_>, _width: u16, _rows: u16) -> Vec<Line<'static>> {
        let words = view.session.map_or_else(
            || " no session".to_owned(),
            |session| {
                format!(
                    " starting the {} of {}…",
                    session.phase_word(),
                    session.version
                )
            },
        );
        vec![Line::from(vec![
            View::plain(" "),
            view.paint(Meaning::Verb, view.spinner()),
            view.paint(Meaning::Dim, words),
        ])]
    }

    fn draw_body(&self, view: &View<'_>, area: Rect, buffer: &mut Buffer) -> bool {
        let Some(screen) = view.session.and_then(|session| session.screen.as_ref()) else {
            return false;
        };
        PseudoTerminal::new(screen)
            .cursor(TermCursor::default().visibility(view.focused))
            .render(area, buffer);
        true
    }

    fn key(&self, _key: Key, _context: Context<'_>, _cursor: &mut Cursor) -> Effect {
        Effect::Ignored
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("keys", "go to Claude"),
            hint("^w c", "leave Claude"),
            hint("^w h/j/k/l", "other panes"),
        ]
    }
}

#[cfg(test)]
mod tests;
