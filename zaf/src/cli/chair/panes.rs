//! The chair's panes (D4, D5) and the [`Pane`] trait each one implements.
//!
//! A pane is stateless: it draws from a [`View`] (the project's state as shown, where the
//! Engineer is, the theme) and answers a key with an [`Effect`] the chair carries out, moving the
//! [`Cursor`] itself where it must. So a pane is one file that knows nothing of the others, and a
//! feature still to come fills in its own file:
//!
//! - [`inbox`], [`problems`], [`now`], [`epics`]: the left column and the epics, built.
//! - [`detail`]: a question, a problem and the answer box; its epic views, reading and graph (F6)
//!   and its why view (F7) in `detail/`.
//! - [`live`]: a ticket followed live, its reports and its phase run's log (F7).
//! - [`phase`]: a release phase's Claude session on a pseudo-terminal (F8), laid over Detail and
//!   Live while it runs; it draws its body itself ([`Pane::draw_body`]).
//!
//! What a key asks to read besides the state (an epic's body, a ticket's log) comes back into
//! [`Looks`], which a pane draws from as it draws from the state.

pub mod detail;
pub mod epics;
pub mod inbox;
pub mod live;
pub mod now;
pub mod phase;
pub mod problems;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::keys::Key;
use super::layout::PaneId;
use super::theme::Theme;
use crate::app::chair::look::{EpicRead, GraphRead, TicketRead};
use crate::app::chair::{ChairAction, ChairState, ReleaseVersion, SquareState};
use crate::cli::steps::FRAMES;
use crate::domain::TicketId;
use crate::domain::screen::Meaning;

/// Where the Engineer is in each pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The selected inbox item.
    pub inbox: usize,
    /// The selected problem.
    pub problem: usize,
    /// The selected running instance in Now.
    pub running: usize,
    /// The selected epic.
    pub epic: usize,
    /// The selected ticket of that epic.
    pub ticket: usize,
    /// How many lines Detail is scrolled down.
    pub scroll: u16,
    /// What Detail shows.
    pub detail: DetailView,
    /// Whether the graph Detail draws is the one as planned (`v`), not the tickets' own.
    pub before: bool,
    /// The ticket Live follows; none follows the first that runs.
    pub live: Option<TicketId>,
}

impl Cursor {
    /// The cursor with every index brought back inside what `state` holds.
    pub fn clamp(&mut self, state: &ChairState) {
        let last = |len: usize| len.saturating_sub(1);
        self.inbox = self.inbox.min(last(state.inbox.len()));
        self.problem = self.problem.min(last(state.problems.len()));
        self.running = self.running.min(last(state.now.running.len()));
        self.epic = self.epic.min(last(state.epics.len()));
        let squares = state
            .epics
            .get(self.epic)
            .map_or(0, |epic| epic.squares.len());
        self.ticket = self.ticket.min(last(squares));
    }
}

/// What the chair holds beside the state on screen: what a key asked to read and what came back
/// (read on the tab's gatherer thread, never where the screen is drawn), and the epics whose
/// signature waits to be written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looks {
    /// The epic Detail reads: the one asked for, and what came back once it has.
    pub epic: Option<(TicketId, Option<EpicRead>)>,
    /// The ticket Live follows: the one asked for, and what came back once it has.
    pub ticket: Option<(TicketId, Option<TicketRead>)>,
    /// The epic whose graph Detail draws: the one asked for, and what came back once it has.
    pub graph: Option<(TicketId, Option<GraphRead>)>,
    /// The epics whose signature waits to be written, or was written since the last read, by
    /// project.
    pub signing: BTreeSet<(PathBuf, TicketId)>,
}

impl Looks {
    /// What was read of `epic`, once it came back.
    pub fn epic(&self, epic: TicketId) -> Option<&EpicRead> {
        self.epic
            .as_ref()
            .filter(|(asked, _)| *asked == epic)
            .and_then(|(_, read)| read.as_ref())
    }

    /// What was read of `ticket`, once it came back.
    pub fn ticket(&self, ticket: TicketId) -> Option<&TicketRead> {
        self.ticket
            .as_ref()
            .filter(|(asked, _)| *asked == ticket)
            .and_then(|(_, read)| read.as_ref())
    }

    /// What was read of the graph of `epic`, once it came back.
    pub fn graph(&self, epic: TicketId) -> Option<&GraphRead> {
        self.graph
            .as_ref()
            .filter(|(asked, _)| *asked == epic)
            .and_then(|(_, read)| read.as_ref())
    }

    /// Whether `epic` of the project at `root` is being signed.
    pub fn signs(&self, root: &Path, epic: TicketId) -> bool {
        self.signing.contains(&(root.to_owned(), epic))
    }
}

/// What the Detail pane shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailView {
    /// The selected inbox item.
    #[default]
    Item,
    /// The selected problem.
    Problem,
    /// A short look at the selected epic.
    Epic,
    /// The selected epic's stage and body, and its release's objectives and design.
    Read,
    /// The selected epic's graph, a row for each layer.
    Graph,
    /// Why a ticket waits, its next phase, its pull request and its cost.
    Why(TicketId),
    /// The answer box.
    Answer,
}

/// The answer box: the question it rules, the ruling typed so far, and how it is seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerBox {
    /// The question.
    pub question: TicketId,
    /// The ruling as typed, the first paragraph of Claude's proposal to begin with.
    pub text: String,
    /// How many lines the box is scrolled up from the cursor, which is at the end: 0 shows the
    /// cursor. Typing scrolls back to it.
    pub back: usize,
    /// What the layout had maximised before the box took the whole screen for its text; none
    /// while the box sits in Detail as laid out.
    pub zoomed: Option<Option<PaneId>>,
}

impl AnswerBox {
    /// The box on `question`, holding `text`, at the cursor, in Detail as laid out.
    pub fn new(question: TicketId, text: String) -> Self {
        Self {
            question,
            text,
            back: 0,
            zoomed: None,
        }
    }
}

/// How a status line message reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Something was done: a green `✓`.
    Done,
    /// Something failed or was refused: a red `✗`.
    Failed,
    /// Something was taken back: `↶`.
    Undone,
    /// Anything else, dim.
    Plain,
}

/// What a pane draws from.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    /// The project on screen, its pending and ruled questions taken out; none before its first
    /// read ends.
    pub state: Option<&'a ChairState>,
    /// The project's name, for what is said before its first read.
    pub project: &'a str,
    /// How to paint.
    pub theme: Theme,
    /// Whether this pane has the focus.
    pub focused: bool,
    /// The frame count, for the spinners.
    pub frame: usize,
    /// Where the Engineer is.
    pub cursor: &'a Cursor,
    /// The answer box, while it is open.
    pub answer: Option<&'a AnswerBox>,
    /// The release phase's session, while there is one.
    pub session: Option<&'a phase::Session>,
    /// What keys asked to read, and the signatures that wait.
    pub looks: &'a Looks,
}

impl View<'_> {
    /// `text` painted as `meaning`.
    pub fn paint(&self, meaning: Meaning, text: impl Into<String>) -> Span<'static> {
        Span::styled(text.into(), self.theme.style(meaning))
    }

    /// `text` unpainted.
    pub fn plain(text: impl Into<String>) -> Span<'static> {
        Span::raw(text.into())
    }

    /// The spinner's frame now.
    pub fn spinner(&self) -> &'static str {
        FRAMES[self.frame % FRAMES.len()]
    }

    /// `▸` when this pane has the focus and `selected`, else a space.
    pub fn marker(&self, selected: bool) -> Span<'static> {
        if self.focused && selected {
            self.paint(Meaning::Verb, "▸")
        } else {
            Self::plain(" ")
        }
    }

    /// A square's symbol: `■` done, the spinner running, `?` waiting on the Engineer, `✗` a
    /// problem, `□` waiting or ready. Each has its colour too, never alone.
    pub fn square(&self, state: SquareState) -> Span<'static> {
        match state {
            SquareState::Done => self.paint(Meaning::Done, "■"),
            SquareState::Running => self.paint(Meaning::Verb, self.spinner()),
            SquareState::Question => self.paint(Meaning::Waiting, "?"),
            SquareState::Failed => self.paint(Meaning::Failed, "✗"),
            SquareState::Ready | SquareState::Waiting => self.paint(Meaning::Dim, "□"),
        }
    }

    /// The line said where the project has not been read yet.
    pub fn reading(&self) -> Line<'static> {
        Line::from(vec![
            Self::plain("  "),
            self.paint(Meaning::Verb, self.spinner()),
            self.paint(Meaning::Dim, format!(" reading {}…", self.project)),
        ])
    }
}

/// A pane's title row: its title, drawn in the focus colour or bold, what follows it in its own
/// colours, and the words at its right when they fit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Title {
    /// The title.
    pub text: String,
    /// What follows the title: a count.
    pub after: Vec<Span<'static>>,
    /// The words at the right.
    pub right: Vec<Span<'static>>,
}

impl Title {
    /// A title with nothing beside it.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// A key and what it does, for the key bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    /// The key as shown: `Enter`, `j/k`.
    pub key: &'static str,
    /// What it does: `answer`.
    pub what: &'static str,
}

/// A hint.
pub const fn hint(key: &'static str, what: &'static str) -> Hint {
    Hint { key, what }
}

/// What a pane asks of the chair after a key or a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Not this pane's key: the chair's own keys may take it.
    Ignored,
    /// Done; the cursor may have moved.
    Done,
    /// Focus that pane.
    Focus(PaneId),
    /// Open the answer box on that question.
    Answer(TicketId),
    /// Sign that epic, once its time to be taken back has passed.
    Sign(TicketId),
    /// Write that at once.
    Write(ChairAction),
    /// Say that on the status line.
    Note(Tone, String),
    /// Run the release phase due of that version, in the phase pane.
    RunPhase(ReleaseVersion),
}

/// What a key or a click is told besides the cursor.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The project on screen, as shown.
    pub state: Option<&'a ChairState>,
    /// The pane's rows, its title included.
    pub rows: u16,
    /// The pane's columns.
    pub width: u16,
}

/// A pane of the chair.
pub trait Pane: Sync {
    /// Its title row.
    fn title(&self, view: &View<'_>) -> Title;

    /// Its rows below the title, at most `rows` of them, each cut at `width` when drawn.
    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>>;

    /// Draws its body into `area` of `buffer` itself, cell by cell, and says so; false leaves it
    /// to [`body`](Self::body)'s lines. Only the phase pane draws itself: a terminal's screen.
    fn draw_body(&self, view: &View<'_>, area: Rect, buffer: &mut Buffer) -> bool {
        let _ = (view, area, buffer);
        false
    }

    /// What `key` does while it has the focus.
    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect;

    /// What a click on its `row` (the title is row 0) at `column` does; it already has the focus.
    fn click(&self, row: u16, column: u16, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let _ = (row, column, context, cursor);
        Effect::Done
    }

    /// The keys the key bar shows while it has the focus.
    fn hints(&self, view: &View<'_>) -> Vec<Hint>;
}

/// The pane `id` names.
pub fn pane(id: PaneId) -> &'static dyn Pane {
    match id {
        PaneId::Inbox => &inbox::Inbox,
        PaneId::Problems => &problems::Problems,
        PaneId::Now => &now::Now,
        PaneId::Epics => &epics::Epics,
        PaneId::Detail => &detail::Detail,
        PaneId::Live => &live::Live,
        PaneId::Phase => &phase::Phase,
    }
}

/// The first of `len` rows shown in a window of `rows` so `selected` shows.
pub fn first_shown(selected: usize, rows: usize) -> usize {
    if rows == 0 {
        0
    } else {
        selected.saturating_sub(rows - 1)
    }
}

/// `selected` moved one step by `key` within `len`, or none when `key` is no step.
pub fn step(key: Key, selected: usize, len: usize) -> Option<usize> {
    match key {
        Key::Char('j') | Key::Down => Some((selected + 1).min(len.saturating_sub(1))),
        Key::Char('k') | Key::Up => Some(selected.saturating_sub(1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
