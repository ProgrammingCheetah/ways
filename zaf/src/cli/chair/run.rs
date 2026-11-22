//! The chair's loop: draw, read the keys, mouse, pastes and resizes for up to 100 ms, take what
//! the gatherers and the writer sent, let time pass, carry out what the chair asks. Writes run on
//! a thread of their own, so a slow board never holds the screen. While the phase pane has the
//! focus, keys go to its session as bytes, `Ctrl-w` alone to the chair; a paste is text, never
//! keys ([`Chair::paste`]). Closing writes what waits
//! first, hangs up a phase session still under way, hands the terminal back and says the build
//! keeps running.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{self, Event, KeyEventKind, MouseButton, MouseEventKind};

use super::build::{self, Control, Controls};
use super::draw::draw;
use super::keys::{Key, key_of};
use super::model::{Chair, Command};
use super::panes::Tone;
use super::panes::phase::{Routed, route};
use super::pings::Desktop;
use super::session::{PhasePorts, Phases};
use super::source::{self, Update, Writes};
use super::terminal::{self, Session};
use super::theme::Theme;
use crate::app::chair::gatherer::{Cadence, Gatherer};
use crate::app::chair::{ChairAction, ChairActions, Outcome, commit_guarded};
use crate::app::projects::ProjectEntry;
use crate::cli::screen::Screen;
use crate::domain::printable;
use crate::domain::screen::Meaning;

/// How long the loop waits for a key before it lets time pass.
const POLL: Duration = Duration::from_millis(100);

/// Opens the chair over `entries`, `active` on screen, writing as the zaf home `home`; stderr goes
/// to `log` while it runs. The exit code: 0 once closed, 2 when the terminal could not be taken,
/// 128 and the signal when SIGTERM, SIGINT or SIGHUP closed it (the terminal handed back first).
pub fn run(
    entries: Vec<ProjectEntry>,
    active: usize,
    home: Option<PathBuf>,
    log: &std::path::Path,
) -> u8 {
    let theme = Theme::from_environment();
    let mut session = match Session::enter(log) {
        Ok(session) => session,
        Err(error) => {
            crate::eprintln_zaf!("zaf: the chair could not take the terminal: {error}");
            return 2;
        }
    };
    let size = session
        .screen
        .size()
        .map_or((80, 24), |size| (size.width, size.height));
    let mut chair = Chair::new(source::tabs(&entries), active, theme, size);
    let (updates, heard) = mpsc::channel::<Update>();
    let gatherers: Vec<Option<Gatherer>> = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let cadence = if index == chair.active {
                Cadence::SHOWN
            } else {
                Cadence::BEHIND
            };
            source::gatherer(index, entry, cadence, updates.clone()).ok()
        })
        .collect();
    let controls: Arc<dyn Controls> = Arc::new(build::machine::Machine::new(home.clone()));
    let desktop = Desktop::from_environment();
    let controlled = updates.clone();
    let mut phases = Phases::new(home.clone(), PhasePorts::real());
    let writes: Arc<dyn ChairActions> = Arc::new(Writes::new(home));
    let roots: Vec<PathBuf> = chair.tabs.iter().map(|tab| tab.root.clone()).collect();
    let looks = updates.clone();
    let fixed = updates.clone();
    let (to_write, writer) = start_writer(Arc::clone(&writes), updates);

    let mut last_tick = Instant::now();
    let mut signalled = None;
    'chair: loop {
        // SIGTERM, SIGINT or SIGHUP: closed as `q` closes, the terminal handed back first.
        if let Some(signal) = terminal::signalled() {
            signalled = Some(signal);
            break 'chair;
        }
        let _ = session
            .screen
            .draw(|frame| draw(&chair, frame, Instant::now(), SystemTime::now()));
        let mut commands = Vec::new();
        if event::poll(POLL).unwrap_or(false) {
            while let Ok(event) = event::read() {
                commands.extend(on_event(&mut chair, &event));
                if !event::poll(Duration::ZERO).unwrap_or(false) {
                    break;
                }
            }
        }
        while let Ok(update) = heard.try_recv() {
            commands.extend(on_update(&mut chair, update));
        }
        for death in terminal::deaths() {
            chair.say(
                Tone::Failed,
                format!("{}; the chair's log has more", printable(&death)),
                Instant::now(),
            );
        }
        commands.extend(phases.poll(&mut chair, Instant::now()));
        let now = Instant::now();
        if now.duration_since(last_tick) >= POLL {
            last_tick = now;
            commands.extend(chair.tick(now));
        }
        // What a command leads to is carried out in the same turn: a write that could not be
        // handed on is said at once.
        let mut at = 0;
        while let Some(command) = commands.get(at).cloned() {
            at += 1;
            match command {
                Command::Quit => break 'chair,
                Command::Write(action @ ChairAction::Fix(_)) => {
                    start_fix(Arc::clone(&writes), action, fixed.clone());
                }
                Command::Write(action) => {
                    commands.extend(hand_on(&mut chair, &to_write, action));
                }
                Command::ReadNow(index) => {
                    if let Some(Some(gatherer)) = gatherers.get(index) {
                        gatherer.now();
                    }
                }
                Command::Look(index, look) => {
                    if let (Some(Some(gatherer)), Some(root)) =
                        (gatherers.get(index), roots.get(index))
                    {
                        source::look(gatherer, index, root.clone(), look, looks.clone());
                    }
                }
                Command::Shown { from, to } => {
                    if let Some(Some(gatherer)) = gatherers.get(from) {
                        gatherer.set_cadence(Cadence::BEHIND);
                    }
                    if let Some(Some(gatherer)) = gatherers.get(to) {
                        gatherer.set_cadence(Cadence::SHOWN);
                    }
                }
                Command::Bell => {
                    let _ = io::stdout().write_all(b"\x07");
                }
                Command::Notify(notice) => desktop.post(&notice),
                Command::Control(control) => {
                    start_control(Arc::clone(&controls), control, controlled.clone());
                }
                Command::Phase(command) => phases.carry(command),
            }
        }
    }
    phases.end();

    let left = chair.pending.drain();
    if !left.is_empty() {
        let words: Vec<String> = left.iter().map(ChairAction::done_words).collect();
        chair.say(
            Tone::Plain,
            format!("writing before closing: {}", words.join(", ")),
            Instant::now(),
        );
        let _ = session
            .screen
            .draw(|frame| draw(&chair, frame, Instant::now(), SystemTime::now()));
    }
    let said = close(to_write, writer, &heard, &left, writes.as_ref());
    drop(gatherers);
    drop(session);
    let screen = Screen::detect();
    // Written, never printed: after a hang-up there is no terminal, and a print would panic.
    let mut out = io::stdout();
    for line in said {
        let _ = writeln!(out, "{} {line}", screen.paint(Meaning::Program, "zaf:"));
    }
    let zaf = screen.paint(Meaning::Program, "zaf");
    let _ = writeln!(
        out,
        "{zaf} closed. The build keeps running; {zaf} opens this again."
    );
    // As a shell reports a process a signal ended: 128 and the signal.
    signalled.map_or(0, |signal| u8::try_from(128 + signal).unwrap_or(u8::MAX))
}

/// The writer's thread: each action it is sent is written and its outcome sent back. A write that
/// panics is said as failed, and the thread goes on with the next.
fn start_writer(
    writes: Arc<dyn ChairActions>,
    updates: Sender<Update>,
) -> (Sender<ChairAction>, Option<std::thread::JoinHandle<()>>) {
    let (to_write, asked) = mpsc::channel::<ChairAction>();
    let writer = std::thread::Builder::new()
        .name("chair-write".to_owned())
        .spawn(move || {
            for action in asked {
                let outcome = commit_guarded(writes.as_ref(), &action);
                if updates.send(Update::Written(action, outcome)).is_err() {
                    return;
                }
            }
        })
        .ok();
    (to_write, writer)
}

/// Hands `action` to the writer; when the writer is gone it is said as not written, so the chair
/// never claims a ruling or a signature it could not hand on (its question comes back).
fn hand_on(chair: &mut Chair, to_write: &Sender<ChairAction>, action: ChairAction) -> Vec<Command> {
    let Err(mpsc::SendError(action)) = to_write.send(action) else {
        return Vec::new();
    };
    let outcome = Outcome::failed(
        &action,
        "the chair's writer has stopped: nothing was written; the chair's log says why".to_owned(),
    );
    chair.written(&action, &outcome, Instant::now())
}

/// Runs the fix `action` on a thread of its own, so a fix that hangs holds neither a ruling, a
/// signature nor the chair's close; the fix itself is killed past its limit
/// ([`source::FIX_LIMIT`]). Nothing waits for it when the chair closes. Whatever happens, its
/// outcome comes back, so the chair knows no fix runs any more (`Chair::fixing`).
fn start_fix(writes: Arc<dyn ChairActions>, action: ChairAction, updates: Sender<Update>) {
    let (on_thread, sent) = (action.clone(), updates.clone());
    let started = std::thread::Builder::new()
        .name("chair-fix".to_owned())
        .spawn(move || {
            let outcome = commit_guarded(writes.as_ref(), &on_thread);
            let _ = sent.send(Update::Written(on_thread, outcome));
        });
    if let Err(error) = started {
        let outcome = Outcome::failed(&action, format!("it did not start: {error}"));
        let _ = updates.send(Update::Written(action, outcome));
    }
}

/// Runs `control` through `controls` on a thread of its own, since a stop waits for an instance
/// to end and a start for the scheduler to settle, and sends back what it did.
fn start_control(controls: Arc<dyn Controls>, control: Control, updates: Sender<Update>) {
    let _ = std::thread::Builder::new()
        .name("chair-control".to_owned())
        .spawn(move || {
            let outcome = build::carry_out(controls.as_ref(), &control);
            let _ = updates.send(Update::Controlled(control, outcome));
        });
}

/// Lets the writes under way end, then writes what still waited: what each did, in a line.
fn close(
    to_write: Sender<ChairAction>,
    writer: Option<std::thread::JoinHandle<()>>,
    heard: &Receiver<Update>,
    left: &[ChairAction],
    writes: &dyn ChairActions,
) -> Vec<String> {
    drop(to_write);
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let mut said: Vec<String> = heard
        .try_iter()
        .filter_map(|update| match update {
            Update::Written(_, outcome) => Some(outcome.line()),
            Update::Gathered(..) | Update::Controlled(..) | Update::Looked(..) => None,
        })
        .collect();
    said.extend(left.iter().map(|action| {
        format!(
            "{} (written as zaf closed)",
            commit_guarded(writes, action).line()
        )
    }));
    said
}

/// What a terminal event does to `chair`.
fn on_event(chair: &mut Chair, event: &Event) -> Vec<Command> {
    let now = Instant::now();
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release && chair.typing_to_claude() => {
            match route(key, chair.application_cursor()) {
                Routed::Window => chair.key(Key::CtrlW, now),
                Routed::Bytes(bytes) => chair.to_claude(bytes),
                Routed::Nothing => Vec::new(),
            }
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            key_of(key).map_or_else(Vec::new, |key| chair.key(key, now))
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => chair.click(mouse.column, mouse.row, now),
            MouseEventKind::ScrollDown => chair.wheel(mouse.column, mouse.row, true, now),
            MouseEventKind::ScrollUp => chair.wheel(mouse.column, mouse.row, false, now),
            _ => Vec::new(),
        },
        Event::Resize(columns, rows) => {
            chair.resize(*columns, *rows);
            Vec::new()
        }
        Event::Paste(text) => chair.paste(text, now),
        _ => Vec::new(),
    }
}

/// What a thread's update does to `chair`.
fn on_update(chair: &mut Chair, update: Update) -> Vec<Command> {
    match update {
        Update::Gathered(index, event) => chair.gathered(index, event),
        Update::Looked(index, looked) => {
            chair.looked(index, looked);
            Vec::new()
        }
        Update::Written(action, outcome) => chair.written(&action, &outcome, Instant::now()),
        Update::Controlled(control, outcome) => {
            chair.controlled(&control, &outcome, Instant::now())
        }
    }
}

#[cfg(test)]
mod tests;
