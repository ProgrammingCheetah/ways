//! The chair's moves on a release phase's session (F8, D10): `o` opens the phase pane and asks the
//! runtime to start the session; what the runtime hears back moves it on; a few keys answer the
//! questions it asks; time passing ends a session that was told `/exit`. Pure, like the rest of
//! the chair: nothing here touches the pseudo-terminal.

use std::path::Path;
use std::time::Instant;

use tui_term::vt100;

use super::{Chair, Command, Mode};
use crate::app::chair::ReleaseVersion;
use crate::app::hub::next::NextKind;
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::panes::phase::{
    Ask, CLEAR_INPUT, Ending, PhaseCommand, PhaseEvent, QUIET_FOR, Question, Session, Stage,
    bytes_of, paste_bytes,
};
use crate::cli::chair::panes::{Hint, Tone, hint};

/// The rows and columns a phase pane starts with when the layout cannot say.
const FALLBACK_SIZE: (u16, u16) = (20, 80);

impl Chair {
    /// Whether keys go straight to Claude: the phase pane has the focus, no other mode or question
    /// is open, and the session's action may still run.
    pub fn typing_to_claude(&self) -> bool {
        self.focus == PaneId::Phase
            && self.mode == Mode::Normal
            && self
                .phase
                .session
                .as_ref()
                .is_some_and(|session| session.live() && session.ask.is_none())
    }

    /// Whether no key counts at `now`: the session's pane closed on its own, and no
    /// [`QUIET_FOR`] has passed since then without a key (R1).
    pub fn quiet(&self, now: Instant) -> bool {
        self.phase.quiet_until.is_some_and(|until| now < until)
    }

    /// A key or a paste came at `now`: whether it is dropped as [`quiet`](Self::quiet). One that
    /// is dropped starts the quiet second again, so the sign question takes keys only after a
    /// second with none: a sentence typed to Claude never runs on into it (Q1). The status line
    /// keeps saying why meanwhile.
    pub(super) fn hush(&mut self, now: Instant) -> bool {
        if !self.quiet(now) {
            return false;
        }
        let until = now + QUIET_FOR;
        self.phase.quiet_until = Some(until);
        if let Some(note) = self.note.as_mut() {
            note.until = note.until.max(until);
        }
        true
    }

    /// What `bytes` typed to Claude do: they are typed into the session, unless it is being
    /// ended, when they are dropped, since they would land after `/exit` (R1).
    pub fn to_claude(&self, bytes: Vec<u8>) -> Vec<Command> {
        let ending = self
            .phase
            .session
            .as_ref()
            .is_some_and(|session| matches!(session.stage, Stage::Ending(_)));
        if ending {
            return Vec::new();
        }
        vec![Command::Phase(PhaseCommand::Type(bytes))]
    }

    /// Whether the session's terminal asked for the cursor keys' application form.
    pub fn application_cursor(&self) -> bool {
        self.phase
            .session
            .as_ref()
            .and_then(|session| session.screen.as_ref())
            .is_some_and(vt100::Screen::application_cursor)
    }

    /// The phase pane's rows (below its title) and columns, while it is laid out: the size of
    /// the session's terminal.
    pub fn phase_size(&self) -> Option<(u16, u16)> {
        self.layout
            .panes(self.panes_area())
            .into_iter()
            .find(|(pane, _)| *pane == PaneId::Phase)
            .map(|(_, rect)| (rect.height.saturating_sub(1).max(1), rect.width.max(1)))
    }

    /// The question about the session that is asked now. The sign question waits while the answer
    /// box, the build question or another mode is open: those take the keys, and it is asked once
    /// they close, so the Enter that ends a ruling or the `y` that answers the build question
    /// never signs the document.
    fn open_ask(&self) -> Option<&Ask> {
        let ask = self.phase.session.as_ref()?.ask.as_ref()?;
        (self.mode == Mode::Normal || !matches!(ask, Ask::Sign(_))).then_some(ask)
    }

    /// Whether a sign question is held back now by a mode that is open.
    pub(super) fn sign_waits(&self) -> bool {
        self.mode != Mode::Normal
            && self
                .phase
                .session
                .as_ref()
                .is_some_and(|session| matches!(session.ask, Some(Ask::Sign(_))))
    }

    /// A key or click was handled at `now`; `waiting` is what [`sign_waits`](Self::sign_waits)
    /// said before it. A sign question that was held back and is asked now takes no key for
    /// [`QUIET_FOR`], so the Enter that ended a ruling, a second Enter after it, or the `y` of
    /// the build question never signs the document.
    pub(super) fn sign_shown(&mut self, waiting: bool, now: Instant) {
        if waiting
            && self
                .open_ask()
                .is_some_and(|ask| matches!(ask, Ask::Sign(_)))
        {
            self.phase.quiet_until = Some(now + QUIET_FOR);
        }
    }

    /// The question about the session on the status line, while one is asked.
    pub fn phase_question(&self) -> Option<Question> {
        let session = self.phase.session.as_ref()?;
        self.open_ask()
            .map(|ask| ask.question(session.phase_word()))
    }

    /// The keys the key bar shows while a question about the session is asked.
    pub(super) fn phase_hints(&self) -> Option<Vec<Hint>> {
        Some(match self.open_ask()? {
            Ask::Sign(_) => vec![hint("Enter/y", "sign"), hint("n", "leave it unsigned")],
            Ask::Leave => vec![hint("y", "end the session"), hint("Enter/n", "go on")],
            Ask::Quit => vec![hint("y", "quit"), hint("Enter/n", "stay")],
        })
    }

    /// Runs `version`'s release phase due in the project on screen: the phase pane takes the place
    /// of Detail and Live, gets the focus, and the runtime is asked to start the session. One
    /// session at a time: while one is there, its pane takes the focus instead.
    pub(super) fn run_phase(&mut self, version: ReleaseVersion, now: Instant) -> Vec<Command> {
        if let Some(session) = &self.phase.session {
            let words = format!(
                "the {} of {} is still under way",
                session.phase_word(),
                session.version
            );
            if self.layout.phase_open() {
                self.set_focus(PaneId::Phase);
            }
            self.say(Tone::Plain, words, now);
            return Vec::new();
        }
        let root = self.tab().root.clone();
        self.layout.open_phase();
        self.set_focus(PaneId::Phase);
        self.phase.follow = false;
        self.phase.session = Some(Session::new(root.clone(), version.clone()));
        let (rows, columns) = self.phase_size().unwrap_or(FALLBACK_SIZE);
        vec![Command::Phase(PhaseCommand::Start {
            root,
            version,
            rows,
            columns,
        })]
    }

    /// What `key` does about the session before anything else sees it, none when it is not the
    /// session's: an open question takes every key (the sign question only while no other mode is open); while keys go to Claude, all but `Ctrl-w` are
    /// typed into the session; `q` (and `Ctrl-C` anywhere) asks first while the session runs.
    pub(super) fn phase_key(&mut self, key: Key, now: Instant) -> Option<Vec<Command>> {
        let session = self.phase.session.as_ref()?;
        if let Some(ask) = self.open_ask().cloned() {
            return Some(self.answer_ask(&ask, key, now));
        }
        if self.typing_to_claude() {
            return (key != Key::CtrlW).then(|| self.to_claude(bytes_of(key)));
        }
        let quit = key == Key::CtrlC || (key == Key::Char('q') && self.mode == Mode::Normal);
        if session.live() && quit {
            self.ask_about_session(Ask::Quit);
            return Some(Vec::new());
        }
        None
    }

    /// What a paste of `text` does about the session before anything else sees it, none when it
    /// is not the session's: an open question takes no paste (a newline in it would answer it),
    /// and while keys go to Claude the paste is typed into the session as a paste.
    pub(super) fn phase_paste(&mut self, text: &str, now: Instant) -> Option<Vec<Command>> {
        let session = self.phase.session.as_ref()?;
        if self.open_ask().is_some() {
            self.say(
                Tone::Plain,
                "a paste answers no question: nothing was done",
                now,
            );
            return Some(Vec::new());
        }
        if self.typing_to_claude() {
            let bracketed = session
                .screen
                .as_ref()
                .is_some_and(vt100::Screen::bracketed_paste);
            return Some(self.to_claude(paste_bytes(text, bracketed)));
        }
        None
    }

    fn ask_about_session(&mut self, ask: Ask) {
        if let Some(session) = self.phase.session.as_mut() {
            session.ask = Some(ask);
        }
    }

    /// The answer `key` gives to `ask`: `y` yes, `n` and Escape no, Enter the default (yes for
    /// the signature, no for the others); any other key leaves the question open. `Ctrl-C` on
    /// the signature quits, which leaves the document unsigned.
    fn answer_ask(&mut self, ask: &Ask, key: Key, now: Instant) -> Vec<Command> {
        let signing = matches!(ask, Ask::Sign(_));
        let yes = match key {
            Key::Char('y' | 'Y') => true,
            Key::Enter => signing,
            Key::Char('n' | 'N') | Key::Esc => false,
            Key::CtrlC if signing => return vec![Command::Quit],
            _ => return Vec::new(),
        };
        if let Some(session) = self.phase.session.as_mut() {
            session.ask = None;
        }
        match (ask, yes) {
            (Ask::Sign(_), _) => {
                let line = if yes { "y\n" } else { "n\n" };
                vec![Command::Phase(PhaseCommand::Answer(line.to_owned()))]
            }
            (Ask::Leave, true) => self.end_session(now, true),
            (Ask::Leave, false) => {
                self.say(Tone::Plain, "the session goes on", now);
                Vec::new()
            }
            (Ask::Quit, true) => vec![Command::Quit],
            (Ask::Quit, false) => {
                self.say(Tone::Plain, "zaf stays open", now);
                Vec::new()
            }
        }
    }

    /// `Ctrl-w c`, or the palette's "Leave the phase pane", from any pane: a ready session is
    /// ended at once, one that is not is asked about first. The chair's other panes never close.
    pub(super) fn leave_phase(&mut self, now: Instant) -> Vec<Command> {
        let Some(session) = self.phase.session.as_ref() else {
            self.say(Tone::Plain, "no Claude session is open", now);
            return Vec::new();
        };
        match session.stage {
            Stage::Ending(_) | Stage::Closed => {
                self.say(Tone::Plain, "the session is ending", now);
                Vec::new()
            }
            Stage::Starting | Stage::Running if session.ready => self.end_session(now, true),
            Stage::Starting | Stage::Running => {
                self.ask_about_session(Ask::Leave);
                Vec::new()
            }
        }
    }

    /// Clears Claude's input, so nothing typed ahead goes with `/exit` as a message (N6); the line
    /// cleared again, `/exit`, Enter, and a hang-up if it is needed, follow with time.
    /// `by_engineer` when the Engineer chose to end it, so its pane closes as asked.
    fn end_session(&mut self, now: Instant, by_engineer: bool) -> Vec<Command> {
        let Some(session) = self.phase.session.as_mut() else {
            return Vec::new();
        };
        session.stage = Stage::Ending(Ending::at(now));
        session.left |= by_engineer;
        vec![Command::Phase(PhaseCommand::Type(CLEAR_INPUT.to_vec()))]
    }

    /// Time passed to `now`: a session being ended gets its Enter, its hang-up and its kill, each
    /// when it is due.
    pub(super) fn phase_tick(&mut self, now: Instant) -> Vec<Command> {
        let Some(session) = self.phase.session.as_mut() else {
            return Vec::new();
        };
        let Stage::Ending(ending) = session.stage else {
            return Vec::new();
        };
        let (ending, due) = ending.step(now);
        session.stage = Stage::Ending(ending);
        due.into_iter().map(Command::Phase).collect()
    }

    /// The session's terminal shows `screen` now.
    pub fn phase_screen(&mut self, screen: vt100::Screen) {
        if let Some(session) = self.phase.session.as_mut() {
            session.screen = Some(screen);
        }
    }

    /// The runtime said `event` about the session.
    pub fn phase_event(&mut self, event: PhaseEvent, now: Instant) -> Vec<Command> {
        let Some(session) = self.phase.session.as_mut() else {
            return Vec::new();
        };
        match event {
            PhaseEvent::Started(phase) => {
                session.phase = Some(phase);
                if session.stage == Stage::Starting {
                    session.stage = Stage::Running;
                }
                Vec::new()
            }
            PhaseEvent::Ready => {
                if session.ready {
                    return Vec::new();
                }
                session.ready = true;
                let ending = matches!(session.stage, Stage::Ending(_) | Stage::Closed);
                // The answer to a leave question no longer matters: the session is done.
                if session.ask == Some(Ask::Leave) {
                    session.ask = None;
                }
                self.say(Tone::Done, "Ready for Exit.", now);
                if ending {
                    Vec::new()
                } else {
                    self.end_session(now, false)
                }
            }
            PhaseEvent::Refused(words) => {
                self.phase.session = None;
                self.close_on_its_own(now);
                self.say(Tone::Failed, words, now);
                Vec::new()
            }
            PhaseEvent::Exited { ready, .. } => {
                session.ready |= ready;
                session.stage = Stage::Closed;
                if matches!(session.ask, Some(Ask::Leave | Ask::Quit)) {
                    session.ask = None;
                }
                if session.left {
                    self.close_phase_pane();
                } else {
                    self.close_on_its_own(now);
                }
                Vec::new()
            }
            PhaseEvent::Asked(prompt) => {
                let path = signed_path(&prompt, &session.root);
                session.ask = Some(Ask::Sign(path));
                Vec::new()
            }
            PhaseEvent::Done { tone, words, next } => {
                let root = session.root.clone();
                self.phase.session = None;
                self.close_phase_pane();
                let words = match &next {
                    Some(next) => format!("{words} · the {next} is due next: o runs it"),
                    None => words,
                };
                self.phase.follow = next.is_some();
                self.say(tone, words, now);
                self.tabs
                    .iter()
                    .position(|tab| tab.root == root)
                    .map(Command::ReadNow)
                    .into_iter()
                    .collect()
            }
        }
    }

    /// The pane closes though the Engineer did not ask: for [`QUIET_FOR`] no key counts and the
    /// status line says the session ended, so the end of a sentence typed to Claude is no command
    /// of the chair's and no answer to its sign question (R1).
    fn close_on_its_own(&mut self, now: Instant) {
        if self.layout.phase_open() {
            self.phase.quiet_until = Some(now + QUIET_FOR);
            self.say(Tone::Plain, "session ended", now);
        }
        self.close_phase_pane();
    }

    /// The phase pane gives Detail and Live their place back, the focus going to the inbox.
    fn close_phase_pane(&mut self) {
        self.layout.close_phase();
        if self.focus == PaneId::Phase {
            self.set_focus(PaneId::Inbox);
        }
    }

    /// After tab `index` was read: once a signed document leads on to the phase due next, that
    /// phase's inbox item is selected, the first read that shows it.
    pub(super) fn follow_phase(&mut self, index: usize) {
        if !self.phase.follow || index != self.active {
            return;
        }
        let Some(at) = self.shown().and_then(|state| {
            state
                .inbox
                .iter()
                .position(|item| item.kind == NextKind::Phase)
        }) else {
            return;
        };
        self.phase.follow = false;
        self.cursor.inbox = at;
        self.set_focus(PaneId::Inbox);
    }
}

/// The document `complete_phase`'s prompt (`Sign <path>? [Y/n] `) names, from `root` when it is
/// inside it.
fn signed_path(prompt: &str, root: &Path) -> String {
    let path = prompt
        .trim_end()
        .strip_prefix("Sign ")
        .and_then(|rest| rest.strip_suffix("? [Y/n]"))
        .unwrap_or(prompt)
        .trim();
    Path::new(path)
        .strip_prefix(root)
        .map_or_else(|_| path.to_owned(), |inside| inside.display().to_string())
}

#[cfg(test)]
mod tests;
