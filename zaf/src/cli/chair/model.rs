//! The chair as a state machine (D4 to D7): what is on screen, and what each key, click, read and
//! write does to it. Pure: keys in, [`Command`]s out. Nothing here reads a port, sleeps or draws;
//! the runtime carries the commands out and the drawing reads the [`Chair`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::layout::Rect;

use super::build::{self, Answered, Ask, Control};
use super::keys::Key;
use super::layout::{Axis, Direction, Layout, PaneId, STEP};
use super::palette::{self, Run};
use super::panes::detail::AnswerView;
use super::panes::phase::{PhaseCommand, PhaseState, Question};
use super::panes::{
    self, AnswerBox, Context, Cursor, DetailView, Effect, Hint, Looks, Tone, View, hint, pane,
};
use super::pings::{self, Notice, Ping};
use super::text::width;
use super::theme::Theme;
use crate::app::chair::gatherer::GatherEvent;
use crate::app::chair::look::{Look, Looked};
use crate::app::chair::pending::PendingQueue;
use crate::app::chair::{ChairAction, ChairState, Outcome, Ruling, Signing};
use crate::app::hub::next::NextKind;
use crate::domain::{EpicStage, TicketId, printable};
use crate::manifest::ProjectName;

/// The least columns the chair draws in.
pub const MIN_COLUMNS: u16 = 80;
/// The least rows the chair draws in.
pub const MIN_ROWS: u16 = 24;
/// How long a status line message stays.
pub const NOTE_FOR: Duration = Duration::from_secs(6);
/// Where the tabs begin on the tab bar, after ` zaf  `.
pub const TABS_AT: u16 = 6;

/// One project's tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    /// The project's name.
    pub name: ProjectName,
    /// The project's root.
    pub root: PathBuf,
    /// Its newest state; none before its first read ends.
    pub state: Option<ChairState>,
    /// When the read under way began; none while no read runs.
    pub reading: Option<SystemTime>,
}

impl Tab {
    /// A tab for the project `name` at `root`, not read yet.
    pub fn new(name: ProjectName, root: PathBuf) -> Self {
        Self {
            name,
            root,
            state: None,
            reading: None,
        }
    }
}

/// What keys go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// The panes.
    Normal,
    /// `Ctrl-w` was pressed: the next key is a window command.
    Window,
    /// The palette is open on this entry.
    Palette(usize),
    /// The answer box is open.
    Answer(AnswerBox),
    /// The status line asks before a control runs (F9, F15).
    Ask(Ask),
}

/// A status line message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// How it reads.
    pub tone: Tone,
    /// What it says.
    pub words: String,
    /// When it goes.
    pub until: Instant,
}

/// What the runtime is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Close the chair, writing what waits first.
    Quit,
    /// Write this now.
    Write(ChairAction),
    /// Read this tab's project again now.
    ReadNow(usize),
    /// Read this for this tab, on its gatherer's thread.
    Look(usize, Look),
    /// The tab on screen changed: read the new one often and the old one seldom.
    Shown {
        /// The tab that was on screen.
        from: usize,
        /// The tab on screen now.
        to: usize,
    },
    /// Ring the bell.
    Bell,
    /// Post a desktop notification (F10).
    Notify(Notice),
    /// Run a control the Engineer said yes to (F9, F15).
    Control(Control),
    /// Do this about the release phase's session (F8).
    Phase(PhaseCommand),
}

/// What the status line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status<'a> {
    /// A write waits to be taken back: what it does, the seconds left and how many more wait.
    Pending {
        /// What it does: `#285 ruled`.
        words: String,
        /// The seconds left.
        left: u64,
        /// The other writes that wait.
        more: usize,
    },
    /// A message.
    Note(&'a Note),
    /// Nothing: the reminders.
    Idle,
    /// A question before a control runs.
    Ask(&'a Ask),
    /// A question about the release phase's session, which the next key answers (F8).
    Phase(Question),
}

/// The chair.
#[derive(Debug, Clone)]
pub struct Chair {
    /// One tab per project.
    pub tabs: Vec<Tab>,
    /// The tab on screen.
    pub active: usize,
    /// The focused pane.
    pub focus: PaneId,
    /// How the panes share the screen.
    pub layout: Layout,
    /// What keys go to.
    pub mode: Mode,
    /// Where the Engineer is in each pane.
    pub cursor: Cursor,
    /// What waits to be written.
    pub pending: PendingQueue<ChairAction>,
    /// The questions kept out of the inbox: their ruling waits, or was written since the last
    /// read.
    pub hidden: BTreeSet<(PathBuf, TicketId)>,
    /// The status line's message.
    pub note: Option<Note>,
    /// The frame count, for the spinners.
    pub frame: usize,
    /// The terminal's columns and rows.
    pub size: (u16, u16),
    /// How to paint.
    pub theme: Theme,
    /// The release phase's session, while there is one.
    pub phase: PhaseState,
    /// What keys asked to read beside the state on screen, and the signatures that wait.
    pub looks: Looks,
    /// The fix running now, as its command reads (`zaf doctor`): one at a time, so a second
    /// Enter is refused until it has ended.
    pub fixing: Option<String>,
}

impl Chair {
    /// The chair over `tabs`, `active` on screen, in a terminal of `size` (columns, rows).
    pub fn new(tabs: Vec<Tab>, active: usize, theme: Theme, size: (u16, u16)) -> Self {
        let active = active.min(tabs.len().saturating_sub(1));
        Self {
            tabs,
            active,
            focus: PaneId::Inbox,
            layout: Layout::chair(),
            mode: Mode::Normal,
            cursor: Cursor::default(),
            pending: PendingQueue::default(),
            hidden: BTreeSet::new(),
            note: None,
            frame: 0,
            size,
            theme,
            phase: PhaseState::default(),
            looks: Looks::default(),
            fixing: None,
        }
    }

    /// The panes' area: below the tab bar and its rule, above the status line and the key bar.
    pub fn panes_area(&self) -> Rect {
        let (columns, rows) = self.size;
        Rect {
            x: 0,
            y: 2,
            width: columns,
            height: rows.saturating_sub(4),
        }
    }

    /// Whether the terminal is large enough to draw in.
    pub fn fits(&self) -> bool {
        let (columns, rows) = self.size;
        columns >= MIN_COLUMNS && rows >= MIN_ROWS && self.layout.fits(self.panes_area())
    }

    /// The tab on screen.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// The questions of `root` kept out of its inbox.
    fn hidden_of(&self, root: &Path) -> BTreeSet<TicketId> {
        self.hidden
            .iter()
            .filter(|(hidden_root, _)| hidden_root == root)
            .map(|(_, question)| *question)
            .collect()
    }

    /// `tab`'s state as shown: its hidden questions taken out.
    pub fn shown_of(&self, tab: usize) -> Option<ChairState> {
        let tab = self.tabs.get(tab)?;
        let state = tab.state.as_ref()?;
        Some(state.without(&self.hidden_of(&tab.root)))
    }

    /// The state on screen, as shown.
    pub fn shown(&self) -> Option<ChairState> {
        self.shown_of(self.active)
    }

    /// The view `pane` draws from, over `state`.
    pub fn view<'a>(&'a self, state: Option<&'a ChairState>, pane: PaneId) -> View<'a> {
        View {
            state,
            project: self.tab().name.as_str(),
            theme: self.theme,
            focused: pane == self.focus && !matches!(self.mode, Mode::Palette(_)),
            frame: self.frame,
            cursor: &self.cursor,
            answer: match &self.mode {
                Mode::Answer(answer) => Some(answer),
                _ => None,
            },
            session: self.phase.session.as_ref(),
            looks: &self.looks,
        }
    }

    /// The badge beside tab `index`: how many things wait on the Engineer there.
    pub fn badge(&self, index: usize) -> usize {
        self.shown_of(index).map_or(0, |state| state.waiting())
    }

    /// Each tab's columns on the tab bar, `[start, end)`.
    pub fn tab_spans(&self) -> Vec<(u16, u16)> {
        let mut at = TABS_AT;
        (0..self.tabs.len())
            .map(|index| {
                let badge = self.badge(index);
                let badge_cells = if badge > 0 {
                    1 + 1 + badge.to_string().len()
                } else {
                    0
                };
                let cells = width(self.tabs[index].name.as_str()) + badge_cells;
                let start = at;
                let end = start.saturating_add(u16::try_from(cells).unwrap_or(u16::MAX));
                at = end.saturating_add(3);
                (start, end)
            })
            .collect()
    }

    /// What the status line says at `now`.
    pub fn status(&self, now: Instant) -> Status<'_> {
        // While no key counts, the status line says why before it asks anything.
        if self.quiet(now)
            && let Some(note) = &self.note
        {
            return Status::Note(note);
        }
        if let Some(question) = self.phase_question() {
            return Status::Phase(question);
        }
        if let Mode::Ask(ask) = &self.mode {
            return Status::Ask(ask);
        }
        if let Some(newest) = self.pending.newest() {
            return Status::Pending {
                words: newest.action.done_words(),
                left: newest.seconds_left(now),
                more: self.pending.len() - 1,
            };
        }
        match &self.note {
            Some(note) if note.until > now => Status::Note(note),
            _ => Status::Idle,
        }
    }

    /// The keys the key bar shows.
    pub fn hints(&self) -> Vec<Hint> {
        if let Some(hints) = self.phase_hints() {
            return hints;
        }
        let mut hints = match &self.mode {
            Mode::Window => vec![
                hint("h/j/k/l", "focus"),
                hint("+ -", "height"),
                hint("< >", "width"),
                hint("=", "equal"),
                hint("o", "maximise"),
                hint("Esc", "cancel"),
            ],
            Mode::Palette(_) => vec![
                hint("j/k", "move"),
                hint("Enter", "run"),
                hint("Esc", "close"),
            ],
            Mode::Ask(ask) => build::hints(ask),
            Mode::Answer(_) => vec![
                hint("type", "edit"),
                hint("Enter", "rule"),
                hint("Ctrl-U", "clear"),
                hint("Esc", "cancel"),
                hint("↑/↓", "scroll"),
                hint("^w", "zoom"),
            ],
            Mode::Normal => {
                let state = self.shown();
                pane(self.focus).hints(&self.view(state.as_ref(), self.focus))
            }
        };
        if !self.pending.is_empty() {
            hints.insert(0, hint("u", "undo"));
        }
        hints
    }

    /// Says `words` on the status line for a while.
    pub fn say(&mut self, tone: Tone, words: impl Into<String>, now: Instant) {
        self.note = Some(Note {
            tone,
            words: words.into(),
            until: now + NOTE_FOR,
        });
    }

    /// The terminal is now `columns` by `rows`; splits that no longer fit go back to their first
    /// shares.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.size = (columns, rows);
        if !self.layout.fits(self.panes_area()) && self.layout.maximised().is_none() {
            self.layout.reset();
        }
    }

    /// Focuses `pane`, Detail following what it shows.
    pub fn set_focus(&mut self, pane: PaneId) {
        self.focus = pane;
        match pane {
            PaneId::Inbox => {
                self.cursor.detail = DetailView::Item;
                self.cursor.scroll = 0;
            }
            PaneId::Problems => {
                self.cursor.detail = DetailView::Problem;
                self.cursor.scroll = 0;
            }
            PaneId::Epics
                if !matches!(
                    self.cursor.detail,
                    DetailView::Graph | DetailView::Read | DetailView::Why(_)
                ) =>
            {
                self.cursor.detail = DetailView::Epic;
                self.cursor.scroll = 0;
            }
            _ => {}
        }
    }

    /// The context a key or click in `pane` is told.
    fn context<'a>(&self, state: Option<&'a ChairState>, pane: PaneId) -> Context<'a> {
        let rect = self
            .layout
            .panes(self.panes_area())
            .into_iter()
            .find(|(shown, _)| *shown == pane)
            .map(|(_, rect)| rect)
            .unwrap_or_default();
        Context {
            state,
            rows: rect.height,
            width: rect.width,
        }
    }

    /// What `key` does at `now`.
    pub fn key(&mut self, key: Key, now: Instant) -> Vec<Command> {
        let waiting = self.sign_waits();
        let commands = self.key_at(key, now);
        self.sign_shown(waiting, now);
        commands
    }

    fn key_at(&mut self, key: Key, now: Instant) -> Vec<Command> {
        if self.hush(now) {
            return Vec::new();
        }
        if let Some(commands) = self.phase_key(key, now) {
            return commands;
        }
        let mut commands = match self.mode.clone() {
            Mode::Window => self.window(key, now),
            Mode::Palette(selected) => self.palette(key, selected, now),
            Mode::Answer(answer) => self.answer(key, answer, now),
            Mode::Ask(ask) => self.ask(key, &ask, now),
            Mode::Normal => self.normal(key, now),
        };
        commands.extend(self.follow(false));
        commands
    }

    /// The Engineer pasted `text` (the terminal's bracketed paste): it is text, never keys, so a
    /// newline in it rules nothing, signs nothing and quits nothing. The answer box takes it with
    /// its newlines; Claude gets it as a paste; anywhere else it is dropped and the status line
    /// says so.
    pub fn paste(&mut self, text: &str, now: Instant) -> Vec<Command> {
        if self.hush(now) {
            return Vec::new();
        }
        if let Some(commands) = self.phase_paste(text, now) {
            return commands;
        }
        if let Mode::Answer(answer) = &mut self.mode {
            answer.text.push_str(&pasted(text));
            answer.back = 0;
            return Vec::new();
        }
        self.say(
            Tone::Plain,
            "a paste goes only into the answer box or to Claude: nothing was done",
            now,
        );
        Vec::new()
    }

    /// The reads what is on screen asks for and has not asked yet: the epic Detail reads, and
    /// the ticket Live follows; `again` asks for that ticket once more, as a fresh read of its
    /// project does, so Live keeps up with its run.
    pub fn follow(&mut self, again: bool) -> Vec<Command> {
        let Some(state) = self.shown() else {
            return Vec::new();
        };
        let mut commands = Vec::new();
        let reading = (self.cursor.detail == DetailView::Read)
            .then(|| panes::epics::selected(&state, &self.cursor).map(|epic| epic.number))
            .flatten();
        match reading {
            Some(epic) if self.looks.epic.as_ref().map(|(asked, _)| *asked) != Some(epic) => {
                self.looks.epic = Some((epic, None));
                commands.push(Command::Look(self.active, Look::Epic(epic)));
            }
            Some(_) => {}
            // Left, it is read again when Enter comes back to it.
            None => self.looks.epic = None,
        }
        // The graph is read again each time it is opened, and for each epic the cursor reaches.
        let drawing = (self.cursor.detail == DetailView::Graph)
            .then(|| panes::epics::selected(&state, &self.cursor).map(|epic| epic.number))
            .flatten();
        match drawing {
            Some(epic) if self.looks.graph.as_ref().map(|(asked, _)| *asked) != Some(epic) => {
                self.looks.graph = Some((epic, None));
                commands.push(Command::Look(self.active, Look::Graph(epic)));
            }
            Some(_) => {}
            None => self.looks.graph = None,
        }
        match panes::live::followed_ticket(&state, &self.cursor) {
            Some(ticket) => {
                let new = self.looks.ticket.as_ref().map(|(asked, _)| *asked) != Some(ticket);
                if new {
                    self.looks.ticket = Some((ticket, None));
                }
                if new || again {
                    commands.push(Command::Look(self.active, Look::Ticket(ticket)));
                }
            }
            None => self.looks.ticket = None,
        }
        commands
    }

    /// Tab `index`'s gatherer read what a key asked for: it is kept when it is still what the
    /// screen asks for.
    pub fn looked(&mut self, index: usize, looked: Looked) {
        if index != self.active {
            return;
        }
        match looked {
            Looked::Epic(read) => {
                if let Some((asked, kept)) = &mut self.looks.epic
                    && *asked == read.epic
                {
                    *kept = Some(read);
                }
            }
            Looked::Ticket(read) => {
                if let Some((asked, kept)) = &mut self.looks.ticket
                    && *asked == read.ticket
                {
                    *kept = Some(read);
                }
            }
            Looked::Graph(read) => {
                if let Some((asked, kept)) = &mut self.looks.graph
                    && *asked == read.epic
                {
                    *kept = Some(*read);
                }
            }
        }
    }

    /// What `key` answers while the status line asks.
    fn ask(&mut self, key: Key, ask: &Ask, now: Instant) -> Vec<Command> {
        match build::answer(ask, key) {
            Answered::Run(control) => {
                self.mode = Mode::Normal;
                self.say(Tone::Plain, control.doing_words(), now);
                vec![Command::Control(control)]
            }
            Answered::Ask(next) => {
                self.mode = Mode::Ask(next);
                Vec::new()
            }
            Answered::Cancelled(words) => {
                self.mode = Mode::Normal;
                self.say(Tone::Plain, words, now);
                Vec::new()
            }
            Answered::Quit => vec![Command::Quit],
            Answered::Waiting => Vec::new(),
        }
    }

    fn normal(&mut self, key: Key, now: Instant) -> Vec<Command> {
        match key {
            Key::Char('q') | Key::CtrlC => return vec![Command::Quit],
            Key::Tab => {
                self.set_focus(self.layout.next(self.focus, self.panes_area()));
                return Vec::new();
            }
            Key::CtrlW => {
                self.mode = Mode::Window;
                return Vec::new();
            }
            Key::Char('u') => return self.undo(now),
            Key::Char(':') => {
                self.mode = Mode::Palette(0);
                return Vec::new();
            }
            Key::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                return self.switch(index);
            }
            Key::Char('r') => {
                self.say(Tone::Plain, "reading the board again", now);
                return vec![Command::ReadNow(self.active)];
            }
            _ => {}
        }
        let state = self.shown();
        let context = self.context(state.as_ref(), self.focus);
        let effect = pane(self.focus).key(key, context, &mut self.cursor);
        if effect != Effect::Ignored {
            return self.apply(effect, now);
        }
        let Some(state) = state else {
            return Vec::new();
        };
        let effect = self.anywhere(key, &state);
        self.apply(effect, now)
    }

    /// What the keys that work from any pane do.
    fn anywhere(&mut self, key: Key, state: &ChairState) -> Effect {
        match key {
            Key::Char('a') => {
                let Some(at) = state.inbox.iter().position(|item| item.answerable()) else {
                    return Effect::Note(Tone::Plain, "no question waits on you".to_owned());
                };
                self.cursor.inbox = at;
                self.set_focus(PaneId::Inbox);
                panes::inbox::open(&state.inbox[at])
            }
            Key::Char('o') => panes::phase::first_due(state).map_or_else(
                || Effect::Note(Tone::Plain, "no release phase is due".to_owned()),
                Effect::RunPhase,
            ),
            // Enter on ready work no scheduler ticks, which the inbox leaves to the chair.
            Key::Enter
                if self.focus == PaneId::Inbox
                    && panes::inbox::selected(state, &self.cursor)
                        .is_some_and(|item| item.kind == NextKind::Unticked) =>
            {
                self.anywhere(Key::Char('b'), state)
            }
            Key::Char(control) if build::KEYS.contains(&control) => {
                match build::asked(control, state, self.ticket(state)) {
                    Some(Ok(ask)) => {
                        self.mode = Mode::Ask(ask);
                        Effect::Done
                    }
                    Some(Err(words)) => Effect::Note(Tone::Plain, words),
                    None => Effect::Done,
                }
            }
            Key::Char('s') => match panes::epics::selected(state, &self.cursor) {
                Some(epic) => Effect::Sign(epic.number),
                None => Effect::Note(Tone::Plain, "no epic is open".to_owned()),
            },
            Key::Char('d') => {
                self.cursor.detail = if self.cursor.detail == DetailView::Graph {
                    DetailView::Epic
                } else {
                    DetailView::Graph
                };
                self.cursor.before = false;
                self.cursor.scroll = 0;
                Effect::Done
            }
            Key::Char('v') if self.cursor.detail == DetailView::Graph => {
                self.cursor.before = !self.cursor.before;
                self.cursor.scroll = 0;
                Effect::Done
            }
            Key::Char('w') => match self.ticket(state) {
                Some(ticket) => {
                    self.cursor.detail = DetailView::Why(ticket);
                    self.cursor.scroll = 0;
                    Effect::Done
                }
                None => Effect::Note(Tone::Plain, "no ticket is selected".to_owned()),
            },
            Key::Char('l') => match self.ticket(state) {
                Some(ticket) => {
                    self.cursor.live = Some(ticket);
                    if panes::live::runs(state, ticket) {
                        Effect::Done
                    } else {
                        let word = panes::detail::why::square(state, ticket)
                            .map_or("not on an open epic", |square| square.state.word());
                        Effect::Note(
                            Tone::Plain,
                            format!(
                                "{ticket} is not running: it is {word}. Live shows its last run; \
                                 w says why."
                            ),
                        )
                    }
                }
                None => Effect::Note(Tone::Plain, "no ticket is selected".to_owned()),
            },
            _ => Effect::Done,
        }
    }

    /// The ticket `w` and `l` are about: the selected run in Now, else the selected square.
    fn ticket(&self, state: &ChairState) -> Option<TicketId> {
        if self.focus == PaneId::Now {
            return state
                .now
                .running
                .get(self.cursor.running)
                .and_then(|line| line.ticket);
        }
        panes::epics::selected_square(state, &self.cursor).map(|square| square.ticket)
    }

    fn apply(&mut self, effect: Effect, now: Instant) -> Vec<Command> {
        match effect {
            Effect::Ignored | Effect::Done => Vec::new(),
            Effect::Focus(pane) => {
                self.set_focus(pane);
                Vec::new()
            }
            Effect::Answer(question) => {
                self.open_answer(question, now);
                Vec::new()
            }
            Effect::Sign(epic) => {
                self.sign(epic, now);
                Vec::new()
            }
            Effect::Write(mut action) => {
                // A fix runs in the tab's own project, whatever its read said (R3), and only
                // when none runs.
                if let ChairAction::Fix(fix) = &mut action {
                    if let Some(running) = &self.fixing {
                        let words =
                            format!("{running} still runs: nothing more starts until it ends");
                        self.say(Tone::Plain, words, now);
                        return Vec::new();
                    }
                    fix.root = self.tab().root.clone();
                    self.fixing = Some(format!("zaf {}", fix.args().join(" ")));
                }
                self.say(Tone::Plain, format!("{}…", action.done_words()), now);
                vec![Command::Write(action)]
            }
            Effect::Note(tone, words) => {
                self.say(tone, words, now);
                Vec::new()
            }
            Effect::RunPhase(version) => self.run_phase(version, now),
        }
    }

    /// Opens the answer box on `question`, the first paragraph of Claude's proposal in it (R2).
    /// When Detail as laid out cannot show all of it, the box takes the whole screen until it
    /// closes.
    fn open_answer(&mut self, question: TicketId, now: Instant) {
        let Some(item) = self
            .shown()
            .and_then(|state| state.question(question).cloned())
        else {
            self.say(Tone::Plain, format!("{question} is no longer open"), now);
            return;
        };
        let text = item
            .question
            .and_then(|question| question.proposal)
            .map(|proposal| panes::detail::prefill(&proposal))
            .unwrap_or_default();
        let mut answer = AnswerBox::new(question, text);
        self.focus = PaneId::Detail;
        self.cursor.detail = DetailView::Answer;
        self.cursor.scroll = 0;
        if self.answer_view(&answer).hidden() > 0 {
            self.zoom_answer(&mut answer);
        }
        self.mode = Mode::Answer(answer);
    }

    /// `answer` as Detail shows it now: in Detail's place in the layout, none of it when Detail is
    /// not laid out or the terminal is too small to draw.
    pub fn answer_view(&self, answer: &AnswerBox) -> AnswerView {
        let rect = self
            .layout
            .panes(self.panes_area())
            .into_iter()
            .find(|(pane, _)| *pane == PaneId::Detail)
            .map(|(_, rect)| rect)
            .filter(|_| self.fits())
            .unwrap_or_default();
        AnswerView::of(answer, rect.width, rect.height.saturating_sub(1))
    }

    /// Gives `answer` the whole screen, keeping what the layout had maximised to give it back.
    fn zoom_answer(&mut self, answer: &mut AnswerBox) {
        if answer.zoomed.is_none() {
            answer.zoomed = Some(self.layout.maximised());
            self.layout.set_maximised(Some(PaneId::Detail));
        }
    }

    /// Puts the layout back as it was before `answer` took the whole screen, when it did.
    fn unzoom_answer(&mut self, answer: &mut AnswerBox) {
        if let Some(before) = answer.zoomed.take() {
            self.layout.set_maximised(before);
        }
    }

    fn answer(&mut self, key: Key, mut answer: AnswerBox, now: Instant) -> Vec<Command> {
        match key {
            Key::Char(character) => {
                // A terminal without bracketed paste types a paste as keys: what draws nothing is
                // dropped here as from a paste.
                answer.text.push_str(&printable(&character.to_string()));
                answer.back = 0;
            }
            Key::Backspace => {
                answer.text.pop();
                answer.back = 0;
            }
            Key::CtrlU => {
                answer.text.clear();
                answer.back = 0;
            }
            Key::Up => {
                let most = self.answer_view(&answer).hidden();
                answer.back = (answer.back + 1).min(most);
            }
            Key::Down => answer.back = answer.back.saturating_sub(1),
            Key::CtrlW if answer.zoomed.is_some() => {
                self.unzoom_answer(&mut answer);
                answer.back = 0;
            }
            Key::CtrlW => {
                self.zoom_answer(&mut answer);
                answer.back = 0;
            }
            Key::Esc => {
                self.unzoom_answer(&mut answer);
                self.mode = Mode::Normal;
                self.set_focus(PaneId::Inbox);
                self.say(Tone::Plain, "not ruled", now);
                return Vec::new();
            }
            Key::CtrlC => return vec![Command::Quit],
            Key::Enter => {
                let hidden = self.answer_view(&answer).hidden();
                if hidden == 0 {
                    return self.rule(answer, now);
                }
                let plural = if hidden == 1 { "line is" } else { "lines are" };
                let how = if answer.zoomed.is_some() || !self.fits() {
                    "shorten the ruling or make the terminal larger"
                } else {
                    "^w gives the box the whole screen"
                };
                self.say(
                    Tone::Failed,
                    format!("{hidden} {plural} out of view, so it is not ruled: {how}"),
                    now,
                );
            }
            Key::Tab | Key::Left | Key::Right => {}
        }
        self.mode = Mode::Answer(answer);
        Vec::new()
    }

    /// Queues the ruling in `answer`, to be written once its time to be taken back has passed.
    fn rule(&mut self, mut answer: AnswerBox, now: Instant) -> Vec<Command> {
        let root = self.tab().root.clone();
        match Ruling::new(root.clone(), answer.question, &answer.text) {
            Ok(ruling) => {
                self.pending.push(ChairAction::Rule(ruling), now);
                self.hidden.insert((root, answer.question));
                self.unzoom_answer(&mut answer);
                self.mode = Mode::Normal;
                self.set_focus(PaneId::Inbox);
                self.note = None;
                if let Some(state) = self.shown() {
                    self.cursor.clamp(&state);
                }
            }
            Err(refusal) => {
                self.say(Tone::Failed, refusal.to_string(), now);
                self.mode = Mode::Answer(answer);
            }
        }
        Vec::new()
    }

    /// Queues the signature on `epic`, to be written once its time to be taken back has passed;
    /// an epic that is not open, already signed or already being signed is said so.
    fn sign(&mut self, epic: TicketId, now: Instant) {
        let root = self.tab().root.clone();
        let Some(state) = self.shown() else {
            return;
        };
        let Some(view) = state.epic(epic) else {
            self.say(Tone::Plain, format!("{epic} is not an open epic"), now);
            return;
        };
        if self.looks.signs(&root, epic) {
            self.say(
                Tone::Plain,
                format!("epic {epic} is already being signed; u takes it back"),
                now,
            );
            return;
        }
        if view.stage != EpicStage::Unsigned {
            self.say(Tone::Plain, format!("epic {epic} is already signed"), now);
            return;
        }
        let others: std::collections::BTreeSet<TicketId> = self
            .looks
            .signing
            .iter()
            .filter(|(signing_root, _)| *signing_root == root)
            .map(|(_, epic)| *epic)
            .chain([epic])
            .collect();
        let last = state.unsigned_of(view.release.as_ref(), &others) == 0;
        self.pending.push(
            ChairAction::Sign(Signing {
                root: root.clone(),
                epic,
                version: view.release.clone(),
                last,
            }),
            now,
        );
        self.looks.signing.insert((root, epic));
        self.note = None;
    }

    /// Takes back the newest write that waits.
    fn undo(&mut self, now: Instant) -> Vec<Command> {
        match self.pending.undo() {
            Some(action) => {
                if let Some(question) = action.question() {
                    self.hidden.remove(&(action.root().to_owned(), question));
                }
                if let Some(epic) = action.signs() {
                    self.looks.signing.remove(&(action.root().to_owned(), epic));
                }
                self.say(Tone::Undone, action.undone_words(), now);
            }
            None => self.say(Tone::Plain, "nothing to undo", now),
        }
        Vec::new()
    }

    fn window(&mut self, key: Key, now: Instant) -> Vec<Command> {
        self.mode = Mode::Normal;
        let area = self.panes_area();
        let toward = |chair: &mut Self, direction| {
            if chair.layout.maximised().is_none()
                && let Some(pane) = chair.layout.toward(chair.focus, direction, area)
            {
                chair.set_focus(pane);
            }
        };
        let resize = |chair: &mut Self, axis, delta| {
            if let Err(refusal) = chair.layout.resize(chair.focus, axis, delta, area) {
                chair.say(Tone::Plain, refusal.words(), now);
            }
        };
        match key {
            Key::Char('h') | Key::Left => toward(self, Direction::Left),
            Key::Char('j') | Key::Down => toward(self, Direction::Down),
            Key::Char('k') | Key::Up => toward(self, Direction::Up),
            Key::Char('l') | Key::Right => toward(self, Direction::Right),
            Key::Char('w') | Key::CtrlW => {
                self.set_focus(self.layout.next(self.focus, area));
            }
            Key::Char('+') => resize(self, Axis::Rows, STEP),
            Key::Char('-') => resize(self, Axis::Rows, -STEP),
            Key::Char('>') => resize(self, Axis::Columns, STEP),
            Key::Char('<') => resize(self, Axis::Columns, -STEP),
            Key::Char('=') => self.layout.reset(),
            Key::Char('o') => self.layout.toggle_maximised(self.focus),
            Key::Char('c') => return self.leave_phase(now),
            Key::CtrlC => return vec![Command::Quit],
            _ => {}
        }
        Vec::new()
    }

    fn palette(&mut self, key: Key, selected: usize, now: Instant) -> Vec<Command> {
        let count = palette::len();
        match key {
            Key::Char('j') | Key::Down => self.mode = Mode::Palette((selected + 1) % count),
            Key::Char('k') | Key::Up => self.mode = Mode::Palette((selected + count - 1) % count),
            Key::Esc => self.mode = Mode::Normal,
            Key::CtrlC => return vec![Command::Quit],
            Key::Enter => {
                self.mode = Mode::Normal;
                return match palette::at(selected).map(|entry| entry.run) {
                    // Through `key`, so a phase session's question comes first: `q` asks.
                    Some(Run::Key(key)) => self.key(key, now),
                    Some(Run::Window(key)) => self.window(key, now),
                    Some(Run::In(pane, key)) => {
                        self.set_focus(pane);
                        self.normal(key, now)
                    }
                    Some(Run::NextTab) => self.switch((self.active + 1) % self.tabs.len().max(1)),
                    None => Vec::new(),
                };
            }
            _ => {}
        }
        Vec::new()
    }

    /// Shows tab `index`.
    pub fn switch(&mut self, index: usize) -> Vec<Command> {
        if index >= self.tabs.len() || index == self.active {
            return Vec::new();
        }
        let from = self.active;
        self.active = index;
        let detail = self.cursor.detail;
        self.cursor = Cursor::default();
        self.looks.epic = None;
        self.looks.ticket = None;
        self.set_focus(self.focus);
        if matches!(detail, DetailView::Item | DetailView::Problem) {
            self.cursor.detail = detail;
        }
        vec![Command::Shown { from, to: index }]
    }

    /// A click at `(x, y)`: a tab switches, a pane takes the focus and its row the selection.
    pub fn click(&mut self, x: u16, y: u16, now: Instant) -> Vec<Command> {
        let waiting = self.sign_waits();
        let commands = self.click_at(x, y, now);
        self.sign_shown(waiting, now);
        commands
    }

    fn click_at(&mut self, x: u16, y: u16, now: Instant) -> Vec<Command> {
        if !self.fits() || self.quiet(now) {
            return Vec::new();
        }
        match self.mode {
            Mode::Answer(_) => return Vec::new(),
            Mode::Palette(_) | Mode::Window | Mode::Ask(_) => {
                self.mode = Mode::Normal;
                return Vec::new();
            }
            Mode::Normal => {}
        }
        if y == 0 {
            let index = self
                .tab_spans()
                .iter()
                .position(|(start, end)| (*start..*end).contains(&x));
            return index.map_or_else(Vec::new, |index| self.switch(index));
        }
        let area = self.panes_area();
        let Some((pane_id, rect)) = self
            .layout
            .panes(area)
            .into_iter()
            .find(|(_, rect)| rect.contains((x, y).into()))
        else {
            return Vec::new();
        };
        if pane_id != self.focus {
            self.set_focus(pane_id);
        }
        let state = self.shown();
        let context = self.context(state.as_ref(), pane_id);
        let effect = pane(pane_id).click(y - rect.y, x - rect.x, context, &mut self.cursor);
        let mut commands = self.apply(effect, now);
        commands.extend(self.follow(false));
        commands
    }

    /// The wheel turned over `(x, y)`: the pane there takes the focus and moves a row.
    pub fn wheel(&mut self, x: u16, y: u16, down: bool, now: Instant) -> Vec<Command> {
        if !self.fits() || self.mode != Mode::Normal {
            return Vec::new();
        }
        let Some(pane_id) = self.layout.pane_at(self.panes_area(), x, y) else {
            return Vec::new();
        };
        if pane_id != self.focus {
            self.set_focus(pane_id);
        }
        let state = self.shown();
        let context = self.context(state.as_ref(), pane_id);
        let key = if down { Key::Down } else { Key::Up };
        let effect = pane(pane_id).key(key, context, &mut self.cursor);
        let mut commands = self.apply(effect, now);
        commands.extend(self.follow(false));
        commands
    }

    /// A frame passed at `now`: the spinners turn, what is due is written, an old message goes.
    pub fn tick(&mut self, now: Instant) -> Vec<Command> {
        self.frame = self.frame.wrapping_add(1);
        if self.note.as_ref().is_some_and(|note| note.until <= now) {
            self.note = None;
        }
        let mut commands = self.phase_tick(now);
        commands.extend(self.pending.take_due(now).into_iter().map(Command::Write));
        commands
    }

    /// Tab `index`'s gatherer said `event`.
    pub fn gathered(&mut self, index: usize, event: GatherEvent<ChairState>) -> Vec<Command> {
        let Some(tab) = self.tabs.get_mut(index) else {
            return Vec::new();
        };
        let state = match event {
            GatherEvent::Started(at) => {
                tab.reading = Some(at);
                return Vec::new();
            }
            GatherEvent::Gathered(state) => *state,
        };
        tab.reading = None;
        let previous = tab.state.replace(state);
        let root = tab.root.clone();
        let waiting: BTreeSet<TicketId> = self
            .pending
            .iter()
            .filter(|pending| pending.action.root() == root)
            .filter_map(|pending| pending.action.question())
            .collect();
        let tab = &self.tabs[index];
        let fresh = tab.state.as_ref();
        // A question stays hidden while its ruling waits or while the board still shows it; one
        // the board no longer shows needs hiding no more.
        self.hidden.retain(|(hidden_root, question)| {
            hidden_root != &root
                || waiting.contains(question)
                || fresh.is_some_and(|state| state.question(*question).is_some())
        });
        // An epic stays signing while its signature waits, or while the board still shows it
        // unsigned after the signature was written.
        let signing: BTreeSet<TicketId> = self
            .pending
            .iter()
            .filter(|pending| pending.action.root() == root)
            .filter_map(|pending| pending.action.signs())
            .collect();
        self.looks.signing.retain(|(signing_root, epic)| {
            signing_root != &root
                || signing.contains(epic)
                || fresh.is_some_and(|state| {
                    state
                        .epic(*epic)
                        .is_some_and(|view| view.stage == EpicStage::Unsigned)
                })
        });
        let pings = fresh.map_or_else(Vec::new, |state| pings::pings(previous.as_ref(), state));
        let mut commands: Vec<Command> = pings
            .into_iter()
            .map(|ping| match ping {
                Ping::Bell => Command::Bell,
                Ping::Notify(notice) => Command::Notify(notice),
            })
            .collect();
        if index == self.active
            && let Some(state) = self.shown()
        {
            self.cursor.clamp(&state);
            commands.extend(self.follow(true));
        }
        self.follow_phase(index);
        commands
    }

    /// `control` ran with `outcome`: it is said, and its project is read again.
    pub fn controlled(
        &mut self,
        control: &Control,
        outcome: &Result<String, String>,
        now: Instant,
    ) -> Vec<Command> {
        match outcome {
            Ok(said) => self.say(Tone::Done, said.clone(), now),
            Err(why) => self.say(Tone::Failed, why.clone(), now),
        }
        self.tabs
            .iter()
            .position(|tab| tab.root == control.root())
            .map(Command::ReadNow)
            .into_iter()
            .collect()
    }

    /// `action` was written with `outcome`: it is said, a refused ruling's question comes back,
    /// and its project is read again.
    pub fn written(
        &mut self,
        action: &ChairAction,
        outcome: &Outcome,
        now: Instant,
    ) -> Vec<Command> {
        if matches!(action, ChairAction::Fix(_)) {
            self.fixing = None;
        }
        if !outcome.written()
            && let Some(question) = outcome.question()
        {
            self.hidden.remove(&(action.root().to_owned(), question));
        }
        if !outcome.written()
            && let Some(epic) = action.signs()
        {
            self.looks.signing.remove(&(action.root().to_owned(), epic));
        }
        let tone = if outcome.written() {
            Tone::Done
        } else {
            Tone::Failed
        };
        self.say(tone, outcome.line(), now);
        self.tabs
            .iter()
            .position(|tab| tab.root == action.root())
            .map(Command::ReadNow)
            .into_iter()
            .collect()
    }
}

/// A paste as the answer box takes it: each line ending a newline, whatever the terminal sent
/// (`\r\n`, `\r`), a run of blank lines one, and no other control character.
fn pasted(text: &str) -> String {
    printable(&panes::detail::collapse_blank_lines(text))
}

mod phase;
#[cfg(test)]
mod tests;
