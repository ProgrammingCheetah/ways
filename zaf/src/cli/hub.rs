//! `zaf hub` and `zaf hub report`, and the reports the other verbs leave (docs/zaf-actions.md §5).
//!
//! `zaf hub` draws the frame. In a terminal it then waits: a new report redraws it, between
//! reports only the clocks move, and Ctrl-C puts the terminal back and exits 0. Piped or
//! redirected it draws one frame and exits. `zaf hub report` is the profiles' `PostToolUse` hook:
//! it reads the tool call on stdin, leaves one report and prints nothing, whatever it is given.
//! A verb that changes state leaves its own report once it has run; a report that cannot be written
//! never fails or changes the verb.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, SystemTime};

use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, HubAction, Verb};
use crate::app::check::Target;
use crate::app::guard;
use crate::app::hub::{
    HubError, HubInputs, Mark, PROGRAMMER, Report, TEAM, ToolCall, Watch, draw, gather,
    lan_address, record, task_in_workspace,
};
use crate::app::report::PriceTable;
use crate::app::report::ledger::{LEDGER_FILE, Ledger, LedgerRow};
use crate::app::{actions::SliceRecords, spawn};
use crate::composition::{Composition, zaf_home};
use crate::domain::IssueNumber;

/// How much of a hook's payload is read: a tool call with its output can be large, and the hook
/// needs only its first fields.
const MAX_PAYLOAD: u64 = 1 << 20;

/// How often the live hub looks at the reports file and the keyboard. Only a file's metadata is
/// read: the board is read when the file has changed.
const POLL: Duration = Duration::from_millis(250);

/// The width of a frame when the terminal does not say.
const DEFAULT_WIDTH: usize = 100;

mod dashboard;

/// Runs `zaf hub`, `zaf hub --port <n>` or `zaf hub report`.
pub fn run(cli: &Cli, action: Option<&HubAction>, port: Option<u16>) -> u8 {
    match action {
        Some(HubAction::Report) => hook(),
        None => show(cli, port),
    }
}

// The hook.

/// `zaf hub report`: reads the payload on stdin, leaves a report and returns 0.
fn hook() -> u8 {
    let mut bytes = Vec::new();
    let stdin = std::io::stdin();
    // A person typing the verb by hand is not waiting to be read.
    if !stdin.is_terminal() {
        let _ = stdin.lock().take(MAX_PAYLOAD).read_to_end(&mut bytes);
    }
    let payload = String::from_utf8_lossy(&bytes);
    let here = std::env::current_dir().ok();
    leave_tool_report(
        &payload,
        here.as_deref(),
        zaf_home().as_deref(),
        SystemTime::now(),
    );
    OK
}

/// Leaves the report of one tool call: its word, and who made it. The call's `cwd` finds the
/// project, and is the slice workspace of a task when it is under one; with no `cwd` the
/// hook's own directory stands in. Nothing is returned and nothing is printed: a payload that
/// is not a payload, a directory in no project and a file that cannot be written all end here.
pub fn leave_tool_report(payload: &str, here: Option<&Path>, home: Option<&Path>, now: SystemTime) {
    let call = ToolCall::parse(payload);
    let Some(cwd) = call.cwd.as_deref().or(here) else {
        return;
    };
    let Ok(composition) = Composition::discover_with_home(cwd, home) else {
        return;
    };
    let manifest = composition.manifest();
    let workspaces = composition.root().join(&manifest.workspaces.path);
    let task = task_in_workspace(cwd, &workspaces, manifest.branches.task.as_ref());
    let worker = if task.is_some() { PROGRAMMER } else { TEAM };
    record(
        &composition.state_dir(),
        &Report {
            at: now,
            task,
            worker: worker.to_owned(),
            what: call.word().to_owned(),
        },
    );
}

// The reports the verbs leave.

/// What a verb that changed something reports: on which task and in which words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The task it is about, when the verb names one.
    pub task: Option<IssueNumber>,
    /// What happened, in general words.
    pub what: &'static str,
    /// Whether a refusal (exit 1) still counts: a check that finds a failure has still run.
    pub counts_when_refused: bool,
}

impl Event {
    fn done(task: impl Into<Option<IssueNumber>>, what: &'static str) -> Option<Self> {
        Some(Self {
            task: task.into(),
            what,
            counts_when_refused: false,
        })
    }

    fn check(task: Option<IssueNumber>, what: &'static str) -> Option<Self> {
        Some(Self {
            task,
            what,
            counts_when_refused: true,
        })
    }

    /// Whether a verb that exited with `code` leaves its report.
    pub fn counts(&self, code: u8) -> bool {
        code == OK || (self.counts_when_refused && code == REFUSED)
    }
}

/// The report a verb leaves, or none when it only reads. A claim, a finished task, a question
/// raised or ruled, a slice opened, published, merged or dropped, and a task or pull request
/// check each leave one, run by name or by `zaf do`.
pub fn event_of(verb: &Verb) -> Option<Event> {
    use crate::app::hub::{TESTING, WAITING_ON_REVIEW};
    match verb {
        Verb::Board { action } => match action {
            super::BoardAction::Claim { task } => Event::done(*task, "claimed"),
            super::BoardAction::Done { task } => Event::done(*task, "finished"),
            super::BoardAction::Question { on, .. } => Event::done(*on, "question raised"),
            super::BoardAction::Ruling { .. } => Event::done(None, "question ruled"),
            // A heartbeat on the ticket and a listing change nothing the hub shows.
            super::BoardAction::Ready | super::BoardAction::Report { .. } => None,
        },
        Verb::Slice { action } => match action {
            super::SliceAction::Open { task } => Event::done(*task, "slice opened"),
            super::SliceAction::Pr { task } => Event::done(*task, "pull request opened"),
            super::SliceAction::Merge { task } => Event::done(*task, "merged"),
            super::SliceAction::Drop { task } => Event::done(*task, "slice dropped"),
        },
        Verb::Check { target, status } if !status => match target.parse::<Target>().ok()? {
            Target::Task(task) => Event::check(Some(task), TESTING),
            Target::Pr { task, .. } => Event::check(task, WAITING_ON_REVIEW),
        },
        Verb::Verify { task } => Event::check(Some(*task), TESTING),
        Verb::Do(args) if !args.show => {
            let (name, rest) = args.words.split_first()?;
            let task = rest
                .first()
                .and_then(|word| word.to_str())
                .and_then(|word| word.trim_start_matches('#').parse::<IssueNumber>().ok());
            match name.to_str()? {
                "slice-open" => Event::done(task, "slice opened"),
                "slice-pr" => Event::done(task, "pull request opened"),
                "slice-merge" => Event::done(task, "merged"),
                "slice-drop" => Event::done(task, "slice dropped"),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Who a verb's report names: the role of `ZAF_INSTANCE`, else `team`.
fn worker_of(instance: Option<&str>) -> String {
    instance
        .and_then(|text| text.split_whitespace().next())
        .unwrap_or(TEAM)
        .to_owned()
}

/// Leaves the report for `verb`, which exited with `code`, in the state folder; true when it left
/// one. A failure to write is dropped.
pub fn leave_report_in(
    state_dir: &Path,
    verb: &Verb,
    code: u8,
    worker: Option<&str>,
    now: SystemTime,
) -> bool {
    let Some(event) = event_of(verb).filter(|event| event.counts(code)) else {
        return false;
    };
    record(
        state_dir,
        &Report {
            at: now,
            task: event.task,
            worker: worker_of(worker),
            what: event.what.to_owned(),
        },
    );
    true
}

/// Called once a verb has run, with its exit code. Outside a project there is nowhere to report.
pub fn leave_report(verb: &Verb, code: u8) {
    if event_of(verb).is_none() {
        return;
    }
    let Ok(composition) = Composition::discover_here_quiet() else {
        return;
    };
    let instance = std::env::var(super::board::INSTANCE_VAR).ok();
    leave_report_in(
        &composition.state_dir(),
        verb,
        code,
        instance.as_deref(),
        SystemTime::now(),
    );
}

// The frame.

/// What the live loop asks of the world, so a test can stand in for the terminal.
pub trait Pace {
    /// The time now.
    fn now(&mut self) -> SystemTime;
    /// The width a frame is drawn to.
    fn width(&mut self) -> usize;
    /// Waits for the next look. False when the viewer is done.
    fn wait(&mut self) -> bool;
}

/// Draws the frame once and, with a `pace`, keeps it current: a changed reports file reads the
/// board and draws again; a quiet one only redraws the clocks. Without a `pace` (piped or
/// redirected output) one frame is drawn and that is all.
pub fn serve<G, W>(
    out: &mut W,
    gather: G,
    state_dir: &Path,
    width: usize,
    pace: Option<&mut dyn Pace>,
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<crate::app::hub::Snapshot, HubError>,
    W: Write,
{
    let io = |error: std::io::Error| error.to_string();
    let Some(pace) = pace else {
        let snapshot = {
            let mut gather = gather;
            gather(SystemTime::now()).map_err(|error| error.to_string())?
        };
        out.write_all(draw(&snapshot, width).as_bytes())
            .map_err(io)?;
        return out.flush().map_err(io);
    };
    let (mut watch, frame) = Watch::start(gather, pace.now(), Mark::read(state_dir), width)
        .map_err(|error| error.to_string())?;
    paint(out, &frame).map_err(io)?;
    while pace.wait() {
        match watch.step(pace.now(), Mark::read(state_dir)) {
            Ok(Some(frame)) => paint(out, &frame).map_err(io)?,
            Ok(None) => {}
            // A board that fails for one report leaves the last frame up; the next report tries again.
            Err(_) => {}
        }
    }
    Ok(())
}

/// Clears the screen and writes a frame from its top.
fn paint(out: &mut impl Write, frame: &str) -> std::io::Result<()> {
    write!(out, "\x1b[H\x1b[2J{frame}")?;
    out.flush()
}

/// `zaf hub`, or `zaf hub --port <n>` to serve the frame as a page.
fn show(cli: &Cli, port: Option<u16>) -> u8 {
    let path = cli.verb.path();
    // The port is taken first: one in use is refused before anything is read.
    let dashboard = match port.map(|port| dashboard::bind(port, &path)).transpose() {
        Ok(dashboard) => dashboard,
        Err(code) => return code,
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let forge = match composition.forge() {
        Ok(forge) => forge,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let vcs = match composition.vcs() {
        Ok(vcs) => vcs,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let state_dir = composition.state_dir();
    let records = SliceRecords::new(composition.root());
    let models: BTreeMap<String, String> = composition
        .manifest()
        .roles
        .iter()
        .map(|(role, section)| (role.to_string(), section.model.to_string()))
        .collect();
    let prices = guard::read_local(&state_dir)
        .ok()
        .and_then(|local| PriceTable::from_local_toml(&local).ok())
        .unwrap_or_default();
    let project = composition.manifest().project.name.to_string();
    let slots_total = composition.manifest().spawn.slots;
    let gatherer = |now: SystemTime| {
        let running = spawn::load(&state_dir).map_or(0, |state| state.running());
        let ledger = ledger_rows(&composition);
        gather(&HubInputs {
            board: board.as_ref(),
            forge: forge.as_ref(),
            vcs: vcs.as_ref(),
            records: &records,
            state_dir: &state_dir,
            slots_free: slots_total.saturating_sub(u32::try_from(running).unwrap_or(u32::MAX)),
            slots_total,
            ledger: &ledger,
            project: &project,
            root: composition.root(),
            prices: &prices,
            models: &models,
            now,
        })
    };
    let mut out = std::io::stdout().lock();
    let live = out.is_terminal();
    let outcome = if let Some(dashboard) = dashboard {
        dashboard::serve(
            &mut out,
            dashboard,
            lan_address(),
            gatherer,
            &state_dir,
            &mut dashboard::Forever,
        )
    } else if live {
        let mut terminal = Terminal::enter();
        let width = terminal.width();
        let outcome = serve(&mut out, gatherer, &state_dir, width, Some(&mut terminal));
        drop(terminal);
        outcome
    } else {
        serve(&mut out, gatherer, &state_dir, DEFAULT_WIDTH, None)
    };
    match outcome {
        Ok(()) => OK,
        Err(message) => {
            eprintln!("zaf {path}: {message}");
            BACKEND
        }
    }
}

/// This machine's ledger, brought up to date from the project's transcripts. A ledger that
/// cannot be read shows no cost: the frame is still worth drawing.
fn ledger_rows(composition: &Composition) -> Vec<LedgerRow> {
    let _ = super::report::ingest_ledger(composition);
    zaf_home()
        .and_then(|home| Ledger::open(&home.join(LEDGER_FILE)).ok())
        .and_then(|ledger| ledger.rows().ok())
        .unwrap_or_default()
}

// The terminal.

/// The terminal while the hub is up: raw keys so Ctrl-C arrives as a byte, the alternate screen
/// and no cursor. Dropping it puts all three back, which is how Ctrl-C ends the hub with exit 0.
struct Terminal {
    saved: Option<String>,
    keys: Option<mpsc::Receiver<u8>>,
}

impl Terminal {
    fn enter() -> Self {
        let saved = stty(&["-g"]).filter(|saved| !saved.is_empty());
        let keys = saved.as_ref().and_then(|_| {
            stty(&["-isig", "-icanon", "-echo", "min", "1"])?;
            let (send, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let mut byte = [0_u8; 1];
                let mut stdin = std::io::stdin().lock();
                while stdin.read(&mut byte).is_ok_and(|read| read == 1) {
                    if matches!(byte[0], 0x03 | b'q') {
                        let _ = send.send(byte[0]);
                        break;
                    }
                }
            });
            Some(receive)
        });
        // Only a terminal that can be put back gets the alternate screen.
        if keys.is_some() {
            print!("\x1b[?1049h\x1b[?25l");
            let _ = std::io::stdout().flush();
        }
        Self { saved, keys }
    }

    fn width_from_stty() -> Option<usize> {
        let size = stty(&["size"])?;
        size.split_whitespace().nth(1)?.parse().ok()
    }
}

impl Pace for Terminal {
    fn now(&mut self) -> SystemTime {
        SystemTime::now()
    }

    fn width(&mut self) -> usize {
        Self::width_from_stty().unwrap_or(DEFAULT_WIDTH)
    }

    fn wait(&mut self) -> bool {
        match &self.keys {
            Some(keys) => match keys.recv_timeout(POLL) {
                Ok(_) => false,
                Err(RecvTimeoutError::Timeout) => true,
                // stdin closed: there is no key to wait for, so wait on the clock.
                Err(RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(POLL);
                    true
                }
            },
            None => {
                std::thread::sleep(POLL);
                true
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.keys.is_some() {
            print!("\x1b[?25h\x1b[?1049l");
            let _ = std::io::stdout().flush();
        }
        if let Some(saved) = &self.saved {
            let _ = stty(&[saved]);
        }
    }
}

/// Runs `stty` on this terminal; its output when it succeeded.
fn stty(args: &[&str]) -> Option<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests;
