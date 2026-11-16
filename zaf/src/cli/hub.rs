//! `zaf hub` and `zaf hub report`, and the reports the other verbs leave (docs/zaf-actions.md §5).
//!
//! `zaf hub` draws the frame. In a terminal it then waits: a new report redraws it, between
//! reports only the clocks move, and Ctrl-C puts the terminal back and exits 0. Piped or
//! redirected it draws one frame and exits. `zaf hub report` is the profiles' `PostToolUse` hook:
//! it reads the tool call on stdin, leaves one report and prints nothing, whatever it is given.
//! A verb that changes state leaves its own report once it has run; a report that cannot be written
//! never fails or changes the verb.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, SystemTime};

use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, HubAction, Verb};
use crate::adapters::process;
use crate::app::actions::SliceRecords;
use crate::app::check::Target;
use crate::app::guard;
use crate::app::hub::guard::Guard;
use crate::app::hub::link as hub_link;
use crate::app::hub::lock::{HubLock, LockError, claim};
use crate::app::hub::{
    HubError, HubInputs, Liveness, Mark, PROGRAMMER, Places, Report, Snapshot, TEAM, ToolCall,
    Watch, Watched, draw, gather, lan_address, lan_url, record, task_in_workspace,
};
use crate::app::projects::Projects;
use crate::app::report::PriceTable;
use crate::app::report::ledger::{LEDGER_FILE, Ledger, LedgerRow};
use crate::app::schedule::gather::gather_for;
use crate::app::schedule::request;
use crate::app::schedule::shadow;
use crate::app::spawn::watch::Watches;
use crate::composition::{Composition, zaf_home};
use crate::domain::TicketId;

/// How much of a hook's payload is read: a tool call with its output can be large, and the hook
/// needs only its first fields.
const MAX_PAYLOAD: u64 = 1 << 20;

/// How often the live hub looks at the reports file and the keyboard. Only a file's metadata is
/// read: the board is read when the file has changed.
const POLL: Duration = Duration::from_millis(250);

/// The width of a frame when the terminal does not say.
const DEFAULT_WIDTH: usize = 100;

mod dashboard;
mod service;
pub mod tick;
pub mod watch;

/// Runs `zaf hub`, `zaf hub --port <n>`, `zaf hub report` or `zaf hub service …`.
pub fn run(cli: &Cli, action: Option<&HubAction>, port: Option<u16>, read_only: bool) -> u8 {
    match action {
        Some(HubAction::Report) => hook(),
        Some(HubAction::Link) => link(&cli.verb.path()),
        Some(HubAction::Service { action }) => service::run(action, &cli.verb.path()),
        None => show(cli, port, read_only),
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

// The link.

/// `zaf hub link`: asks the running hub for its control link and prints it. Inside an instance, with
/// no hub running, or with a hub that gives none, it prints no key and exits 1.
fn link(path: &str) -> u8 {
    let Some(home) = zaf_home() else {
        eprintln!("zaf {path}: this machine has no zaf home, so no hub runs");
        return REFUSED;
    };
    let me = hub_link::judge_over(home.clone())(std::process::id());
    match hub_link::fetch(&home, &me) {
        Ok(link) => {
            println!("{link}");
            OK
        }
        Err(error @ hub_link::LinkError::Io(_)) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
    }
}

// The reports the verbs leave.

/// What a verb that changed something reports: on which task and in which words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The task it is about, when the verb names one.
    pub task: Option<TicketId>,
    /// What happened, in general words.
    pub what: &'static str,
    /// Whether a refusal (exit 1) still counts: a check that finds a failure has still run.
    pub counts_when_refused: bool,
}

impl Event {
    fn done(task: impl Into<Option<TicketId>>, what: &'static str) -> Option<Self> {
        Some(Self {
            task: task.into(),
            what,
            counts_when_refused: false,
        })
    }

    fn check(task: Option<TicketId>, what: &'static str) -> Option<Self> {
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
            super::BoardAction::File { .. } => Event::done(None, "task filed"),
            // A heartbeat on the ticket and a listing change nothing the hub shows.
            super::BoardAction::Ready
            | super::BoardAction::Report { .. }
            | super::BoardAction::Handoff { .. }
            | super::BoardAction::Lint { .. } => None,
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
                .and_then(|word| word.trim_start_matches('#').parse::<TicketId>().ok());
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
/// one. A failure to write is dropped. The same event asks the scheduler that runs for the project
/// for a tick ([`asker_of`]), so what it changed is acted on at once; with no scheduler nothing is
/// asked. A request that cannot be written is one line on stderr, `zaf <verb>: the running
/// scheduler (…) was not asked for a tick for <who>: <why>; its timer picks the change up at its
/// next tick`: what the verb did stands, and its exit is its own.
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
    let who = asker_of(verb, &event);
    if let Err(not_asked) = request::ask(state_dir, &who, now, &process::alive) {
        eprintln!("zaf {}: {}", verb.path(), not_asked.left_to_the_timer(&who));
    }
    true
}

/// Who an event's request for a tick names: `ruling #45`, `question on #7`, else what happened and
/// the task, `claimed #7`.
pub fn asker_of(verb: &Verb, event: &Event) -> String {
    match verb {
        Verb::Board {
            action: super::BoardAction::Ruling { question, .. },
        } => format!("ruling {question}"),
        Verb::Board {
            action: super::BoardAction::Question { on, .. },
        } => format!("question on {on}"),
        _ => match event.task {
            Some(task) => format!("{} {task}", event.what),
            None => event.what.to_owned(),
        },
    }
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
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    W: Write,
{
    serve_live(out, gather, |_| Ok(()), state_dir, width, pace)
}

/// [`serve`], with `live` bringing the snapshot's slots and claims up to the process table before
/// each redraw that is not a report's, so an instance that ended is not drawn as running.
pub fn serve_live<G, L, W>(
    out: &mut W,
    gather: G,
    live: L,
    state_dir: &Path,
    width: usize,
    pace: Option<&mut dyn Pace>,
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    L: FnMut(&mut Snapshot) -> Result<(), HubError>,
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
    let (mut watch, frame) = Watch::start(gather, live, pace.now(), Mark::read(state_dir), width)
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
fn show(cli: &Cli, port: Option<u16>, read_only: bool) -> u8 {
    let path = cli.verb.path();
    let home = zaf_home();
    // A hub already serving on this machine is named before the port is tried, since its port
    // would only say "in use". The terminal frame has no port and asks nothing of the lock.
    if let (Some(home), Some(_)) = (home.as_deref(), port)
        && let Some(first) = HubLock::held(home, &process::alive)
    {
        refuse_second(&path, &first);
        return REFUSED;
    }
    // The port is taken next: one in use is refused before anything is read.
    let dashboard = match port.map(|port| dashboard::bind(port, &path)).transpose() {
        Ok(dashboard) => dashboard,
        Err(code) => return code,
    };
    // The key is made once the page is to be served, before anything is read or served: a hub that
    // cannot get one from the system serves no controls, and so does not start.
    let guard = match dashboard
        .as_ref()
        .map(|_| Guard::start(read_only))
        .transpose()
    {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!(
                "zaf {path}: cannot make the hub's key from the system's random source: {error}"
            );
            return BACKEND;
        }
    };
    // The lock names the port the socket got, which is a free one for port 0. It is given up
    // when the hub ends; a hub that is killed leaves a stale one, which the next start takes over.
    let lock = match take_lock(
        home.as_deref(),
        dashboard.as_ref().map(crate::app::hub::Dashboard::port),
    ) {
        Ok(lock) => lock,
        Err(LockError::Held(first)) => {
            refuse_second(&path, &first);
            return REFUSED;
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
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
    let refresh = |snapshot: &mut Snapshot| {
        let read = Liveness::read(&state_dir, slots_total, &snapshot.claims, &process::alive)?;
        snapshot.apply(&read);
        Ok(())
    };
    // A served hub draws its first frame from the ledger as it stands and fills it once its URL
    // is out (`fill`): on a fresh zaf home the fill reads every transcript on the machine. The
    // terminal frame fills it at its first read, as every later read of either does.
    let filled = Cell::new(dashboard.is_none());
    let fill = || {
        filled.set(true);
        super::report::ingest_ledger(&composition).is_ok_and(|wrote| wrote > 0)
    };
    let gatherer = |now: SystemTime| {
        let ledger = if filled.get() {
            ledger_rows(&composition)
        } else {
            stored_ledger_rows()
        };
        // Where the core places each ticket, as the tick gathers them; a gather that fails leaves
        // the labels' word.
        let places: Option<Places> =
            gather_for(&composition, board.as_ref())
                .ok()
                .flatten()
                .map(|gathered| {
                    shadow::places(&gathered, now)
                        .into_iter()
                        .map(|placed| (placed.ticket, placed.place))
                        .collect()
                });
        gather(&HubInputs {
            board: board.as_ref(),
            forge: forge.as_ref(),
            vcs: vcs.as_ref(),
            records: &records,
            state_dir: &state_dir,
            alive: &process::alive,
            slots_total,
            ledger: &ledger,
            project: &project,
            root: composition.root(),
            prices: &prices,
            models: &models,
            now,
            places: places.as_ref(),
        })
    };
    let mut out = std::io::stdout().lock();
    let live = out.is_terminal();
    // The watches the page starts: they end with this value, which lives as long as the hub runs.
    let watches = Watches::new();
    let outcome = if let (Some(dashboard), Some(guard)) = (dashboard, guard) {
        // Only a terminal is handed the key. A hub started by the service, or with its output
        // piped, writes to a journal or a file, which must never hold it.
        let serving = dashboard::Serving {
            address: lan_address(),
            guard,
            controls: dashboard::controls(home.clone(), watches.handle()),
            show_link: live,
            // Only a hub that holds the lock answers `zaf hub link`: the socket is the lock's.
            link_home: lock.as_ref().and(home.clone()),
            fill: Some(&fill),
        };
        // Every listed project, and this one even when the list does not have it yet.
        let mut listed = || {
            let mut folders: Vec<std::path::PathBuf> = home
                .as_deref()
                .map(Projects::read)
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| !entry.missing)
                .map(|entry| entry.folder)
                .collect();
            if !folders.iter().any(|folder| folder == composition.root()) {
                folders.push(composition.root().to_owned());
            }
            folders.iter().map(|folder| Watched::of(folder)).collect()
        };
        dashboard::serve_watching(
            &mut out,
            dashboard,
            serving,
            dashboard::machine_reader(home.clone()),
            gatherer,
            &state_dir,
            &mut dashboard::Forever,
            &mut listed,
            &process::alive,
        )
    } else if live {
        let mut terminal = Terminal::enter();
        let width = terminal.width();
        let outcome = serve_live(
            &mut out,
            gatherer,
            refresh,
            &state_dir,
            width,
            Some(&mut terminal),
        );
        drop(terminal);
        outcome
    } else {
        serve(&mut out, gatherer, &state_dir, DEFAULT_WIDTH, None)
    };
    // Every watch ends before the hub gives up its own lock.
    drop(watches);
    if let (Some(lock), Some(home)) = (&lock, home.as_deref()) {
        lock.release(home);
    }
    match outcome {
        Ok(()) => OK,
        Err(message) => {
            eprintln!("zaf {path}: {message}");
            BACKEND
        }
    }
}

/// The lock for a served hub on `port`; none for the terminal frame, and none where there is
/// no zaf home or no way to read this process, which leaves the hub unlocked as before.
fn take_lock(home: Option<&Path>, port: Option<u16>) -> Result<Option<HubLock>, LockError> {
    let (Some(home), Some(me)) = (home, process::this()) else {
        return Ok(None);
    };
    claim(home, port, &me, &process::alive)
}

/// Says where the hub that already serves is, on stderr.
fn refuse_second(path: &str, first: &HubLock) {
    eprintln!(
        "zaf {path}: a hub is already serving at {} (pid {})",
        lan_url(lan_address(), first.port),
        first.pid
    );
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

/// This machine's ledger as it stands: nothing is ingested and nothing is made, so it reads at
/// once. A ledger that is not there, or cannot be read, shows no cost.
fn stored_ledger_rows() -> Vec<LedgerRow> {
    zaf_home()
        .and_then(|home| Ledger::rows_if_present(&home.join(LEDGER_FILE)).ok())
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
