//! The live view of `zaf spawn --watch` in a terminal: one frame, redrawn in place (design 0.5.2,
//! Part F, M25).
//!
//! The scheduler's CLI host starts it only when stdin and stdout are a terminal, `--json` is not
//! given and the terminal can redraw ([`wanted`]); anywhere else the stream stays as it was. It
//! draws on the normal screen, never the alternate one: it moves up over the last frame, clears
//! below and draws the next, so the last frame stays in the scrollback when the watch ends.
//!
//! The view runs on a thread of its own, fed by the host's tick events; the scheduler's thread
//! never waits on drawing. The thread reads the window's size before each frame, gathers the
//! project's state ([`gather_project`]) after each tick and whenever the hub's reports file changes
//! (the gather runs on a thread of its own, so the spinner goes on), and between those only turns
//! the spinner, [`FRAME_EVERY`] apart.
//!
//! What the process writes to stdout and stderr goes to `.zaf/logs/scheduler.log` while the view is
//! up: starting it points descriptors 1 and 2 at the log, so each tick's text block, the host's
//! lines and its warnings land there, and so does what the tick's own children write. The view
//! keeps a copy of the terminal to draw on. A tick that fails shows one line under the frame naming
//! the log, until the next tick that does not.
//!
//! When the scheduler reports the version built ([`WatchView::finish`]) the view gathers the state
//! once more, draws the last frame and the end screen under it (what was built, the release pull
//! request to merge, the command that starts the next release) and ends; the host then stops the
//! scheduler as a stop does and the watch exits 0.
//!
//! Typed keys are not echoed (`stty -echo`; Ctrl-C still reaches the scheduler as a signal, as it
//! does without a view). Everything is put back by [`Restore::put_back`], which runs once on every
//! way out: the view's drop (a normal end, a Ctrl-C that ended the loop, and the restart into a
//! new zaf, which drops it before the `exec`) and a panic hook. It throws away what was typed
//! ahead first, so lines a paste left queued never reach the shell once the settings are back, and
//! turns bracketed paste off (`ESC[?2004l`) with them.
//!
//! # Answering (design 0.5.2, M26)
//!
//! A question open on the board stops the redrawing: the frame that shows it is the last, the
//! question is written under it in full with its own number and how to answer from any terminal
//! (`zaf board ruling <question> '…'`), echo goes on, and one whole line is read from stdin on a
//! reader thread ([`read_line`]) that hands it back over a channel; the view's thread only polls
//! for it, so the ticks, the gathers and the host go on. Bracketed paste (`ESC[?2004h`) is on while a
//! question is asked and off after its answer or skip, as echo is: a terminal then wraps a paste in
//! `ESC[200~` … `ESC[201~`, so the view knows a paste when it reads one. A paste with a newline in
//! it, its last line ended or not, is refused and the question asked again; a one-line paste is
//! taken as if typed, without the markers. A line with words is ruled as
//! `zaf board ruling` rules it ([`rule_in`]) with no gap check, since it went through no shell, a
//! tick is asked for, and the frames go on below what was said. An empty line skips the question
//! for the run: it is not asked again, and shows "? question for you" as before. Questions are
//! asked one at a time, the lowest number first. While one is asked the host is told so
//! ([`WatchView::asking`]) and does not restart into a new binary. A stop while one is asked ends
//! the view with that question's number ([`WatchView::end`]), which the host names once the
//! terminal is back.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use semver::Version;

use super::board::rule_in;
use super::hub::stty;
use super::screen::{Screen, window_size};
use crate::adapters::process;
use crate::app::hub::controls::Failure;
use crate::app::hub::state::{ProjectState, QuestionState, gather_project};
use crate::app::hub::{LineState, Mark};
use crate::app::schedule::request;
use crate::app::spawn::view::{SPINNER, frame};
use crate::composition::{STATE_DIR, zaf_home};
use crate::domain::TicketId;
use crate::domain::screen::{Line, Meaning, clean};

/// The log the view's process writes to, under the state folder.
pub const LOG_FILE: &str = "logs/scheduler.log";

/// How long the spinner stands on one step: the mock's.
const FRAME_EVERY: Duration = Duration::from_millis(90);

/// Turns bracketed paste on: the terminal then wraps a paste in [`PASTE_START`] and [`PASTE_END`].
const PASTE_ON: &str = "\x1b[?2004h";

/// Turns bracketed paste off.
const PASTE_OFF: &str = "\x1b[?2004l";

/// What a terminal in bracketed paste mode sends before a paste.
const PASTE_START: &[u8] = b"\x1b[200~";

/// What a terminal in bracketed paste mode sends after a paste.
const PASTE_END: &[u8] = b"\x1b[201~";

/// The rows a frame leaves free: the failed tick's line, the cursor's line and one spare.
const SPARE_ROWS: u16 = 3;

/// What the host tells the view.
enum Event {
    /// Tick `number` ended with `code`; the state is read again.
    Ticked { number: u64, code: u8 },
    /// The version is built: the last frame, the end screen, and the view ends.
    Built(End),
    /// The view ends.
    Stop,
}

/// What the end screen says: the version that is built, the release pull request and the version
/// the next release is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct End {
    /// The version that is built.
    pub version: Version,
    /// The release pull request's number.
    pub number: String,
    /// The version after it, from the one next-version function.
    pub next: Version,
}

/// The screen this process would draw the view on: none unless stdin and stdout are a terminal,
/// `--json` is not given and the terminal can redraw (`TERM` is not `dumb`).
pub fn wanted(json: bool) -> Option<Screen> {
    if json || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    let screen = Screen::detect();
    screen.live.then_some(screen)
}

/// The path of the log, for the line that names it.
fn log_name() -> String {
    format!("{STATE_DIR}/{LOG_FILE}")
}

/// What the restore path does to the terminal, so a test can stand in for it.
trait Terminal: Send + Sync {
    /// Writes out what the process has buffered for stdout.
    fn flush_output(&self);
    /// Throws away what was typed and not yet read.
    fn flush_typed(&self);
    /// Makes descriptor `fd` another name for `file`.
    fn replace_fd(&self, fd: i32, file: &File);
    /// Writes `text` to the terminal through `to`, the copy of it the view kept, and flushes.
    fn write_terminal(&self, to: &File, text: &str);
    /// Sets the terminal's settings back to `settings`, the output of `stty -g`.
    fn settle(&self, settings: &str);
}

/// The terminal this process runs on.
struct Real;

impl Terminal for Real {
    fn flush_output(&self) {
        let _ = std::io::stdout().flush();
    }

    fn flush_typed(&self) {
        process::flush_typed();
    }

    fn replace_fd(&self, fd: i32, file: &File) {
        let _ = process::replace_fd(fd, file);
    }

    fn write_terminal(&self, mut to: &File, text: &str) {
        let _ = to.write_all(text.as_bytes()).and_then(|()| to.flush());
    }

    fn settle(&self, settings: &str) {
        let _ = stty(&[settings]);
    }
}

/// What puts the terminal back: the descriptors the output was moved off and the `stty` settings
/// from before the view. Idempotent, so every way out may call it.
struct Restore {
    terminal: Box<dyn Terminal>,
    stdout: File,
    stderr: File,
    /// The output of `stty -g` before the view.
    settings: String,
    done: AtomicBool,
}

impl Restore {
    fn put_back(&self) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        self.terminal.flush_output();
        // What was typed ahead (a paste's later lines) is thrown away before the settings come
        // back: once they do, the shell would read it and run it unseen.
        self.terminal.flush_typed();
        self.terminal.replace_fd(1, &self.stdout);
        self.terminal.replace_fd(2, &self.stderr);
        // Bracketed paste goes off with the echo setting: the shell must not get the markers.
        self.terminal.write_terminal(&self.stdout, PASTE_OFF);
        self.terminal.settle(&self.settings);
    }
}

/// The running view. Dropping it ends the view's thread and puts the terminal back.
pub struct WatchView {
    events: Sender<Event>,
    /// Ends with the number of the question that was being asked when the view ended, if any.
    thread: Option<JoinHandle<Option<u64>>>,
    restore: Arc<Restore>,
    /// Whether a question is being asked now.
    asking: Arc<AtomicBool>,
}

impl WatchView {
    /// Starts the view of `version`'s build on `screen`, the project at `root` keeping its state in
    /// `state_dir`. None, with the terminal as it was, when anything cannot be set up: the host
    /// then streams as it always did.
    pub fn start(
        screen: Screen,
        root: &Path,
        state_dir: &Path,
        version: &str,
    ) -> Option<WatchView> {
        let version = Version::parse(version).ok()?;
        let log_path = state_dir.join(LOG_FILE);
        fs::create_dir_all(log_path.parent()?).ok()?;
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok()?;
        let settings = stty(&["-g"]).filter(|settings| !settings.is_empty())?;
        let stdout = process::duplicate_fd(1).ok()?;
        let stderr = process::duplicate_fd(2).ok()?;
        let drawing = stdout.try_clone().ok()?;
        let _ = writeln!(
            log,
            "-- zaf spawn --watch {version}, live view, {}",
            humantime::format_rfc3339_seconds(SystemTime::now())
        );
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        stty(&["-echo"])?;
        let restore = Arc::new(Restore {
            terminal: Box::new(Real),
            stdout,
            stderr,
            settings,
            done: AtomicBool::new(false),
        });
        // From here every way out puts the terminal back: a panic first, before its message.
        let previous = std::panic::take_hook();
        let on_panic = Arc::clone(&restore);
        std::panic::set_hook(Box::new(move |info| {
            on_panic.put_back();
            previous(info);
        }));
        if process::replace_fd(1, &log).is_err() || process::replace_fd(2, &log).is_err() {
            restore.put_back();
            return None;
        }
        let view = View {
            screen,
            drawing,
            root: root.to_owned(),
            state_dir: state_dir.to_owned(),
            version,
            started: Instant::now(),
        };
        Self::spawn(restore, view)
    }

    /// Runs `view` on its thread, `restore` putting the terminal back at every way out. None, with
    /// the terminal put back, when the thread cannot start.
    fn spawn(restore: Arc<Restore>, view: View) -> Option<WatchView> {
        let (events, inbox) = mpsc::channel();
        let asking = Arc::new(AtomicBool::new(false));
        let asked = Arc::clone(&asking);
        let thread = std::thread::Builder::new()
            .name("watch-view".to_owned())
            .spawn(move || {
                // A stop signal is the scheduler thread's to take; this thread never gets it.
                let _ = process::hold_stop();
                view.run(&inbox, &asked)
            });
        let Ok(thread) = thread else {
            restore.put_back();
            return None;
        };
        Some(WatchView {
            events,
            thread: Some(thread),
            restore,
            asking,
        })
    }

    /// Tells the view that tick `number` ended with `code`, which is zero for a tick that went
    /// through. Never waits on drawing.
    pub fn ticked(&self, number: u64, code: u8) {
        let _ = self.events.send(Event::Ticked { number, code });
    }

    /// Whether a question is being asked of the person now: the host holds its restart meanwhile.
    pub fn asking(&self) -> bool {
        self.asking.load(Ordering::SeqCst)
    }

    /// Ends the view and puts the terminal back; gives the number of the question that was being
    /// asked, and so stays open, if there was one.
    pub fn end(mut self) -> Option<u64> {
        self.wind_up()
    }

    fn wind_up(&mut self) -> Option<u64> {
        let _ = self.events.send(Event::Stop);
        let open = self
            .thread
            .take()
            .and_then(|thread| thread.join().ok())
            .flatten();
        self.restore.put_back();
        open
    }
}

impl WatchView {
    /// Ends the view with the end screen for `end`: the last frame and the lines under it are
    /// drawn, then the terminal is put back. Returns once that is done.
    pub fn finish(self, end: End) {
        let _ = self.events.send(Event::Built(end));
        // Dropping joins the thread, which ends after the drawing, and puts the terminal back.
    }
}

impl Drop for WatchView {
    fn drop(&mut self) {
        self.wind_up();
    }
}

/// What the view's thread needs to draw.
struct View {
    screen: Screen,
    drawing: File,
    root: PathBuf,
    state_dir: PathBuf,
    version: Version,
    /// When the watch started: the end screen's minutes count from here.
    started: Instant,
}

impl View {
    /// Runs until the host ends the view; gives the number of the question being asked then, if
    /// one was.
    fn run(self, inbox: &Receiver<Event>, asking: &AtomicBool) -> Option<u64> {
        let mut out = &self.drawing;
        let mut state: Option<ProjectState> = None;
        let mut gathering: Option<Receiver<ProjectState>> = None;
        let mut gather_again = true;
        let mut failed: Option<String> = None;
        let mut mark = Mark::read(&self.state_dir);
        let mut drawn = 0;
        let mut spin: u8 = 0;
        let mut ending: Option<End> = None;
        // The question being asked, and those answered or skipped in this run.
        let mut open: Option<Asking> = None;
        let mut settled: BTreeSet<u64> = BTreeSet::new();
        loop {
            match inbox.recv_timeout(FRAME_EVERY) {
                Ok(Event::Ticked { number, code }) => {
                    gather_again = true;
                    failed = (code != 0).then(|| {
                        format!(
                            "tick {number} failed with exit {code}; the log is {}",
                            log_name()
                        )
                    });
                }
                Ok(Event::Built(end)) => {
                    // The last frame is drawn from the state as it stands now, read here.
                    state = Some(gather_project(&self.root, SystemTime::now()));
                    ending = Some(end);
                }
                Ok(Event::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    return open.map(|asked| asked.number);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now_mark = Mark::read(&self.state_dir);
            if now_mark != mark {
                mark = now_mark;
                gather_again = true;
            }
            if ending.is_some() {
                gathering = None;
                gather_again = false;
            }
            if let Some(inbox) = &gathering {
                match inbox.try_recv() {
                    Ok(gathered) => {
                        state = Some(gathered);
                        gathering = None;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => gathering = None,
                }
            }
            if gather_again && gathering.is_none() {
                gather_again = false;
                gathering = Some(gather(self.root.clone()));
            }
            if open.is_some() && ending.is_some() {
                // The version is built: its end screen wins over a question waiting for a line.
                open = None;
                asking.store(false, Ordering::SeqCst);
                let _ = out.write_all(PASTE_OFF.as_bytes());
                drawn = 0;
            }
            if let Some(asked) = &open {
                // The view stands still while a question waits: only its answer is looked for.
                let line = match asked.line.try_recv() {
                    Ok(Typed::Line(line)) => line,
                    Ok(Typed::Many) => {
                        // More than one line: nothing is ruled, and the same question is asked
                        // again once the refusal is out.
                        let again = refused(&self.screen, &asked.block);
                        let number = asked.number;
                        let block = asked.block.clone();
                        if out
                            .write_all(again.as_bytes())
                            .and_then(|()| out.flush())
                            .is_err()
                        {
                            asking.store(false, Ordering::SeqCst);
                            return None;
                        }
                        open = Some(Asking {
                            number,
                            block,
                            line: read_line(),
                        });
                        continue;
                    }
                    Err(TryRecvError::Empty) => continue,
                    // The reader is gone with nothing read: the same as an empty line.
                    Err(TryRecvError::Disconnected) => String::new(),
                };
                let number = asked.number;
                open = None;
                // Echo is off again before anything else is said or drawn.
                let _ = stty(&["-echo"]);
                asking.store(false, Ordering::SeqCst);
                settled.insert(number);
                let said = format!("{PASTE_OFF}{}", self.settle(number, &line));
                if out
                    .write_all(said.as_bytes())
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    return None;
                }
                // What was said stays above; the next frame is drawn below it.
                drawn = 0;
                gather_again = true;
            }
            let (rows, columns) = window_size().unwrap_or((self.screen.rows, self.screen.columns));
            let screen = Screen {
                rows,
                columns,
                ..self.screen
            };
            let mut lines = match &state {
                Some(state) => frame(
                    state,
                    &self.version,
                    SystemTime::now(),
                    spin,
                    (columns.saturating_sub(1), rows.saturating_sub(SPARE_ROWS)),
                ),
                None => vec![reading(&self.version, spin)],
            };
            if let Some(end) = &ending {
                let merged = state
                    .as_ref()
                    .map_or(0, |state| merged(state, &end.version));
                let minutes = self.started.elapsed().as_secs() / 60;
                lines.extend(end_lines(end, merged, minutes));
            } else if let Some(failure) = &failed {
                lines.push(vec![(Meaning::Failed, failure.as_str().into())]);
            }
            let (text, height) = redraw(&screen, &lines, drawn);
            if out
                .write_all(text.as_bytes())
                .and_then(|()| out.flush())
                .is_err()
            {
                // The terminal is gone: nothing more to draw on.
                return None;
            }
            drawn = height;
            spin = spin.wrapping_add(1);
            if ending.is_some() {
                return None;
            }
            // The frame that shows a question is the last one before it is asked.
            let next = state
                .as_ref()
                .and_then(|state| next_question(state, &settled));
            if let Some(question) = next {
                asking.store(true, Ordering::SeqCst);
                process::flush_typed();
                let _ = stty(&["echo"]);
                let block = question_block(&self.screen, question);
                if out
                    .write_all(format!("{PASTE_ON}{block}").as_bytes())
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    asking.store(false, Ordering::SeqCst);
                    return None;
                }
                open = Some(Asking {
                    number: question.number,
                    block,
                    line: read_line(),
                });
                drawn = 0;
            }
        }
    }

    /// Settles the question `number` with the `line` typed for it, and says what happened: a line
    /// with words is ruled and a tick asked for; an empty one skips the question.
    fn settle(&self, number: u64, line: &str) -> String {
        let Some(answer) = answer_of(line) else {
            let skipped =
                format!("  skipped; #{number} stays open, and is not asked again in this run.");
            return format!("{}\n\n", self.screen.paint(Meaning::Dim, &skipped));
        };
        match self.rule(number, answer) {
            Ok(()) => format!(
                "  {} recorded; #{number} goes on.\n\n",
                self.screen.paint(Meaning::Done, "✓")
            ),
            Err(why) => {
                let failure = flat(&format!("#{number} was not recorded: {why}"));
                format!("  {}\n\n", self.screen.paint(Meaning::Failed, &failure))
            }
        }
    }

    /// Rules `number` with `answer` as `zaf board ruling` does and asks the scheduler for a tick.
    /// The gap check is not applied: this text went through no shell.
    fn rule(&self, number: u64, answer: &str) -> Result<(), String> {
        let question: TicketId = number
            .to_string()
            .parse()
            .map_err(|why: crate::domain::IdError| why.to_string())?;
        rule_in(&self.root, zaf_home().as_deref(), question, answer).map_err(|failure| {
            match failure {
                Failure::Refused(why) | Failure::Failed(why) => why,
            }
        })?;
        let who = format!("ruling {question}");
        if let Err(not_asked) =
            request::ask(&self.state_dir, &who, SystemTime::now(), &process::alive)
        {
            // Said in the log: the timer picks the ruling up at its next tick.
            eprintln!("zaf spawn --watch: {}", not_asked.left_to_the_timer(&who));
        }
        Ok(())
    }
}

/// A question that is being asked: its number, and where the line typed for it arrives.
struct Asking {
    number: u64,
    /// What was written to ask it, written again when the answer is refused.
    block: String,
    line: Receiver<Typed>,
}

/// The open question to ask next: the lowest number not yet answered or skipped.
fn next_question<'a>(
    state: &'a ProjectState,
    settled: &BTreeSet<u64>,
) -> Option<&'a QuestionState> {
    state
        .questions
        .iter()
        .filter(|question| !settled.contains(&question.number))
        .min_by_key(|question| question.number)
}

/// Reads one whole line from stdin on a thread of its own, so that nothing else waits on the
/// terminal. The line arrives with its ending; an empty one when stdin ended.
fn read_line() -> Receiver<Typed> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = process::hold_stop();
        let typed = read_typed(
            &mut std::io::stdin().lock(),
            process::typed_waiting,
            process::flush_typed,
        );
        let _ = send.send(typed);
    });
    receive
}

/// What was typed for a question: one line, or a line with more queued behind it (a paste).
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    /// The line, with its ending; an empty one when stdin ended.
    Line(String),
    /// More input was queued after the line: it is thrown away and the line is not taken.
    Many,
}

/// Reads one line from `input`, then asks `queued` whether more input waits behind it. On more,
/// `clear` throws it away and the line is refused, so a pasted block is never read as an answer
/// piece by piece.
///
/// A line that starts with `ESC[200~` is a bracketed paste. One holding a newline is refused the
/// same way, whether or not its last line was ended: the line discipline hands a line over at its
/// newline, so a paste with no final newline shows as a first line that ends inside the markers.
/// A one-line paste is the line as typed, without the markers.
fn read_typed(
    input: &mut impl BufRead,
    queued: impl FnOnce() -> bool,
    clear: impl FnOnce(),
) -> Typed {
    let mut bytes = Vec::new();
    let _ = input.read_until(b'\n', &mut bytes);
    if let Some(inside) = bytes.strip_prefix(PASTE_START) {
        let (pasted, rest) = match find(inside, PASTE_END) {
            Some(at) => (&inside[..at], &inside[at + PASTE_END.len()..]),
            // No end yet: the line ended inside the paste, which therefore holds a newline.
            None if inside.ends_with(b"\n") => {
                clear();
                return Typed::Many;
            }
            None => (inside, &[][..]),
        };
        if pasted.iter().any(|byte| matches!(byte, b'\n' | b'\r')) {
            clear();
            return Typed::Many;
        }
        let mut line = pasted.to_vec();
        line.extend(unmarked(rest));
        bytes = line;
    }
    if queued() {
        clear();
        return Typed::Many;
    }
    Typed::Line(String::from_utf8_lossy(&bytes).into_owned())
}

/// Where `needle` first occurs in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `bytes` without any paste marker.
fn unmarked(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(PASTE_START) || rest.starts_with(PASTE_END) {
            at += PASTE_START.len();
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    out
}

/// What is written when several lines were given as an answer: the refusal, then the question's
/// `block` again, which ends on the prompt.
fn refused(screen: &Screen, block: &str) -> String {
    let said = "  one line only; answer longer ones with `zaf board ruling`";
    format!("\n{}\n{block}", screen.paint(Meaning::Failed, said))
}

/// The answer a typed line holds: the line without its ending, as typed; none for a line with no
/// words, which skips the question.
fn answer_of(line: &str) -> Option<&str> {
    let answer = line.trim_end_matches(['\n', '\r']);
    (!answer.trim().is_empty()).then_some(answer)
}

/// `text` with each control character a space (see [`clean`]).
fn flat(text: &str) -> String {
    clean(text)
}

/// What is written for a question to be answered: its number and its whole text, how to answer
/// from any terminal, and the prompt, which the typed answer follows on the same line.
pub fn question_block(screen: &Screen, question: &QuestionState) -> String {
    let number = question.number;
    let mut words = question.text.lines();
    let first = flat(words.next().unwrap_or_default());
    let asks = screen.paint(Meaning::Waiting, &format!("#{number} asks:"));
    let mut out = format!("\n  {asks} {}\n", first.trim_end());
    for line in words {
        out.push_str(&format!("    {}\n", flat(line).trim_end()));
    }
    let rest = "The rest keeps building while you think. Answer here, or from any terminal:";
    out.push_str(&format!("  {}\n", screen.paint(Meaning::Dim, rest)));
    out.push_str(&format!(
        "  {} {}\n\n",
        screen.paint(Meaning::Program, "zaf"),
        screen.paint(
            Meaning::Verb,
            &format!("board ruling {number} 'your answer'")
        ),
    ));
    out.push_str(&format!(
        "  {} ",
        screen.paint(Meaning::Bold, "Your answer:")
    ));
    out
}

/// The tickets of the epics of `version` that are merged, in `state`.
fn merged(state: &ProjectState, version: &Version) -> usize {
    state
        .epics
        .iter()
        .filter(|epic| {
            epic.release
                .as_deref()
                .is_some_and(|release| release.trim() == version.to_string())
        })
        .flat_map(|epic| epic.tasks.iter())
        .filter(|task| task.state == LineState::Done)
        .count()
}

/// The end screen's lines: what was built and in how many minutes, the pull request to merge and
/// the command that starts the next release, a blank line around them as the mock has it.
pub fn end_lines(end: &End, merged: usize, minutes: u64) -> Vec<Line> {
    let tickets = if merged == 1 { "ticket" } else { "tickets" };
    vec![
        Vec::new(),
        vec![
            (Meaning::Done, "✓".into()),
            (Meaning::Bold, " ".into()),
            (Meaning::Program, "zaf".into()),
            (Meaning::Bold, " built ".into()),
            (Meaning::Noun, end.version.to_string().into()),
            (Meaning::Bold, ": ".into()),
            (Meaning::Bold, format!("{merged} {tickets}").into()),
            (Meaning::Dim, format!(" merged in {minutes} min.").into()),
        ],
        Vec::new(),
        vec![
            (Meaning::Bold, "Next:".into()),
            (
                Meaning::Bold,
                format!(" merge pull request #{} on the forge to ship ", end.number).into(),
            ),
            (Meaning::Noun, format!("{}.", end.version).into()),
        ],
        vec![
            (Meaning::Dim, "Then start the next release: ".into()),
            (Meaning::Program, "zaf".into()),
            (
                Meaning::Verb,
                format!(" begin --version {}", end.next).into(),
            ),
        ],
        Vec::new(),
    ]
}

/// Gathers the project's state on a thread of its own and hands it back.
fn gather(root: PathBuf) -> Receiver<ProjectState> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = process::hold_stop();
        let _ = send.send(gather_project(&root, SystemTime::now()));
    });
    receive
}

/// The one line drawn until the first state is read.
fn reading(version: &Version, spin: u8) -> Line {
    let step = SPINNER[usize::from(spin) % SPINNER.len()];
    vec![
        (Meaning::Program, "zaf".into()),
        (Meaning::Bold, " is building ".into()),
        (Meaning::Noun, version.to_string().into()),
        (Meaning::Verb, format!("  {step}  ").into()),
        (Meaning::Dim, "reading the board".into()),
    ]
}

/// What to write to put `lines` where the last frame of `previous` lines stood: up over it, to the
/// first column, clear to the end of the screen, then the lines painted, each ended. Also the
/// number of lines now on the screen.
pub fn redraw(screen: &Screen, lines: &[Line], previous: usize) -> (String, usize) {
    let mut out = String::new();
    if previous > 0 {
        out.push_str(&format!("\x1b[{previous}A"));
    }
    out.push_str("\r\x1b[J");
    for line in lines {
        for (meaning, text) in line {
            out.push_str(&screen.paint(*meaning, text.as_str()));
        }
        out.push('\n');
    }
    (out, lines.len())
}

#[cfg(test)]
mod tests;
