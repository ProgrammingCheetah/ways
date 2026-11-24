//! What a key asks the chair to read beside a project's regular read (D8, D9): an epic's body and
//! its release's objectives and design (Enter on an epic), and a ticket's reports and the tail of
//! its phase run's log (`l`, and Live while it follows a ticket).
//!
//! A [`Look`] is read off the draw loop and comes back as a [`Looked`]: an epic on the project's
//! gatherer thread ([`super::gatherer::Gatherer::run`]), as every board read is, a ticket (the
//! state folder's files only) on a thread of its own, since a project's read can take half a
//! minute. Everything read is made
//! [`printable`] on the way in, as the regular read's strings are. Nothing here writes.
//!
//! A release document is shown without its signature block ([`document`]). A phase run's log is
//! found from its run folder: the log the folder's `output` names when it names one, else the log
//! the runner opened in the second the run started (`<stamp>_<action>.log`, [`log_names`]), the
//! one that names the ticket first when two runs of the action started together. A `claude -p`
//! phase writes its log only at its end, so a live run with no log lines yet is "working". A log is
//! read only when it is a regular file inside the project's `.zaf/logs/` once its links are
//! followed, and only its last [`MOST_LOG_BYTES`] for the tail.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::ReleaseVersion;
use crate::app::hub::{Report, reports_file};
use crate::app::plan::show::{PlanView, ShowError, show, show_before};
use crate::app::plan::verify::verify;
use crate::app::release::ReleaseFolder;
use crate::app::schedule::record::{OUTPUT_FILE, PHASES_DIR, read_run, read_text};
use crate::composition::{Composition, STATE_DIR};
use crate::domain::plan::shape::ShapeFailure;
use crate::domain::schedule::RunRecord;
use crate::domain::{DocumentSignature, TicketId, printable};
use crate::manifest::Manifest;
use crate::ports::Holder;

/// The folder of the state folder the action runner writes its logs to.
const LOGS_DIR: &str = "logs";

/// The most reports of a ticket a look keeps, the newest last.
pub const MOST_REPORTS: usize = 8;

/// The most lines of a log's tail a look keeps.
pub const MOST_LOG_LINES: usize = 40;

/// What a key asks to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// An epic's body and its release's documents.
    Epic(TicketId),
    /// A ticket's reports and its phase run's log.
    Ticket(TicketId),
    /// An epic's graph as its tickets read now, and whether it holds to O3's shape.
    Graph(TicketId),
}

/// What a look read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Looked {
    /// An epic's body and its release's documents.
    Epic(EpicRead),
    /// A ticket's reports and its phase run's log.
    Ticket(TicketRead),
    /// An epic's graph and its shape check.
    Graph(Box<GraphRead>),
}

/// An epic's graph as `d` reads it (D23): the view of its tickets and the shape check beside it.
/// Each half fails on its own, with the words of why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRead {
    /// The epic.
    pub epic: TicketId,
    /// The graph as the tickets read now, or why it is not shown.
    pub view: Result<PlanView, String>,
    /// The graph as the plan records wrote it, or why it is not shown; an epic filed before 0.6.0
    /// has the same view in both, of kind `Older`.
    pub before: Result<PlanView, String>,
    /// What breaks O3's rules, none for a good graph, or why it was not checked.
    pub shape: Result<Vec<ShapeFailure>, String>,
}

/// An epic as Enter reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicRead {
    /// The epic.
    pub epic: TicketId,
    /// Its body as it reads now, or why it could not be read.
    pub body: Result<String, String>,
    /// Its release's documents, when its body names a release.
    pub release: Option<ReleaseRead>,
}

/// A release's documents, read from its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRead {
    /// The version.
    pub version: ReleaseVersion,
    /// The release folder, relative to the project's root.
    pub folder: String,
    /// `objectives.md`, else `scope.md`; none when the folder holds neither.
    pub objectives: Option<ReleaseDocument>,
    /// `design.md`; none when the folder holds none.
    pub design: Option<ReleaseDocument>,
}

/// A release document as the chair shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDocument {
    /// Its file's name: `objectives.md`, `scope.md`, `design.md`.
    pub name: String,
    /// Its text without its signature block, printable.
    pub text: String,
}

/// A ticket as `l` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketRead {
    /// The ticket.
    pub ticket: TicketId,
    /// The hub's reports on it, the newest last, at most [`MOST_REPORTS`].
    pub reports: Vec<ReportLine>,
    /// Its newest phase run and the tail of that run's log; none when it has run no phase.
    pub run: Option<RunTail>,
}

/// One of the hub's reports, as the hub recorded it: a few general words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLine {
    /// When it was made.
    pub at: SystemTime,
    /// Who made it: one word.
    pub worker: String,
    /// What happened.
    pub what: String,
}

/// A phase run and the tail of its log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTail {
    /// The action it runs: `implement`, `review`.
    pub action: String,
    /// Whether it runs now.
    pub live: bool,
    /// When it started.
    pub started_at: SystemTime,
    /// Its exit code, once it ended with one.
    pub exit: Option<i32>,
    /// Its log, when one was found.
    pub log: Option<PathBuf>,
    /// The log's last lines that hold words, printable, at most [`MOST_LOG_LINES`].
    pub lines: Vec<String>,
}

impl RunTail {
    /// Whether it runs and its log holds nothing yet: a `claude -p` phase at work.
    pub fn working(&self) -> bool {
        self.live && self.lines.is_empty()
    }
}

/// Reads `look` in the project at `root`; `alive` says whether a recorded process still runs.
pub fn read(root: &Path, look: Look, alive: &dyn Fn(&Holder) -> bool) -> Looked {
    match look {
        Look::Epic(epic) => Looked::Epic(read_epic(root, epic)),
        Look::Ticket(ticket) => Looked::Ticket(read_ticket(&root.join(STATE_DIR), ticket, alive)),
        Look::Graph(epic) => Looked::Graph(Box::new(read_graph(root, epic))),
    }
}

/// The graph of the epic `epic` of the project at `root`: [`show`] and [`verify`] over its board,
/// every word made [`printable`].
pub fn read_graph(root: &Path, epic: TicketId) -> GraphRead {
    let failed = |why: String| GraphRead {
        epic,
        view: Err(why.clone()),
        before: Err(why.clone()),
        shape: Err(why),
    };
    let composition = match Composition::load(root) {
        Ok(composition) => composition,
        Err(error) => {
            return failed(printable(&format!("the manifest cannot be read: {error}")));
        }
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return failed(printable(&error.to_string())),
    };
    let shown = |read: Result<PlanView, ShowError>| {
        read.map(|mut view| {
            for node in &mut view.nodes {
                node.title = printable(&node.title);
            }
            view
        })
        .map_err(|error| printable(&error.to_string()))
    };
    let view = shown(show(board.as_ref(), epic));
    let before = shown(show_before(board.as_ref(), epic));
    let shape = composition
        .vcs()
        .map_err(|error| printable(&error.to_string()))
        .and_then(|vcs| {
            verify(
                root,
                composition.manifest(),
                board.as_ref(),
                vcs.as_ref(),
                epic,
            )
            .map_err(|error| printable(&error.to_string()))
        });
    GraphRead {
        epic,
        view,
        before,
        shape,
    }
}

/// The epic `epic` of the project at `root`: its body from the board, and its release's
/// documents from the folder `[release] folder` names.
pub fn read_epic(root: &Path, epic: TicketId) -> EpicRead {
    let composition = match Composition::load(root) {
        Ok(composition) => composition,
        Err(error) => {
            return EpicRead {
                epic,
                body: Err(printable(&format!("the manifest cannot be read: {error}"))),
                release: None,
            };
        }
    };
    let read = composition
        .board()
        .map_err(|error| error.to_string())
        .and_then(|board| board.epic(epic).map_err(|error| error.to_string()));
    let (body, version) = match read {
        Ok(read) => (
            Ok(printable(read.body.as_str())),
            read.release().map(str::to_owned),
        ),
        Err(why) => (Err(printable(&why)), None),
    };
    EpicRead {
        epic,
        body,
        release: version.map(|version| release_read(root, composition.manifest(), &version)),
    }
}

/// The documents of `version`'s release folder under `root`.
pub fn release_read(root: &Path, manifest: &Manifest, version: &str) -> ReleaseRead {
    let folder = ReleaseFolder::of(root, manifest, version);
    let read = |path: PathBuf| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).ok()?;
        Some(document(&name, &text))
    };
    ReleaseRead {
        version: ReleaseVersion::from(printable(version)),
        folder: printable(
            &folder
                .path()
                .strip_prefix(root)
                .unwrap_or(folder.path())
                .display()
                .to_string(),
        ),
        objectives: folder.objectives_document().and_then(read),
        design: Some(folder.design_document())
            .filter(|path| path.is_file())
            .and_then(read),
    }
}

/// The document `name` reads as `text`, without its signature block and the blank lines that
/// open it, printable.
pub fn document(name: &str, text: &str) -> ReleaseDocument {
    let body = DocumentSignature::body(text);
    let body = body.trim_start_matches(['\n', '\r']).trim_end();
    ReleaseDocument {
        name: printable(name),
        text: printable(body),
    }
}

/// The ticket `ticket` of the state folder `state_dir`: the hub's reports on it and its newest
/// phase run with its log's tail.
pub fn read_ticket(
    state_dir: &Path,
    ticket: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> TicketRead {
    let reports = std::fs::read_to_string(reports_file(state_dir))
        .map(|text| reports_of(&text, ticket, MOST_REPORTS))
        .unwrap_or_default();
    TicketRead {
        ticket,
        reports,
        run: newest_run(&state_dir.join(PHASES_DIR), ticket, alive)
            .map(|(folder, run)| run_tail(state_dir, ticket, &folder, &run)),
    }
}

/// The reports on `ticket` in the reports file's `text`, the newest last, at most `most`.
pub fn reports_of(text: &str, ticket: TicketId, most: usize) -> Vec<ReportLine> {
    let mut reports: Vec<ReportLine> = text
        .lines()
        .rev()
        .filter_map(Report::parse)
        .filter(|report| report.task == Some(ticket))
        .take(most)
        .map(|report| ReportLine {
            at: report.at,
            worker: printable(&report.worker),
            what: printable(&report.what),
        })
        .collect();
    reports.reverse();
    reports
}

/// The newest run folder of `ticket` under `dir` and its record; none when it has none, or the
/// folders cannot be read.
fn newest_run(
    dir: &Path,
    ticket: TicketId,
    alive: &dyn Fn(&Holder) -> bool,
) -> Option<(PathBuf, RunRecord)> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let run = read_run(&path, ticket, alive).ok().flatten()?;
            Some((path, run))
        })
        .max_by_key(|(_, run)| run.started_at)
}

/// The run `run` in `folder`, with the tail of its log.
fn run_tail(state_dir: &Path, ticket: TicketId, folder: &Path, run: &RunRecord) -> RunTail {
    let logs = state_dir.join(LOGS_DIR);
    let action = run.action.to_string();
    let named = read_text(&folder.join(OUTPUT_FILE))
        .ok()
        .flatten()
        .and_then(|output| log_named(&output, &logs, &action));
    let log = named.or_else(|| {
        let found: Vec<(PathBuf, String)> = log_names(&action, run.started_at)
            .into_iter()
            .map(|name| logs.join(name))
            .filter_map(|path| {
                let text = read_log(&path, &logs, End::Head)?;
                Some((path, text))
            })
            .collect();
        pick_log(found, ticket)
    });
    let lines = log
        .as_ref()
        .and_then(|log| read_log(log, &logs, End::Tail))
        .map(|text| tail(&text, MOST_LOG_LINES))
        .unwrap_or_default();
    RunTail {
        action: printable(&action),
        live: run.live(),
        started_at: run.started_at,
        exit: run.exit.as_ref().map(|exit| exit.code.get()),
        log,
        lines,
    }
}

/// The most of a log a look reads: its last bytes for the tail, its first for the ticket it
/// names.
pub const MOST_LOG_BYTES: u64 = 64 * 1024;

/// Which end of a log a look reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// Its first [`MOST_LOG_BYTES`]: what a run wrote as it began.
    Head,
    /// Its last, from the first whole line: what it wrote lately.
    Tail,
}

/// At most [`MOST_LOG_BYTES`] of the log at `path`, from its `end`, as text; none unless it is a
/// regular file inside `logs` once every link in its path is followed (a run folder's `output`
/// may name anything, and a FIFO would hold the read for ever).
fn read_log(path: &Path, logs: &Path, end: End) -> Option<String> {
    let logs = logs.canonicalize().ok()?;
    let real = path.canonicalize().ok()?;
    if !real.starts_with(&logs) {
        return None;
    }
    // Opened without waiting (a FIFO swapped in opens at once) and without following a link
    // swapped in, then checked for what it is.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(&real)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let from = match end {
        End::Head => 0,
        End::Tail => meta.len().saturating_sub(MOST_LOG_BYTES),
    };
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.take(MOST_LOG_BYTES).read_to_end(&mut bytes).ok()?;
    if from > 0 {
        // The first line is cut: it starts before the bytes read.
        let whole = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1);
        bytes.drain(..whole);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The log of `action` a run folder's `output` names (`…, log <path>`), when it names one under
/// `logs`.
fn log_named(output: &str, logs: &Path, action: &str) -> Option<PathBuf> {
    output
        .split_whitespace()
        .map(|word| word.trim_end_matches([',', '.', ';', ')']))
        .map(Path::new)
        .find(|path| {
            path.starts_with(logs)
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(&format!("_{action}")))
        })
        .map(Path::to_path_buf)
}

/// The names the runner may have given the log of a run of `action` that started at
/// `started_at`: `<stamp>_<action>.log` and its `.2.log` to `.4.log`, for the start's second and
/// the few seconds around it, the closest first. The stamp is the runner's: UTC to the second,
/// with no `-` or `:`.
pub fn log_names(action: &str, started_at: SystemTime) -> Vec<String> {
    let mut names = Vec::new();
    for offset in [0_i64, 1, -1, 2, 3, -2, 4, 5] {
        let at = if offset >= 0 {
            started_at.checked_add(Duration::from_secs(offset.unsigned_abs()))
        } else {
            started_at.checked_sub(Duration::from_secs(offset.unsigned_abs()))
        };
        let Some(at) = at else { continue };
        let stamp: String = humantime::format_rfc3339_seconds(at)
            .to_string()
            .chars()
            .filter(|c| !matches!(c, '-' | ':'))
            .collect();
        names.push(format!("{stamp}_{action}.log"));
        names.extend((2..=4).map(|attempt| format!("{stamp}_{action}.{attempt}.log")));
    }
    names
}

/// Of the logs `found` (each with its text), closest first, the first that names `ticket`, else
/// the first.
pub fn pick_log(found: Vec<(PathBuf, String)>, ticket: TicketId) -> Option<PathBuf> {
    let number = ticket.get().to_string();
    let names = |text: &str| {
        text.split(|c: char| !c.is_ascii_digit())
            .any(|word| word == number)
    };
    let chosen = found.iter().position(|(_, text)| names(text)).unwrap_or(0);
    found.into_iter().nth(chosen).map(|(path, _)| path)
}

/// The last `most` lines of `text` that hold words, printable, oldest first.
pub fn tail(text: &str, most: usize) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(most)
        .map(|line| printable(line.trim_end()))
        .collect();
    lines.reverse();
    lines
}

#[cfg(test)]
mod tests;
