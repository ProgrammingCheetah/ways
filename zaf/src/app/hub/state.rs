//! One project's whole state as one value that serializes to JSON.
//!
//! [`gather_project`] reads what `zaf status`, `zaf epic list` and `zaf report --epics` read:
//! the open epics with their stage, their tasks done by group and their cost; the queue of
//! ready, claimed, blocked and question tasks, each state where the core places the task when it
//! does ([`shown_state`]); the instances that run now. It only reads. A
//! board that cannot be reached (no token, the instance down) is not an error: the project
//! comes back with its [`reason`](ProjectState::reason) in plain words and what could still be
//! read. [`gather_state`] is the same read over ports, which a test or another caller wires
//! itself. [`gather_machine`] reads every project on the zaf home's list into one
//! [`MachineState`] headed by the machine line (the [`machine`] module).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::gather::{LineState, Places, dollars_by_epic, shown_state, tally};
use super::reports::last_report;
use crate::adapters::process;
use crate::app::actions::{SliceRecords, derive_phase, gather_phase_facts};
use crate::app::profile;
use crate::app::release::{self, DueRelease};
use crate::app::report::PriceTable;
use crate::app::report::ledger::{LEDGER_FILE, Ledger, LedgerRow};
use crate::app::schedule::gather::{Gathered, gather_for};
use crate::app::schedule::shadow;
use crate::app::spawn::runs::{self, Run};
use crate::composition::{Composition, zaf_home};
use crate::domain::schedule::WaitingOn;
use crate::domain::{Epic, EpicStage, EpicTask, Instance, TaskTemplate, TicketId, TicketState};
use crate::ports::{Board, ClaimRecord, Forge, Holder, NewQuestion, PortError, Vcs};

mod line;
pub mod machine;

pub use machine::{MachineLine, MachineState, gather_machine, gather_one};

/// A project's state as the hub reads it: serialize it with `serde_json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProjectState {
    /// The project's name, from its manifest; the root's folder name when the manifest cannot
    /// be read.
    pub project: String,
    /// The project's root.
    pub root: PathBuf,
    /// Whether the project's folder, or its manifest, is gone from the machine: the entry is
    /// kept on the list, nothing of it is read and no board is asked.
    pub missing: bool,
    /// When it was gathered, RFC 3339.
    pub gathered_at: String,
    /// Why part of it is missing, in plain words; none when everything was read. A board that
    /// cannot be reached leaves `epics` and `queue` empty and says so here.
    pub reason: Option<String>,
    /// The open epics, in issue order.
    pub epics: Vec<EpicState>,
    /// The tasks ready, claimed, blocked or waiting on a question, by epic and issue order.
    pub queue: Vec<QueueTask>,
    /// The questions open on the board, in issue order: what waits on the Engineer's ruling.
    pub questions: Vec<QuestionState>,
    /// What keeps the project from running, each with the command that fixes it; empty when
    /// nothing does.
    pub problems: Vec<Problem>,
    /// The instances running now.
    pub instances: Vec<InstanceState>,
    /// The claims held by an instance this machine launched that no longer runs, as `zaf status`
    /// counts them orphaned: the next tick releases each, unless its pull request or a running
    /// phase holds it.
    pub orphaned: Vec<OrphanedClaim>,
    /// The releases whose objectives or design are due, oldest first, each with the command that
    /// runs the phase.
    pub releases_due: Vec<ReleaseDueState>,
    /// Whether the scheduler (`zaf spawn --watch`) runs, from its lock; absent for a project that
    /// cannot be read.
    pub watch: Option<WatchState>,
    /// The last tick; absent until the tick record lands.
    pub last_tick: Option<TickState>,
    /// What only `zaf status` showed before: the slots, the guard, the tasks the spawner skips or
    /// can never start, and the next version; read in the same gather, never by a second read.
    pub status: StatusState,
}

/// What `zaf status` shows beside the board, read in the hub's own gather (release 0.5.3, F14).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct StatusState {
    /// The manifest's slots and how many of them are in use; none when the manifest cannot be
    /// read.
    pub slots: Option<SlotsState>,
    /// The guard as `zaf guard` last wrote it; none when it never ran or its file does not read.
    pub guard: Option<crate::app::guard::GuardState>,
    /// The tasks the board lists ready whose body does not follow the template, which the
    /// spawner skips.
    pub unparsed: Vec<UnparsedTask>,
    /// The blocker cycles among the open tasks of the open epics, each from its lowest task: the
    /// first waits on the second, the last on the first.
    pub cycles: Vec<Vec<u64>>,
    /// The blockers open tasks name that never close: not an issue, or closed as not planned.
    pub dead_blockers: Vec<crate::app::spawn::blockers::DeadLine>,
    /// The version `zaf begin` would begin next (`release::next_version`); none when the
    /// manifest cannot be read.
    pub next_version: Option<String>,
}

/// The slots: the manifest's `[spawn] slots` and how many are in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SlotsState {
    /// The manifest's `[spawn] slots`.
    pub total: u32,
    /// The instances that run and the live model runs of the phase run folders, as the tick's
    /// budget counts them.
    pub in_use: u32,
}

/// A task the board lists ready whose body does not follow the template.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UnparsedTask {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// One line per problem, as `zaf board lint` prints them.
    pub problems: Vec<String>,
}

/// What a ticket's runs cost in the ledger, as of its last ingest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct TicketCost {
    /// The tokens of every run that named it: fresh input, cache writes, cache reads and output.
    pub tokens: u64,
    /// The seconds those runs took, each from its transcript's first time to its last.
    pub seconds: u64,
}

/// An open epic.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EpicState {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Where it stands: unsigned, signed, planned or challenged.
    pub stage: EpicStage,
    /// The version its body's `Release:` line names; `null` when the body has none.
    pub release: Option<String>,
    /// Where a person opens its issue; read for an unsigned epic only, the one the Engineer signs.
    pub url: Option<String>,
    /// Its tasks done.
    pub done: u32,
    /// Its tasks that count, done ones included.
    pub total: u32,
    /// What its runs cost in the ledger, estimated, in dollars.
    pub dollars: f64,
    /// Its tasks done of all, by `group:<name>` label.
    pub groups: Vec<GroupState>,
    /// Its own line colour: `question` while a question is open on the epic itself, else `done`
    /// when every task that counts is done, else `working` when any is, else `waiting_tick` when
    /// any has started, else `not_started` (the Engineer's ruling on #128).
    pub line_state: LineState,
    /// The first question open on the epic itself.
    pub line_question: Option<LineQuestion>,
    /// Its tasks that count (open or done, never closed as not planned), in id order: what the
    /// page draws its line from.
    pub tasks: Vec<LineTask>,
}

/// A question open on a ticket, as a line shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineQuestion {
    /// The question's issue number.
    pub number: u64,
    /// The whole question, without the `Raised on` line.
    pub text: String,
}

/// A task of an epic as its line shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineTask {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Its size, `XS` to `E`, from its body's template; none when the body has none or is not a
    /// task.
    pub size: Option<&'static str>,
    /// Its line colour.
    pub state: LineState,
    /// The phase it is in, by name; none for a done task, one the core does not place, or one
    /// with every phase ended.
    pub phase: Option<String>,
    /// The phases of its type that have ended.
    pub phases_done: u32,
    /// All the phases of its type; `0` with `phases_done` for a task the core does not place.
    pub phases_total: u32,
    /// Each blocker its `Blocked by:` line names.
    pub blocked_by: Vec<LineBlocker>,
    /// The first question open on it.
    pub question: Option<LineQuestion>,
    /// Where a person opens its issue.
    pub issue_url: Option<String>,
    /// What the core says it waits on, from the same placement its colour comes from; empty when
    /// the core places it anywhere but waiting, `null` when the core does not place it.
    pub waits: Option<Vec<WaitState>>,
    /// What its runs cost, as the ledger holds them; none when no run named it.
    pub cost: Option<TicketCost>,
}

/// One thing a ticket waits on, as the core names it ([`WaitingOn`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WaitState {
    /// What kind of thing it waits on.
    pub on: WaitKind,
    /// The ticket, question or epic it is, for those kinds.
    pub number: Option<u64>,
    /// The commit whose checks it waits on, for a check or the base.
    pub commit: Option<String>,
}

/// What kind of thing a ticket waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    /// Its epic, not signed or not challenged yet.
    Epic,
    /// A question open on it or on its epic: the Engineer's ruling.
    Question,
    /// Another ticket it depends on, not done.
    Ticket,
    /// The checks at its head.
    Check,
    /// The merge of another ticket, which merges first.
    Merge,
    /// The checks at the integration branch's head.
    Base,
}

impl From<&WaitingOn> for WaitState {
    fn from(waiting_on: &WaitingOn) -> Self {
        let (on, number, commit) = match waiting_on {
            WaitingOn::Epic(epic) => (WaitKind::Epic, Some(epic.get()), None),
            WaitingOn::Question(question) => (WaitKind::Question, Some(question.get()), None),
            WaitingOn::Ticket(ticket) => (WaitKind::Ticket, Some(ticket.get()), None),
            WaitingOn::Check(ticket, commit) => (
                WaitKind::Check,
                Some(ticket.get()),
                Some(commit.to_string()),
            ),
            WaitingOn::Merge(ticket) => (WaitKind::Merge, Some(ticket.get()), None),
            WaitingOn::Base(commit) => (WaitKind::Base, None, Some(commit.to_string())),
        };
        Self { on, number, commit }
    }
}

impl WaitState {
    /// Whether it is the question `question`.
    pub fn is_question(&self, question: u64) -> bool {
        self.on == WaitKind::Question && self.number == Some(question)
    }
}

/// A claim held by an instance that has ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OrphanedClaim {
    /// The task it holds.
    pub task: u64,
    /// The instance that held it, as `<role> instance <n> on #<epic>`.
    pub instance: String,
}

/// A release whose objectives or design are due.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReleaseDueState {
    /// The version.
    pub version: String,
    /// The phase due: `objectives` or `design`.
    pub phase: String,
    /// The command that runs it: `zaf begin --version <version>`.
    pub command: String,
}

impl From<&DueRelease> for ReleaseDueState {
    fn from(due: &DueRelease) -> Self {
        Self {
            version: due.version.clone(),
            phase: due.phase.name().to_owned(),
            command: due.command(),
        }
    }
}

/// A blocker a task's `Blocked by:` line names.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineBlocker {
    /// Its issue number.
    pub number: u64,
    /// Its title; none when the board does not hold it.
    pub title: Option<String>,
    /// Its line colour when it is a task of an open epic, else `done` or `not_started` from its
    /// standing.
    pub state: LineState,
}

/// An open question: a ticket the Engineer rules on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuestionState {
    /// Its issue number.
    pub number: u64,
    /// The issue it was raised on, when its body says.
    pub raised_on: Option<u64>,
    /// The whole question, without the `Raised on` line.
    pub text: String,
    /// Whether it was raised as a door, a one-way decision.
    pub door: bool,
    /// Where a person opens it.
    pub url: Option<String>,
}

/// What keeps a project from running, with the command that fixes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Problem {
    /// What kind of fault it is.
    pub kind: ProblemKind,
    /// What is wrong, in plain words.
    pub detail: String,
    /// The command that fixes it, as a person runs it; none when no command does.
    pub fix: Option<String>,
    /// The zaf verb that fixes it, when one does: what the chair runs, never the text above.
    #[serde(skip)]
    pub verb: Option<FixVerb>,
}

/// A zaf verb that fixes a problem, with its words fixed here: the chair runs it in the project's
/// root and builds its arguments from this, so no text a board or a file gave is ever run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixVerb {
    /// `zaf doctor`: says what is wrong with the project.
    Doctor,
    /// `zaf profile init`: makes the project's stale profiles again.
    ProfileInit,
}

impl FixVerb {
    /// Its arguments after `zaf`.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::Doctor => &["doctor"],
            Self::ProfileInit => &["profile", "init"],
        }
    }
}

/// The kinds of fault that keep a project from running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// The manifest cannot be read.
    Manifest,
    /// The variable that holds the board's token is not set.
    Token,
    /// The board cannot be reached.
    Board,
    /// Generated profiles are older than the manifest or zaf, or have no stamp: one problem
    /// for the project, however many profiles.
    Profile,
    /// The project's folder, or its manifest, is gone from the machine.
    Missing,
    /// An open epic whose every task was dropped: one problem for each such epic. The Engineer
    /// decides whether to file new work or drop what it blocks, so it has no fix and no verb.
    EveryTaskDropped,
}

impl Problem {
    /// The problem of an open epic whose every task was dropped; none for any other epic.
    pub fn every_task_dropped(epic: &Epic) -> Option<Self> {
        epic.every_task_dropped().then(|| Self {
            kind: ProblemKind::EveryTaskDropped,
            detail: format!("epic {} {}: every task dropped", epic.id, epic.title),
            fix: None,
            verb: None,
        })
    }
}

/// A group of an epic's tasks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GroupState {
    /// The name in the label.
    pub name: String,
    /// Its tasks done.
    pub done: u32,
    /// Its tasks that count, done ones included.
    pub total: u32,
}

/// A task in the queue.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueueTask {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Its epic.
    pub epic: u64,
    /// Its state: ready, claimed, blocked or question.
    pub state: TicketState,
    /// Its group, if it has one.
    pub group: Option<String>,
    /// The instance holding it, for a claimed task.
    pub instance: Option<String>,
    /// Its Team Phase, for a claimed task whose phase could be read.
    pub phase: Option<String>,
    /// Where a person opens its issue.
    pub issue_url: Option<String>,
    /// Its pull request's number, once one is recorded.
    pub pull_request: Option<u64>,
    /// Where a person opens its pull request.
    pub pull_request_url: Option<String>,
}

/// An instance that runs now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstanceState {
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// Its role.
    pub role: String,
    /// The task it works on; none for one launched with none (planning, the challenger).
    pub task: Option<u64>,
    /// The ticket it is named by: its task, else its epic for an instance launched with no task
    /// (planning, the challenger, release); none when neither could be told.
    pub number: Option<u64>,
    /// That ticket's title, whole and as the board read it; none when the board could not be read
    /// or does not hold the ticket.
    pub title: Option<String>,
    /// The task's Team Phase, when it could be read.
    pub phase: Option<String>,
    /// Seconds since it started.
    pub run_seconds: Option<u64>,
    /// The word of its last reported tool call, `reading`, `writing`, `testing`.
    pub last_tool_call: Option<String>,
}

/// Whether the project's scheduler (`zaf spawn --watch`) runs, read from its lock
/// (`.zaf/scheduler.lock`, [`crate::app::schedule::lock`]), and when the last tick began, read from
/// the trace's last line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WatchState {
    /// Whether a scheduler runs now: its lock names a process that is still alive.
    pub running: bool,
    /// Its process id, while it runs.
    pub pid: Option<u32>,
    /// Since when it runs, RFC 3339, while it runs; a restart into a new binary keeps it.
    pub since: Option<String>,
    /// The zaf version it runs, while it runs.
    pub zaf: Option<String>,
    /// The milestone it was started for (`--version`), while it runs.
    pub version: Option<String>,
    /// When the last tick began, RFC 3339, whoever ran it; none before the first.
    pub last_tick: Option<String>,
}

impl WatchState {
    /// The scheduler of the state folder `state_dir`, as its lock and the trace say now.
    pub fn read(state_dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Self {
        let holding = crate::app::schedule::lock::running(state_dir, alive);
        let last_tick = crate::app::schedule::ticks::last_at(state_dir)
            .map(|at| humantime::format_rfc3339_seconds(at).to_string());
        Self {
            running: holding.is_some(),
            pid: holding.as_ref().map(|holding| holding.pid),
            since: holding.as_ref().map(|holding| holding.since.clone()),
            zaf: holding.as_ref().map(|holding| holding.zaf.clone()),
            version: holding.map(|holding| holding.version),
            last_tick,
        }
    }
}

/// The last tick. Its shape is the tick record's, which has not landed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TickState {
    /// When it ran, RFC 3339.
    pub at: String,
}

/// The open epics and the open questions of one board, each read once: [`gather_project`] reads
/// them at its start and hands them to the placement's gather and to [`gather_state`], so a project
/// read asks the board for each once.
pub struct BoardLists {
    /// The board's open epics, as its read answered.
    pub epics: Result<Vec<Epic>, PortError>,
    /// The board's open questions, as its read answered.
    pub questions: Result<Vec<crate::domain::Ticket>, PortError>,
}

impl BoardLists {
    /// One read of each from `board`.
    pub fn read(board: &dyn Board) -> Self {
        Self {
            epics: board.epics(),
            questions: board.questions(),
        }
    }

    /// The lists as the placement's gather takes them: a board that cannot list its epics at all
    /// ([`PortError::Rejected`]) has none; any other failed read leaves nothing to place.
    fn for_gather(&self) -> Option<(&[Epic], &[crate::domain::Ticket])> {
        let epics = match &self.epics {
            Ok(epics) => epics.as_slice(),
            Err(PortError::Rejected(_)) => &[],
            Err(_) => return None,
        };
        Some((epics, self.questions.as_deref().ok()?))
    }
}

/// What [`gather_state`] reads from.
pub struct StateInputs<'a> {
    /// The open epics and questions the caller already read; none has [`gather_state`] read them
    /// from the board itself.
    pub lists: Option<&'a BoardLists>,
    /// The project's name.
    pub project: &'a str,
    /// The project's root.
    pub root: &'a Path,
    /// The project's state folder.
    pub state_dir: &'a Path,
    /// The board, or why there is none to read, in plain words: the reason the state carries.
    pub board: Result<&'a dyn Board, String>,
    /// The forge, for the phases; none leaves every phase unread.
    pub forge: Option<&'a dyn Forge>,
    /// The version control, for the phases; none leaves every phase unread.
    pub vcs: Option<&'a dyn Vcs>,
    /// The slice records, for the phases and the pull requests.
    pub records: &'a SliceRecords,
    /// Every row of the ledger, this machine's: the gather keeps this project's.
    pub ledger: &'a [LedgerRow],
    /// What a run costs.
    pub prices: &'a PriceTable,
    /// The address a pull request's number is appended to; none when the forge has no page.
    pub pull_base: Option<&'a str>,
    /// Whether a recorded process still runs.
    pub alive: &'a dyn Fn(&Holder) -> bool,
    /// The time the state is of.
    pub now: SystemTime,
    /// Where the core places each open ticket with a phase list at `now`; none when nothing was
    /// placed, and every task shows its label.
    pub places: Option<&'a Places>,
    /// What the tick's gather read of each open ticket (its facts, its body, the phases of each
    /// type); none when nothing was gathered, and a line then reads its label.
    pub gathered: Option<&'a Gathered>,
    /// The releases whose objectives or design are due, worked out from the open epics the board
    /// read; none leaves `releases_due` empty.
    pub releases: Option<&'a DueReleases<'a>>,
}

/// What works out the releases due from the open epics: [`release::due_releases`] for a project.
pub type DueReleases<'a> = dyn Fn(&[Epic]) -> Vec<DueRelease> + 'a;

/// Gathers the state of the project at `root`. It never fails: a project that cannot be read
/// comes back with its reason.
pub fn gather_project(root: &Path, now: SystemTime) -> ProjectState {
    let composition = match Composition::load(root) {
        Ok(composition) => composition,
        Err(error) => {
            let reason = format!("the manifest cannot be read: {error}");
            let mut state = unreadable(root, now, reason.clone());
            state.problems.push(Problem {
                kind: ProblemKind::Manifest,
                detail: reason,
                fix: Some(doctor_command(root)),
                verb: Some(FixVerb::Doctor),
            });
            return state;
        }
    };
    let manifest = composition.manifest();
    // A board with no token is named before anything asks the network: an instance that is down
    // would hide the variable that is missing.
    let mut problems = Vec::new();
    let board = match missing_token(&manifest.board) {
        Some(variable) => {
            let reason = format!(
                "the board cannot be reached: the variable `{variable}` that holds its token is \
                 not set"
            );
            problems.push(Problem {
                kind: ProblemKind::Token,
                detail: reason.clone(),
                fix: Some(format!("export {variable}=<token>")),
                verb: None,
            });
            Err(reason)
        }
        None => composition.board().map_err(|error| {
            let reason = format!("the board is not set up: {error}");
            problems.push(Problem {
                kind: ProblemKind::Board,
                detail: reason.clone(),
                fix: Some(doctor_command(root)),
                verb: Some(FixVerb::Doctor),
            });
            reason
        }),
    };
    let board = board
        .as_ref()
        .map(|board| board.as_ref())
        .map_err(Clone::clone);
    read_project(&composition, board, problems, now)
}

/// [`gather_project`] over a board already built, or the reason there is none, with the problems
/// found building it. The board's open epics and open questions are read once here and shared by
/// the placement's gather and the state's own read.
fn read_project(
    composition: &Composition,
    board: Result<&dyn Board, String>,
    mut problems: Vec<Problem>,
    now: SystemTime,
) -> ProjectState {
    let (root, manifest) = (composition.root(), composition.manifest());
    let project = manifest.project.name.to_string();
    let forge = composition.forge().ok();
    let vcs = composition.vcs().ok();
    let state_dir = composition.state_dir();
    let prices = crate::app::guard::read_local(&state_dir)
        .ok()
        .and_then(|local| PriceTable::from_local_toml(&local).ok())
        .unwrap_or_default();
    let ledger = zaf_home()
        .and_then(|home| Ledger::rows_if_present(&home.join(LEDGER_FILE)).ok())
        .unwrap_or_default();
    // Where the core places each ticket, as the tick gathers them; a gather that fails leaves the
    // labels' word.
    let lists = board.as_ref().ok().map(|board| BoardLists::read(*board));
    let gathered: Option<Gathered> = board.as_ref().ok().and_then(|board| {
        let (epics, questions) = lists.as_ref()?.for_gather()?;
        gather_for(composition, *board, None, epics, questions)
            .ok()
            .flatten()
    });
    let places: Option<Places> = gathered.as_ref().map(|gathered| {
        shadow::places(gathered, now)
            .into_iter()
            .map(|placed| (placed.ticket, placed.place))
            .collect()
    });
    let pull_base = manifest.forge.as_ref().and_then(|forge| {
        let repo = forge.repo.as_ref()?;
        match forge.kind {
            crate::manifest::ForgeKind::Forgejo => Some(format!(
                "{}/{repo}/pulls/",
                forge.url.as_ref()?.as_ref().trim_end_matches('/')
            )),
            crate::manifest::ForgeKind::Github => Some(format!("https://github.com/{repo}/pull/")),
        }
    });
    // The releases due, as `zaf status` works them out from the epics the board read.
    let due = |epics: &[Epic]| {
        release::due_releases(root, manifest, epics, &|version| {
            release::is_released(root, manifest, vcs.as_deref(), version)
        })
    };
    let mut state = gather_state(&StateInputs {
        lists: lists.as_ref(),
        project: &project,
        root,
        state_dir: &state_dir,
        board,
        forge: forge.as_deref(),
        vcs: vcs.as_deref(),
        records: &SliceRecords::new(root),
        ledger: &ledger,
        prices: &prices,
        pull_base: pull_base.as_deref(),
        alive: &process::alive,
        now,
        places: places.as_ref(),
        gathered: gathered.as_ref(),
        releases: Some(&due),
    });
    // The faults that stopped the board's read lead; a stale profile follows.
    problems.append(&mut state.problems);
    // One problem for all of the project's stale profiles: one sentence, one fix.
    if let Some(detail) = profile::stale_line(&profile::stale_homes(&state_dir, manifest)) {
        problems.push(Problem {
            kind: ProblemKind::Profile,
            detail,
            fix: Some(format!("cd {} && zaf profile init", root.display())),
            verb: Some(FixVerb::ProfileInit),
        });
    }
    state.problems = problems;
    // What `zaf status` shows beside the board, from this same read: the slots the instances and
    // the live model runs hold (as the tick's budget counts them), the guard `zaf guard` last
    // wrote, and the version `zaf begin` would begin next.
    let model_runs = gathered
        .as_ref()
        .and_then(|gathered| gathered.model_runs.clone().ok())
        .unwrap_or(0);
    state.status.slots = Some(SlotsState {
        total: manifest.spawn.slots,
        in_use: u32::try_from(state.instances.len() + model_runs).unwrap_or(u32::MAX),
    });
    state.status.guard = crate::app::guard::read(&state_dir).ok().flatten();
    state.status.next_version =
        Some(release::next_version(root, manifest, vcs.as_deref()).to_string());
    state
}

/// The variable that holds a Forgejo board's token when it is unset or empty.
fn missing_token(board: &crate::manifest::BoardSection) -> Option<String> {
    if board.kind != crate::manifest::BoardKind::ForgejoIssues {
        return None;
    }
    let variable = board.token_env();
    let set = std::env::var(variable.as_str()).is_ok_and(|token| !token.trim().is_empty());
    (!set).then(|| variable.to_string())
}

/// The command that reports what is wrong with the project at `root`.
fn doctor_command(root: &Path) -> String {
    format!("cd {} && zaf doctor", root.display())
}

/// A project with nothing read and the reason why.
fn unreadable(root: &Path, now: SystemTime, reason: String) -> ProjectState {
    ProjectState {
        project: root
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        root: root.to_owned(),
        missing: false,
        gathered_at: humantime::format_rfc3339_seconds(now).to_string(),
        reason: Some(reason),
        epics: Vec::new(),
        queue: Vec::new(),
        questions: Vec::new(),
        problems: Vec::new(),
        instances: Vec::new(),
        orphaned: Vec::new(),
        releases_due: Vec::new(),
        watch: None,
        last_tick: None,
        status: StatusState::default(),
    }
}

/// Gathers one project's state from the ports in `inputs`. A board that fails leaves the epics
/// and the queue empty and the reason set; the instances are read from the run folders all the
/// same.
pub fn gather_state(inputs: &StateInputs) -> ProjectState {
    let mut state = unreadable(inputs.root, inputs.now, String::new());
    state.project = inputs.project.to_owned();
    state.reason = None;
    state.watch = Some(WatchState::read(inputs.state_dir, inputs.alive));
    let mut claims = Vec::new();
    let mut titles = HashMap::new();
    match &inputs.board {
        Ok(board) => match read_board(inputs, *board) {
            Ok(read) => {
                claims = read.claims.clone();
                titles = read.titles;
                state.epics = read.epics;
                state.queue = read.queue;
                state.questions = read.questions;
                state.releases_due = read.releases_due;
                state.status.unparsed = read.unparsed;
                state.status.cycles = read.cycles;
                state.status.dead_blockers = read.dead_blockers;
                state.problems.extend(read.problems);
            }
            Err(error) => {
                let reason = format!("the board cannot be reached: {}", plain(&error));
                state.problems.push(Problem {
                    kind: ProblemKind::Board,
                    detail: reason.clone(),
                    fix: Some(doctor_command(inputs.root)),
                    verb: Some(FixVerb::Doctor),
                });
                state.reason = Some(reason);
            }
        },
        Err(reason) => state.reason = Some(reason.clone()),
    }
    match runs::read_all(inputs.state_dir) {
        Ok(runs) => {
            state.instances = instances_of(inputs, &runs, &claims, &titles);
            state.orphaned = orphaned_of(inputs, &runs, &claims);
        }
        Err(error) => {
            // The board's reason, when there is one, is the one to fix first.
            state
                .reason
                .get_or_insert_with(|| format!("the run folders cannot be read: {error}"));
        }
    }
    state
}

/// What the board gave.
struct BoardRead {
    epics: Vec<EpicState>,
    queue: Vec<QueueTask>,
    questions: Vec<QuestionState>,
    claims: Vec<ClaimRecord>,
    /// Every epic's and task's title, by number: what the one read of the epics held.
    titles: HashMap<u64, String>,
    /// The releases due, from the epics the board listed.
    releases_due: Vec<ReleaseDueState>,
    /// The ready tasks whose body does not follow the template.
    unparsed: Vec<UnparsedTask>,
    /// The blocker cycles among the open tasks.
    cycles: Vec<Vec<u64>>,
    /// The blockers open tasks name that never close.
    dead_blockers: Vec<crate::app::spawn::blockers::DeadLine>,
    /// The open epics whose every task was dropped, one problem each.
    problems: Vec<Problem>,
}

/// The claims of instances this machine launched that no longer run, as `zaf status` reads them
/// ([`crate::app::spawn::orphaned`]); none when the phase run folders cannot be read.
fn orphaned_of(inputs: &StateInputs, runs: &[Run], claims: &[ClaimRecord]) -> Vec<OrphanedClaim> {
    let (Ok(held), Ok(ended)) = (
        crate::app::spawn::held(inputs.state_dir, inputs.alive),
        crate::app::spawn::ended_phases(inputs.state_dir, inputs.alive),
    ) else {
        return Vec::new();
    };
    crate::app::spawn::orphaned(claims, runs, &ended, inputs.alive, &held)
        .into_iter()
        .map(|claim| OrphanedClaim {
            task: claim.task.get(),
            instance: claim.instance.to_string(),
        })
        .collect()
}

fn read_board(inputs: &StateInputs, board: &dyn Board) -> Result<BoardRead, PortError> {
    let read;
    let (trees, open_questions): (&[Epic], &[crate::domain::Ticket]) = match inputs.lists {
        Some(lists) => (
            lists.epics.as_deref().map_err(Clone::clone)?,
            lists.questions.as_deref().map_err(Clone::clone)?,
        ),
        None => {
            read = BoardLists::read(board);
            (
                read.epics.as_deref().map_err(Clone::clone)?,
                read.questions.as_deref().map_err(Clone::clone)?,
            )
        }
    };
    let claims = board.claims()?;
    let ready = board.ready()?;
    let ready_urls: HashMap<TicketId, String> = ready
        .iter()
        .map(|ticket| (ticket.id, ticket.url.to_string()))
        .collect();
    let holders: HashMap<TicketId, String> = claims
        .iter()
        .map(|claim| (claim.task, claim.instance.to_string()))
        .collect();
    let epic_of: HashMap<u64, u64> = trees
        .iter()
        .flat_map(|tree| {
            tree.tasks
                .iter()
                .map(move |task| (task.number.get(), tree.id.get()))
        })
        .collect();
    let dollars = dollars_by_epic(
        inputs.ledger,
        inputs.project,
        inputs.root,
        inputs.prices,
        &epic_of,
    );
    let titles: HashMap<u64, String> = trees
        .iter()
        .flat_map(|tree| {
            std::iter::once((tree.id.get(), tree.title.to_string())).chain(
                tree.tasks
                    .iter()
                    .map(|task| (task.number.get(), task.title.to_string())),
            )
        })
        .collect();
    // A board that cannot tell the doors shows every question as a two-way one.
    let doors = board.open_doors().unwrap_or_default();
    let questions: Vec<QuestionState> = open_questions
        .iter()
        .map(|ticket| QuestionState {
            number: ticket.id.get(),
            raised_on: NewQuestion::raised_on(ticket.body.as_str()).map(TicketId::get),
            text: NewQuestion::text_of(ticket).trim().to_owned(),
            door: doors.contains(&ticket.id),
            url: Some(ticket.url.to_string()).filter(|url| !url.is_empty()),
        })
        .collect();
    // The ready tasks the spawner skips, read from the bodies the board already gave.
    let unparsed = ready
        .iter()
        .filter_map(|ticket| {
            let problems = TaskTemplate::problems(ticket.body.as_str());
            (!problems.is_empty()).then(|| UnparsedTask {
                number: ticket.id.get(),
                title: ticket.title.to_string(),
                problems: problems.iter().map(ToString::to_string).collect(),
            })
        })
        .collect();
    let (mut lines, dead_blockers) = line::read(inputs, board, trees, &questions, &titles, ready);
    let cycles = cycles_of(trees, &lines);
    let mut epics = Vec::new();
    let mut queue = Vec::new();
    for tree in trees {
        let mut epic = epic_state(tree, dollars.get(&tree.id.get()).copied().unwrap_or(0.0));
        if let Some(line) = lines.remove(&tree.id.get()) {
            epic.line_state = line.state;
            epic.line_question = line.question;
            epic.tasks = line.tasks;
        }
        if epic.stage == EpicStage::Unsigned {
            // The link the Engineer signs from; a read that fails shows none.
            epic.url = board
                .task(tree.id)
                .ok()
                .map(|ticket| ticket.url.to_string())
                .filter(|url| !url.is_empty());
        }
        epics.push(epic);
        for task in &tree.tasks {
            let shown = shown_state(task, inputs.places, holders.contains_key(&task.number));
            let Some(state) = shown.filter(|state| in_queue(*state)) else {
                continue;
            };
            queue.push(queue_task(
                inputs,
                board,
                tree.id,
                task,
                state,
                &holders,
                &ready_urls,
            ));
        }
    }
    let releases_due = inputs
        .releases
        .map(|due| due(trees).iter().map(ReleaseDueState::from).collect())
        .unwrap_or_default();
    Ok(BoardRead {
        epics,
        queue,
        questions,
        claims,
        titles,
        releases_due,
        unparsed,
        cycles,
        dead_blockers,
        problems: trees
            .iter()
            .filter_map(Problem::every_task_dropped)
            .collect(),
    })
}

/// The blocker cycles among the open tasks of `trees`, each task's blockers as its line read them
/// from its body: the same walk `zaf status` makes, over the tasks this read already holds.
fn cycles_of(trees: &[Epic], lines: &HashMap<u64, line::EpicLine>) -> Vec<Vec<u64>> {
    let open: std::collections::BTreeSet<u64> = trees
        .iter()
        .flat_map(|tree| &tree.tasks)
        .filter(|task| !task.finished())
        .map(|task| task.number.get())
        .collect();
    let graph: crate::app::spawn::blockers::Graph = lines
        .values()
        .flat_map(|line| &line.tasks)
        .filter(|task| open.contains(&task.number))
        .map(|task| {
            (
                TicketId::from(task.number),
                task.blocked_by
                    .iter()
                    .map(|blocker| TicketId::from(blocker.number))
                    .collect(),
            )
        })
        .collect();
    crate::app::spawn::blockers::cycles(&graph)
        .iter()
        .map(|cycle| crate::app::spawn::blockers::cycle_numbers(cycle))
        .collect()
}

fn epic_state(tree: &Epic, dollars: f64) -> EpicState {
    let (done, total) = tally(tree.tasks.iter());
    let mut groups: BTreeMap<&str, Vec<&EpicTask>> = BTreeMap::new();
    for task in &tree.tasks {
        if let Some(name) = &task.group
            && task.counts()
        {
            groups.entry(name.as_str()).or_default().push(task);
        }
    }
    EpicState {
        number: tree.id.get(),
        title: tree.title.to_string(),
        stage: tree.stage(),
        release: tree.release().map(str::to_owned),
        url: None,
        done,
        total,
        dollars,
        line_state: LineState::NotStarted,
        line_question: None,
        tasks: Vec::new(),
        groups: groups
            .into_iter()
            .map(|(name, tasks)| {
                let (done, total) = tally(tasks.into_iter());
                GroupState {
                    name: name.to_owned(),
                    done,
                    total,
                }
            })
            .collect(),
    }
}

/// Whether a task in the state it shows ([`shown_state`]) is in the queue: ready, claimed,
/// blocked or waiting on a question.
fn in_queue(state: TicketState) -> bool {
    matches!(
        state,
        TicketState::Ready | TicketState::Claimed | TicketState::Blocked | TicketState::Question
    )
}

fn queue_task(
    inputs: &StateInputs,
    board: &dyn Board,
    epic: TicketId,
    task: &EpicTask,
    state: TicketState,
    holders: &HashMap<TicketId, String>,
    ready_urls: &HashMap<TicketId, String>,
) -> QueueTask {
    let issue_url = match ready_urls.get(&task.number) {
        Some(url) => Some(url.clone()),
        None => board
            .task(task.number)
            .ok()
            .map(|ticket| ticket.url.to_string()),
    }
    .filter(|url| !url.is_empty());
    // Shown only: a record that cannot be read shows no pull request, and decides nothing.
    let pull_request = inputs.records.pr(task.number).ok().flatten();
    let claimed = state == TicketState::Claimed;
    QueueTask {
        number: task.number.get(),
        title: task.title.to_string(),
        epic: epic.get(),
        state,
        group: task.group.as_ref().map(ToString::to_string),
        instance: holders.get(&task.number).cloned().filter(|_| claimed),
        phase: if claimed {
            phase_of(inputs, board, task.number)
        } else {
            None
        },
        issue_url,
        pull_request: pull_request.map(|pr| pr.get()),
        pull_request_url: pull_request
            .zip(inputs.pull_base)
            .map(|(pr, base)| format!("{base}{}", pr.get())),
    }
}

/// A task's Team Phase; none when the forge or the version control is absent or a read fails.
fn phase_of(inputs: &StateInputs, board: &dyn Board, task: TicketId) -> Option<String> {
    let facts = gather_phase_facts(board, inputs.forge?, inputs.vcs?, inputs.records, task).ok()?;
    Some(derive_phase(&facts).to_string())
}

fn instances_of(
    inputs: &StateInputs,
    runs: &[Run],
    claims: &[ClaimRecord],
    titles: &HashMap<u64, String>,
) -> Vec<InstanceState> {
    let mut phases: HashMap<TicketId, Option<String>> = HashMap::new();
    let mut live = Vec::new();
    for run in runs.iter().filter(|run| run.running(inputs.alive)) {
        let Some(record) = &run.record else {
            continue;
        };
        let task = record.tasks.first().copied().or_else(|| {
            claims
                .iter()
                .find(|claim| claim.instance.to_string() == record.instance)
                .map(|claim| claim.task.get())
        });
        let parsed = record.instance.parse::<Instance>().ok();
        let role = parsed.as_ref().map_or_else(
            || {
                record
                    .instance
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            },
            |instance| instance.role.to_string(),
        );
        // Named by its task; one with none (planning, the challenger, release) by its epic.
        let number = task.or_else(|| parsed.as_ref().map(|instance| instance.epic.get()));
        let started = record.started_at.as_deref().unwrap_or(&record.reserved_at);
        let run_seconds = humantime::parse_rfc3339(started)
            .ok()
            .and_then(|at| inputs.now.duration_since(at).ok())
            .map(|elapsed| elapsed.as_secs());
        let ticket = task.map(TicketId::from);
        live.push(InstanceState {
            instance: record.instance.clone(),
            role,
            task,
            number,
            title: number.and_then(|number| titles.get(&number).cloned()),
            phase: ticket.and_then(|ticket| {
                let board = inputs.board.as_ref().ok()?;
                phases
                    .entry(ticket)
                    .or_insert_with(|| phase_of(inputs, *board, ticket))
                    .clone()
            }),
            run_seconds,
            last_tool_call: ticket
                .and_then(|ticket| last_report(inputs.state_dir, ticket))
                .map(|(word, _)| word),
        });
    }
    live
}

/// A board's failure in plain words: what the backend said, without its error class.
fn plain(error: &PortError) -> String {
    match error {
        PortError::NotFound(text)
        | PortError::Conflict(text)
        | PortError::Rejected(text)
        | PortError::Unavailable(text)
        | PortError::Backend(text) => text.clone(),
        PortError::TimedOut { .. } => error.to_string(),
    }
}

#[cfg(test)]
mod tests;
