//! The phase pane's runtime (F8, D10): the pseudo-terminal, the threads, and every call the chair
//! makes into begin, the release folder and the runner, in this one module (D15).
//!
//! `Start` opens a pseudo-terminal with portable-pty, of the phase pane's size, and starts two
//! threads. One reads what the session writes and hands it on; the drawing feeds it to a vt100
//! parser. The other does what `zaf begin` does, through its app path: works out the phase due
//! (`begin::begin`), runs that phase's action on the pseudo-terminal's side (`begin::run_phase`,
//! the runner's `OnPty` plan, `ZAF_READY` set), and, once the ready file or the action's exit 0
//! leads on (`begin::leads_to_sign`), checks and signs the document through
//! `begin::complete_phase`, whose `[Y/n]` question the chair asks on its status line. A signed
//! objectives leads on: the phase due next is said. The loop polls [`Phases`] for what was heard
//! and hands it to the chair, which stays pure.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use portable_pty::{MasterPty, PtySize, native_pty_system};
use tui_term::vt100;

use super::model::{Chair, Command};
use super::panes::Tone;
use super::panes::phase::{PhaseCommand, PhaseEvent};
use crate::adapters::process;
use crate::app::actions::{PhaseEnv, PtySide, RunOutcome, Terminal};
use crate::app::begin::{self, BeginOutcome, BeginRequest, Completion, PhaseRun, phase_word};
use crate::app::chair::ReleaseVersion;
use crate::app::epic::SignGuard;
use crate::app::release::ReleaseFolder;
use crate::cli::do_action::run_named_phase_in;
use crate::composition::Composition;
use crate::ports::Board;

/// How much the reader takes from the pseudo-terminal at a time.
const READ_CHUNK: usize = 16 * 1024;

/// Makes the board of a project.
pub type BoardOf = dyn Fn(&Composition) -> Result<Box<dyn Board>, String> + Send + Sync;

/// Runs a phase's action, named, in the project at a root, on a terminal, with a phase's
/// environment.
pub type RunOf = dyn Fn(&Path, &str, Terminal, PhaseEnv) -> RunOutcome + Send + Sync;

/// How a session reaches the board and the runner: the composition's board and `zaf begin`'s run
/// for real, fakes in a test.
#[derive(Clone)]
pub struct PhasePorts {
    /// The project's board.
    pub board: Arc<BoardOf>,
    /// The phase's action run.
    pub run: Arc<RunOf>,
}

impl PhasePorts {
    /// The ports `zaf begin` uses: the manifest's board and the runner from the project's root.
    pub fn real() -> Self {
        Self {
            board: Arc::new(|composition: &Composition| {
                composition.board().map_err(|error| error.to_string())
            }),
            run: Arc::new(|root: &Path, name: &str, terminal, env| {
                run_named_phase_in(root, name, terminal, env)
            }),
        }
    }
}

/// What the session's threads say.
enum Heard {
    /// The session wrote this.
    Output(Vec<u8>),
    /// The phase due is `phase`, its action started, and `ready` is its ready file.
    Started { phase: String, ready: PathBuf },
    /// This, for the chair.
    Phase(PhaseEvent),
}

/// The chair's phase sessions: none, or the one running.
pub struct Phases {
    home: Option<PathBuf>,
    ports: PhasePorts,
    running: Option<Running>,
    queued: Vec<PhaseEvent>,
}

/// A session under way.
struct Running {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    parser: vt100::Parser,
    heard: Receiver<Heard>,
    answers: Sender<String>,
    ready: Option<PathBuf>,
    told_ready: bool,
    size: (u16, u16),
    /// The phase's own thread, which a closing chair waits for a while ([`THREAD_WAIT`]).
    thread: Option<JoinHandle<()>>,
    /// Whether the action has ended: then there is no one left to hang up.
    exited: bool,
}

/// How long a closing chair waits for a session it hung up to end before it kills it.
pub const HANG_UP_WAIT: Duration = Duration::from_millis(500);
/// How long a closing chair waits for the phase's own thread to end, so a signature under way
/// is written whole.
pub const THREAD_WAIT: Duration = Duration::from_secs(2);

impl Phases {
    /// No session yet; sessions run with the zaf home `home` through `ports`.
    pub fn new(home: Option<PathBuf>, ports: PhasePorts) -> Self {
        Self {
            home,
            ports,
            running: None,
            queued: Vec::new(),
        }
    }

    /// Carries out what the chair asked.
    pub fn carry(&mut self, command: PhaseCommand) {
        match command {
            PhaseCommand::Start {
                root,
                version,
                rows,
                columns,
            } => {
                if self.running.is_some() {
                    return;
                }
                match self.start(root, version, (rows, columns)) {
                    Ok(running) => self.running = Some(running),
                    Err(error) => self.queued.push(PhaseEvent::Refused(format!(
                        "cannot open a terminal for the session: {error}"
                    ))),
                }
            }
            PhaseCommand::Type(bytes) => {
                if let Some(running) = self.running.as_mut() {
                    // A session that has gone has closed its side: there is no one to type to.
                    let _ = running
                        .writer
                        .write_all(&bytes)
                        .and_then(|()| running.writer.flush());
                }
            }
            PhaseCommand::HangUp => self.signal(libc::SIGHUP),
            PhaseCommand::Kill => self.signal(libc::SIGKILL),
            PhaseCommand::Answer(line) => {
                if let Some(running) = &self.running {
                    let _ = running.answers.send(line);
                }
            }
        }
    }

    /// Sends `signal` to the session's process group, while there is a session.
    fn signal(&self, signal: libc::c_int) {
        if let Some(running) = &self.running {
            running.signal(signal);
        }
    }

    /// Hands what the threads said since the last poll to `chair` at `now`, and what it asks back:
    /// the session's screen when it wrote anything, its ready file once it is there, its events.
    /// The pseudo-terminal follows the pane's size. A settled session is let go.
    pub fn poll(&mut self, chair: &mut Chair, now: Instant) -> Vec<Command> {
        let mut commands = Vec::new();
        for event in std::mem::take(&mut self.queued) {
            commands.extend(chair.phase_event(event, now));
        }
        let Some(running) = self.running.as_mut() else {
            return commands;
        };
        let (mut wrote, mut over) = (false, false);
        while let Ok(heard) = running.heard.try_recv() {
            match heard {
                Heard::Output(bytes) => {
                    running.parser.process(&bytes);
                    wrote = true;
                }
                Heard::Started { phase, ready } => {
                    running.ready = Some(ready);
                    commands.extend(chair.phase_event(PhaseEvent::Started(phase), now));
                }
                Heard::Phase(event) => {
                    running.exited |= matches!(event, PhaseEvent::Exited { .. });
                    over |= matches!(event, PhaseEvent::Done { .. } | PhaseEvent::Refused(_));
                    commands.extend(chair.phase_event(event, now));
                }
            }
        }
        if wrote {
            chair.phase_screen(running.parser.screen().clone());
        }
        if !running.told_ready && running.ready.as_deref().is_some_and(begin::is_ready) {
            running.told_ready = true;
            commands.extend(chair.phase_event(PhaseEvent::Ready, now));
        }
        if let Some(size) = chair.phase_size()
            && size != running.size
        {
            running.resize(size);
            chair.phase_screen(running.parser.screen().clone());
        }
        if over {
            self.running = None;
        }
        commands
    }

    /// The chair closes: a session still under way is hung up, as its terminal closing would, and
    /// killed when it has not ended within [`HANG_UP_WAIT`]; a question it asks is let go
    /// unanswered (the document stays unsigned); the phase's own thread is waited for at most
    /// [`THREAD_WAIT`], so a signature being written is written whole.
    pub fn end(mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        if !running.exited {
            running.signal(libc::SIGHUP);
            if !running.ended_within(HANG_UP_WAIT) {
                running.signal(libc::SIGKILL);
            }
        }
        let thread = running.thread.take();
        drop(running);
        let until = Instant::now() + THREAD_WAIT;
        if let Some(thread) = thread {
            while !thread.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Opens the pseudo-terminal and starts the reader and the phase's own thread.
    fn start(
        &self,
        root: PathBuf,
        version: ReleaseVersion,
        (rows, columns): (u16, u16),
    ) -> Result<Running, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;
        // The side the action runs on, as a plain file descriptor the runner takes (`PtySide`).
        let name = pair
            .master
            .tty_name()
            .ok_or("the pseudo-terminal has no name")?;
        let side = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&name)
            .map_err(|error| format!("{}: {error}", name.display()))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let (said, heard) = mpsc::channel();
        let (answers, asked) = mpsc::channel();
        let output = said.clone();
        std::thread::Builder::new()
            .name("chair-phase-read".to_owned())
            .spawn(move || {
                let mut chunk = vec![0; READ_CHUNK];
                // Ends once every copy of the side is closed: the action has ended and the phase's
                // thread has let its copy go.
                while let Ok(read @ 1..) = reader.read(&mut chunk) {
                    if output.send(Heard::Output(chunk[..read].to_vec())).is_err() {
                        return;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        let home = self.home.clone();
        let ports = self.ports.clone();
        let thread = std::thread::Builder::new()
            .name("chair-phase".to_owned())
            .spawn(move || {
                let work = Work {
                    root: &root,
                    version: version.as_str(),
                    home: home.as_deref(),
                    ports: &ports,
                    said: &said,
                    asked: &asked,
                };
                work.run(side);
            })
            .map_err(|error| error.to_string())?;
        Ok(Running {
            master: pair.master,
            writer,
            parser: vt100::Parser::new(rows, columns, 0),
            heard,
            answers,
            ready: None,
            told_ready: false,
            size: (rows, columns),
            thread: Some(thread),
            exited: false,
        })
    }
}

impl Running {
    /// Sends `signal` to the session's process group: the terminal's foreground group, which is
    /// the action's own, since it leads a session of its own. Never zaf's own group.
    fn signal(&self, signal: libc::c_int) {
        let Some(group) = self.master.process_group_leader() else {
            return;
        };
        // SAFETY: getpgrp and kill take and return plain numbers; the group is checked to be
        // neither zaf's own nor one of the special ones before the signal is sent.
        unsafe {
            if group > 1 && group != libc::getpgrp() {
                libc::kill(-group, signal);
            }
        }
    }

    /// Whether the action ends within `wait`: the phase's thread says so, or has gone.
    fn ended_within(&self, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.heard.recv_timeout(left) {
                Ok(Heard::Phase(
                    PhaseEvent::Exited { .. } | PhaseEvent::Done { .. } | PhaseEvent::Refused(_),
                ))
                | Err(RecvTimeoutError::Disconnected) => return true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }

    /// The pseudo-terminal and the parser to `rows` by `columns`; the session hears of it as from
    /// a terminal resized.
    fn resize(&mut self, (rows, columns): (u16, u16)) {
        self.size = (rows, columns);
        let _ = self.master.resize(PtySize {
            rows,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.screen_mut().set_size(rows, columns);
    }
}

/// The phase's own thread: what `zaf begin` does for one phase, its question asked through the
/// chair.
struct Work<'a> {
    root: &'a Path,
    version: &'a str,
    home: Option<&'a Path>,
    ports: &'a PhasePorts,
    said: &'a Sender<Heard>,
    asked: &'a Receiver<String>,
}

impl Work<'_> {
    fn say(&self, event: PhaseEvent) {
        let _ = self.said.send(Heard::Phase(event));
    }

    fn done(&self, tone: Tone, words: String, next: Option<String>) {
        self.say(PhaseEvent::Done { tone, words, next });
    }

    /// Works out the phase due, runs it on `side` and settles its signature; every way it ends is
    /// said, the last thing said being `Refused` or `Done`.
    fn run(&self, side: std::fs::File) {
        let composition = match Composition::discover_with_home(self.root, self.home) {
            Ok(composition) => composition,
            Err(error) => return self.say(PhaseEvent::Refused(error.to_string())),
        };
        let board = match (self.ports.board)(&composition) {
            Ok(board) => board,
            Err(error) => return self.say(PhaseEvent::Refused(error)),
        };
        let root = composition.root();
        let folder = ReleaseFolder::of(root, composition.manifest(), self.version);
        let ancestors = process::ancestors();
        let state_dir = composition.state_dir();
        let guard = SignGuard {
            state_dir: &state_dir,
            ancestors: &ancestors,
            alive: &process::alive,
        };
        let terminal = Terminal::Pty(PtySide::new(side.as_raw_fd()));
        let request = BeginRequest {
            version: self.version,
            folder: &folder,
            terminal,
            guard: &guard,
            phase: None,
        };
        let phase = match begin::begin(board.as_ref(), &request) {
            Ok(BeginOutcome::Due { phase, .. }) => phase,
            Ok(BeginOutcome::NothingDue(_)) => {
                return self.say(PhaseEvent::Refused(format!(
                    "no phase of {} is due: its epics wait to be signed",
                    self.version
                )));
            }
            Ok(BeginOutcome::Refused(refusal)) => {
                return self.say(PhaseEvent::Refused(refusal.to_string()));
            }
            Err(error) => return self.say(PhaseEvent::Refused(error.to_string())),
        };
        let word = phase_word(phase);
        // The session's own name: a file or a link left by anyone earlier can never end it.
        let ready = begin::ready_file(root, self.version, phase, &begin::session_token());
        let _ = self.said.send(Heard::Started {
            phase: word.to_owned(),
            ready: ready.clone(),
        });
        let run = |name: &str, env: PhaseEnv| {
            (self.ports.run)(root, name, terminal, env.with_ready(ready.clone()))
        };
        let ran = begin::run_phase(&folder, self.version, phase, &run);
        // The reader's end comes once this copy of the side is gone too.
        drop(side);
        let ran = match ran {
            Ok(ran) => ran,
            Err(error) => {
                self.say(PhaseEvent::Exited {
                    ready: false,
                    leads: false,
                });
                let folder = folder.path().display();
                return self.done(Tone::Failed, format!("cannot make {folder}: {error}"), None);
            }
        };
        let is_ready = begin::is_ready(&ready);
        let leads = begin::leads_to_sign(is_ready, &ran);
        self.say(PhaseEvent::Exited {
            ready: is_ready,
            leads,
        });
        if !leads {
            let _ = std::fs::remove_file(&ready);
            let words = match &ran {
                PhaseRun::Undone { outcome } => {
                    format!(
                        "the {word} ended with code {}: the phase is not done",
                        outcome.exit
                    )
                }
                PhaseRun::NotRun { outcome } => outcome
                    .headline()
                    .unwrap_or_else(|| format!("the {word} did not run")),
                PhaseRun::Done => format!("the {word} ended"),
            };
            return self.done(Tone::Failed, words, None);
        }
        // Dropping typed-ahead input is the asker's job: the chair's is any answer already queued
        // for the session, dropped before each question, so only an answer given to it can sign.
        let ask = |prompt: &str| {
            while self.asked.try_recv().is_ok() {}
            let _ = self
                .said
                .send(Heard::Phase(PhaseEvent::Asked(prompt.to_owned())));
            self.asked
                .recv()
                .map_err(|_| io::Error::other("the chair closed before it was answered"))
        };
        let completion = begin::complete_phase(
            board.as_ref(),
            &folder,
            self.version,
            phase,
            SystemTime::now(),
            &ask,
        );
        let _ = std::fs::remove_file(&ready);
        let shown = |path: &Path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
        };
        match completion {
            Ok(Completion::Signed(path)) => {
                let next = match begin::begin(board.as_ref(), &request) {
                    Ok(BeginOutcome::Due { phase: next, .. }) if next != phase => {
                        Some(phase_word(next).to_owned())
                    }
                    _ => None,
                };
                self.done(Tone::Done, format!("signed {}", shown(&path)), next);
            }
            Ok(Completion::NotSigned(path)) => {
                self.done(Tone::Plain, format!("{} is not signed", shown(&path)), None);
            }
            Ok(Completion::Changed(path)) => self.done(
                Tone::Failed,
                format!("{} changed while it asked; o asks again", shown(&path)),
                None,
            ),
            Ok(Completion::Missing(missing)) => {
                let words = if missing.is_empty() {
                    format!("the {word} left nothing to sign")
                } else {
                    missing.join("; ")
                };
                self.done(Tone::Failed, words, None);
            }
            Err(error) => self.done(Tone::Failed, error.to_string(), None),
        }
    }
}

#[cfg(test)]
mod tests;
