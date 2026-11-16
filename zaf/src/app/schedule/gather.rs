//! The ports read into the core's facts: one [`TicketFacts`] for each open ticket whose type has a
//! phase list, for [`crate::domain::schedule::place`] to place.
//!
//! [`gather`] lists the open tickets ([`Board::open_tickets`]), the open epics and the open
//! questions once, then reads each ticket with a phase list and an epic: its type's actions
//! ([`phases_of`] folds adjacent model actions into one [`Phase`]), its size, its epic's signature
//! and challenge, the open questions raised on it or on its epic, where each ticket its `Blocked
//! by:` line names stands ([`Board::standing`], each asked once), the forge's word on its recorded
//! pull request, its phase run folders ([`record::read_runs`]) and, once an action of it has ended
//! [`NOWHERE_LIMIT`] times, the markers of the ruled questions on it.
//!
//! Each ticket's priority is its `priority/<n>` label as the board lists it; a label that names no
//! level, or more than one, is ignored and named in [`Gathered::warnings`], never a failure. Its
//! `ready_since` is the best time the board gives with no state kept: the latest of when the
//! ticket was opened and when its last done dependency was closed ([`Board::standing_at`]). The
//! ruling time of a question on it or its epic is not read: the board gives no ruling time, and
//! the questions on a ticket are a list read of their own. With neither time the core falls back
//! as it always has (the phase that ended last, then the tick's own time).
//!
//! A ticket with no phase list is passed over and never reaches the core, as the tick passes it
//! over today. A ticket something about which cannot be read is named in
//! [`Gathered::unreadable`] and skipped ([`Skip`]); so is a ticket that is not a task of an epic,
//! which has no signature to run under ([`NOT_OF_AN_EPIC`]), and one whose pull request record
//! cannot be read, which is never taken for one with none. A list read for every ticket that
//! fails is the error. Beside each ticket's facts it keeps what the tick acts on but the core
//! does not read ([`Beside`]): the phase list as declared, the epic and the questions open on the
//! epic. [`gather_one`] reads one ticket the same way, for `zaf spawn phase`, which refuses a
//! ticket the tick skips in the same words.
//!
//! [`gather`] also counts the slots the live model runs hold, from the phase run folders
//! themselves ([`model_runs`]), so a live run holds its slot even when the rest of its ticket
//! cannot be read. It keeps what it listed for the tick's label sync ([`super::labels`]): every
//! open ticket with its label and the open questions by the issue each was raised on; and, for the
//! trace's count, the done tasks of the open epics whose type has a phase list. Nothing here
//! writes, and nothing here decides.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, UNIX_EPOCH};

use super::ask::marked_run;
use super::record;
use crate::adapters::process;
use crate::app::actions::{ActionKind, ListedPhase, SliceRecords, live_phases, parse, resolve};
use crate::app::spawn::blockers::blockers;
use crate::app::spawn::phase::{Ports, listed};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::schedule::{
    ActionName, CheckFact, CheckResult, Dependency, EpicFacts, NOWHERE_LIMIT, Phase, PullFacts,
    ReviewFact, RuledMarker, TicketFacts,
};
use crate::domain::{
    DEFAULT_TASK_TYPE, Epic, EpicTask, OpenTicket, Standing, StandingAt, Ticket, TicketId,
};
use crate::ports::{Board, CheckState, Forge, PortError, PrNumber};

/// Why a ticket that names no epic is skipped by the tick and refused by `zaf spawn phase`: it has
/// no signature to run under, so it does not run.
pub const NOT_OF_AN_EPIC: &str = "it is not a task of an epic";

/// What [`gather`] read: the facts of each open ticket with a phase list, in ticket order, what
/// the tick reads beside them, the tickets it skipped, and the slots the live model runs hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gathered {
    /// Each open ticket whose type has a phase list, the lowest number first.
    pub tickets: Vec<TicketFacts>,
    /// What the tick reads of each of them beside its facts, by ticket.
    pub beside: BTreeMap<TicketId, Beside>,
    /// The tickets skipped: something about them could not be read, or they cannot run.
    pub unreadable: Vec<Unreadable>,
    /// The live model runs of the project's phase run folders, every ticket's, gathered or not
    /// ([`model_runs`]); the error when the folders cannot be read, and then no model starts.
    pub model_runs: Result<usize, String>,
    /// Every open ticket as the board listed it, its label included, a phase list or not: what
    /// the label sync writes over.
    pub open: Vec<Ticket>,
    /// The open questions, by the issue each was raised on ([`questions_by_issue`]).
    pub questions: BTreeMap<TicketId, BTreeSet<TicketId>>,
    /// The tasks of the open epics that are done (labelled so, or closed as completed) and whose
    /// type has a phase list: the done the trace counts, since the board lists no done ticket as
    /// open.
    pub done: BTreeSet<TicketId>,
    /// The open epics as the board listed them: what the challenge deadline reads
    /// ([`crate::domain::schedule::unchallenged`]).
    pub epics: Vec<Epic>,
    /// What was read but ignored, in words, for the tick to print: a gathered ticket's priority
    /// labels that name no level, or more than one.
    pub warnings: Vec<String>,
}

impl Default for Gathered {
    /// Nothing gathered, and no live model run.
    fn default() -> Self {
        Self {
            tickets: Vec::new(),
            beside: BTreeMap::new(),
            unreadable: Vec::new(),
            model_runs: Ok(0),
            open: Vec::new(),
            questions: BTreeMap::new(),
            done: BTreeSet::new(),
            epics: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// What the tick reads of a gathered ticket that the core does not: its phase list as its actions
/// declare it (the kind of each, which says the step that runs it), its epic, and the questions
/// open on the epic, which name why the ticket waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beside {
    /// Its type's phase list, unfolded, each action with its `kind`, `ends` and `on-fail`.
    pub list: Vec<ListedPhase>,
    /// Its epic, open or not: every gathered ticket names one ([`NOT_OF_AN_EPIC`]).
    pub epic: TicketId,
    /// The open questions raised on its epic.
    pub on_epic: BTreeSet<TicketId>,
}

/// A ticket the tick skips, and why: something about it could not be read, or it cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// The ticket.
    pub ticket: TicketId,
    /// Why it is skipped, as [`Skip::reason`] words it.
    pub reason: String,
}

/// Why one ticket was not gathered. The tick names either in [`Gathered::unreadable`] and skips
/// the ticket; `zaf spawn phase` refuses the first and fails on the second, in the same words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The ticket cannot run as it stands: [`NOT_OF_AN_EPIC`].
    CannotRun(String),
    /// Something about it, or a list every ticket shares, could not be read.
    Unread(String),
}

impl Skip {
    /// Why, in words: `it is not a task of an epic`, `pull request #9: not found: #9`.
    pub fn reason(&self) -> &str {
        match self {
            Self::CannotRun(reason) | Self::Unread(reason) => reason,
        }
    }
}

/// The reads every ticket shares, made once.
struct Shared {
    /// The open epics, by id.
    epics: BTreeMap<TicketId, EpicFacts>,
    /// The open questions, by the issue each was raised on.
    questions: BTreeMap<TicketId, BTreeSet<TicketId>>,
    /// The open epics' tasks, as the epics list them.
    tasks: Vec<EpicTask>,
    /// The open epics as listed.
    listed: Vec<Epic>,
}

/// Reads the facts of every open ticket whose type has a phase list (see the module above).
///
/// The open tickets, the open epics and the open questions are each read once; a board that
/// fails one of those reads is the error, and nothing is gathered. A board that cannot list its
/// epics at all ([`PortError::Rejected`]) gives every ticket no epic facts, which holds nothing
/// back. The live model runs are counted from the run folders ([`model_runs`]).
pub fn gather(ports: &Ports<'_>) -> Result<Gathered, PortError> {
    let open = ports.board.open_tickets()?;
    let shared = shared(ports.board)?;
    let mut standings = BTreeMap::new();
    let done = shared
        .tasks
        .iter()
        .filter(|task| task.standing == Standing::Done)
        .filter(|task| {
            let kind = task
                .kind
                .as_ref()
                .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str());
            ports.manifest.phase_list(kind).is_some()
        })
        .map(|task| task.number)
        .collect();
    let mut gathered = Gathered {
        model_runs: model_runs(ports),
        questions: shared.questions.clone(),
        done,
        epics: shared.listed.clone(),
        ..Gathered::default()
    };
    for open in &open {
        let ticket = &open.ticket;
        match read_ticket(ports, &shared, &mut standings, open) {
            Ok(Some((facts, beside))) => {
                if let Err(ignored) = &open.priority {
                    gathered.warnings.push(format!(
                        "{}: its priority labels are ignored: {ignored}",
                        ticket.id
                    ));
                }
                gathered.beside.insert(ticket.id, beside);
                gathered.tickets.push(facts);
            }
            Ok(None) => {}
            Err(skip) => gathered.unreadable.push(Unreadable {
                ticket: ticket.id,
                reason: skip.reason().to_owned(),
            }),
        }
    }
    gathered.tickets.sort_by_key(|facts| facts.ticket);
    gathered
        .unreadable
        .sort_by_key(|unreadable| unreadable.ticket);
    gathered.open = OpenTicket::tickets(open);
    Ok(gathered)
}

/// [`gather`] for the project `composition` loads, over `board`, as the tick gathers it: its
/// manifest, its forge, its root, its state folder, the zaf home's actions and this machine's
/// process table. None with no `[phases]`, where nothing is placed. A forge that cannot be built,
/// or a read every ticket needs that fails, is the error, in words.
pub fn gather_for(
    composition: &Composition,
    board: &dyn Board,
) -> Result<Option<Gathered>, String> {
    let manifest = composition.manifest();
    if manifest.phases.is_empty() {
        return Ok(None);
    }
    let forge = composition.forge().map_err(|error| error.to_string())?;
    let root = composition.root();
    let state_dir = composition.state_dir();
    // With no home there is no machine tier, so the project's actions alone are looked for.
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    let ports = Ports {
        board,
        forge: forge.as_ref(),
        manifest,
        root,
        home: &home,
        state_dir: &state_dir,
        alive: &process::alive,
    };
    gather(&ports).map(Some).map_err(|error| error.to_string())
}

/// One ticket gathered as [`gather`] gathers each: its facts and what is read beside them, or none
/// when it is no open ticket whose type has a phase list (done, closed, or of a type with no
/// list). The open tickets, the open epics and the open questions are read as for every ticket.
/// A ticket the tick would skip is the [`Skip`] the tick names it by: one that cannot run, or
/// one something about which fails to read, the lists every ticket shares included.
pub fn gather_one(
    ports: &Ports<'_>,
    task: TicketId,
) -> Result<Option<(TicketFacts, Beside)>, Skip> {
    let unread = |error: PortError| Skip::Unread(error.to_string());
    let open = ports.board.open_tickets().map_err(unread)?;
    let Some(ticket) = open.iter().find(|open| open.ticket.id == task) else {
        return Ok(None);
    };
    let shared = shared(ports.board).map_err(unread)?;
    read_ticket(ports, &shared, &mut BTreeMap::new(), ticket)
}

/// The slots the project's live phase runs hold, read from the phase run folders themselves
/// ([`live_phases`]), whatever ticket each is of and whether or not that ticket reads this tick:
/// one for each whose action declares `kind: model`. A wait or a `zaf`-kind run holds none, and
/// neither does a run whose process is dead or that recorded its exit. A run whose action's kind
/// cannot be read holds one: a slot held in doubt never lets a tick start one over the slots.
/// Folders that cannot be read are the error.
pub fn model_runs(ports: &Ports<'_>) -> Result<usize, String> {
    let live = live_phases(&ports.state_dir.join(record::PHASES_DIR), ports.alive)?;
    let mut kinds: BTreeMap<&ActionName, ActionKind> = BTreeMap::new();
    let mut count = 0;
    for run in &live {
        let kind = *kinds
            .entry(&run.action)
            .or_insert_with(|| declared_kind(ports, &run.action).unwrap_or(ActionKind::Model));
        if kind == ActionKind::Model {
            count += 1;
        }
    }
    Ok(count)
}

/// The `kind` `action` declares in its metadata; none when it cannot be found or read, or
/// declares none.
fn declared_kind(ports: &Ports<'_>, action: &ActionName) -> Option<ActionKind> {
    let found = resolve(ports.root, ports.home, action.as_str()).ok()??;
    let text = std::fs::read_to_string(&found.path).ok()?;
    parse(&found.path, &text).ok()?.metadata.kind
}

/// The reads every ticket shares: the open epics and the open questions. A board that cannot list
/// its epics at all ([`PortError::Rejected`]) has none, which holds nothing back.
fn shared(board: &dyn Board) -> Result<Shared, PortError> {
    let epics = match board.epics() {
        Ok(epics) => epics,
        Err(PortError::Rejected(_)) => Vec::new(),
        Err(error) => return Err(error),
    };
    Ok(Shared {
        tasks: epics.iter().flat_map(|epic| epic.tasks.clone()).collect(),
        epics: epics
            .iter()
            .map(|epic| (epic.id, epic_facts(epic)))
            .collect(),
        questions: questions_by_issue(&board.questions()?),
        listed: epics,
    })
}

/// What the core reads of an open epic.
fn epic_facts(epic: &Epic) -> EpicFacts {
    EpicFacts {
        id: epic.id,
        signed: epic.signed(),
        challenged: epic.challenged,
    }
}

/// The open questions under the issue each was raised on; a question that names none is under
/// none.
pub fn questions_by_issue(questions: &[Ticket]) -> BTreeMap<TicketId, BTreeSet<TicketId>> {
    let mut by_issue: BTreeMap<TicketId, BTreeSet<TicketId>> = BTreeMap::new();
    for question in questions {
        if let Some(on) = crate::ports::NewQuestion::raised_on(question.body.as_str()) {
            by_issue.entry(on).or_default().insert(question.id);
        }
    }
    by_issue
}

/// The open questions raised on `ticket` or on its epic, from the open questions by the issue each
/// was raised on ([`questions_by_issue`]).
pub fn open_on(
    questions: &BTreeMap<TicketId, BTreeSet<TicketId>>,
    ticket: &Ticket,
) -> BTreeSet<TicketId> {
    [Some(ticket.id), ticket.epic]
        .into_iter()
        .flatten()
        .filter_map(|issue| questions.get(&issue))
        .flatten()
        .copied()
        .collect()
}

/// One ticket's facts and what is read beside them: none when its type has no phase list, the
/// [`Skip`] that names why otherwise: [`NOT_OF_AN_EPIC`] for one that names no epic, or what could
/// not be read. `standings` keeps each dependency's standing, and when it closed, once it is
/// asked. Its priority is its label's, none when the label is ignored (the caller names that).
fn read_ticket(
    ports: &Ports<'_>,
    shared: &Shared,
    standings: &mut BTreeMap<TicketId, StandingAt>,
    open: &OpenTicket,
) -> Result<Option<(TicketFacts, Beside)>, Skip> {
    let ticket = &open.ticket;
    let task_type = ticket
        .kind
        .as_ref()
        .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str());
    let Some(names) = ports.manifest.phase_list(task_type) else {
        return Ok(None);
    };
    let Some(epic_id) = ticket.epic else {
        return Err(Skip::CannotRun(NOT_OF_AN_EPIC.to_owned()));
    };
    let list =
        listed(ports.root, ports.home, names).map_err(|error| Skip::Unread(error.to_string()))?;
    let size = ticket
        .template()
        .map_err(|error| Skip::Unread(format!("it is not a task: {error}")))?
        .size;
    let epic = shared.epics.get(&epic_id).copied();
    let open_questions = open_on(&shared.questions, ticket);
    let mut dependencies = Vec::new();
    let mut last_done = None;
    for blocker in blockers(ticket.body.as_str()) {
        let at = match standings.get(&blocker) {
            Some(at) => *at,
            None => {
                let at = ports
                    .board
                    .standing_at(blocker)
                    .map_err(|error| Skip::Unread(format!("its blocker {blocker}: {error}")))?;
                standings.insert(blocker, at);
                at
            }
        };
        if at.standing == Standing::Done {
            last_done = last_done.max(at.closed_at);
        }
        dependencies.push(Dependency {
            ticket: blocker,
            standing: at.standing,
        });
    }
    // A record that cannot be read is never "no pull request": implement would start again over
    // an open one.
    let recorded = SliceRecords::new(ports.root)
        .pr(ticket.id)
        .map_err(|error| Skip::Unread(format!("its pull request record {error}")))?;
    let pull = match recorded {
        Some(pr) => Some(
            read_pull(ports.forge, pr)
                .map_err(|reason| Skip::Unread(format!("pull request {pr}: {reason}")))?,
        ),
        None => None,
    };
    let runs = record::read_runs(
        &ports.state_dir.join(record::PHASES_DIR),
        ticket.id,
        ports.alive,
    )
    .map_err(Skip::Unread)?;
    let phases = phases_of(&list);
    // The board's question list is long; it is read only once an action has ended often enough
    // for getting nowhere to count.
    let ended = |action: &ActionName| {
        runs.iter()
            .filter(|run| &run.action == action && run.exit.is_some())
            .count()
    };
    let counting: Vec<ActionName> = runs
        .iter()
        .map(|run| run.action.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|action| ended(action) >= NOWHERE_LIMIT)
        .collect();
    let ruled = if counting.is_empty() {
        Vec::new()
    } else {
        ruled_markers(ports.board, ticket.id, &counting)
            .map_err(|error| Skip::Unread(format!("its questions: {error}")))?
    };
    let on_epic = shared.questions.get(&epic_id).cloned().unwrap_or_default();
    let facts = TicketFacts {
        ticket: ticket.id,
        epic,
        open_questions,
        dependencies,
        pull,
        runs,
        phases,
        size,
        priority: open.priority.clone().ok().flatten(),
        ruled,
        ready_since: open.opened_at.max(last_done),
    };
    let beside = Beside {
        list,
        epic: epic_id,
        on_epic,
    };
    Ok(Some((facts, beside)))
}

/// A type's phase list as the core reads it: adjacent `model` actions folded into one [`Phase`]
/// (one process runs them), named by the first, ending at each folded action's end and failing
/// to the first's `on-fail`; every other action a phase of its own.
pub fn phases_of(list: &[ListedPhase]) -> Vec<Phase> {
    let mut phases: Vec<Phase> = Vec::new();
    let mut folding = false;
    for listed in list {
        let model = listed.kind == ActionKind::Model;
        match phases.last_mut() {
            Some(last) if model && folding => last.ends.push(listed.ends),
            _ => phases.push(Phase {
                name: listed.action.clone(),
                ends: vec![listed.ends],
                on_fail: listed.on_fail.clone(),
            }),
        }
        folding = model;
    }
    phases
}

/// The forge's facts about one pull request, as the core reads them. A merged pull request with
/// no merge time, or a finished check with no time, is an error, never a guess, and so is a forge
/// that cannot say whether it changes a file.
pub fn read_pull(forge: &dyn Forge, pr: PrNumber) -> Result<PullFacts, String> {
    let failed = |source: PortError| source.to_string();
    let merged = forge.merged(pr).map_err(failed)?;
    let request = forge.pull_request(pr).map_err(failed)?;
    let merged_at = if merged {
        Some(
            request
                .merged_at
                .ok_or_else(|| "it is merged but the forge gives no merge time".to_owned())?,
        )
    } else {
        None
    };
    let head = forge
        .head(pr)
        .map_err(failed)?
        .map(crate::domain::CommitId::from);
    let changes_a_file = forge.last_changed_file(pr).map_err(failed)?.is_some();
    let mut checks = Vec::new();
    for check in forge.checks(pr).map_err(failed)? {
        let result = match (check.state, check.finished_at) {
            (CheckState::Pending, _) => CheckResult::Pending,
            (CheckState::Passed, Some(at)) => CheckResult::Passed(at),
            (CheckState::Failed, Some(at)) => CheckResult::Failed(at),
            (CheckState::Passed | CheckState::Failed, None) => {
                return Err(format!("check {} finished with no time", check.name));
            }
        };
        checks.push(CheckFact {
            name: check.name,
            result,
        });
    }
    let reviews = forge
        .review_comments(pr)
        .map_err(failed)?
        .into_iter()
        .map(|comment| ReviewFact {
            head: comment.head,
            posted_at: comment.posted_at,
        })
        .collect();
    Ok(PullFacts {
        opened_at: request.opened_at,
        merged_at,
        head,
        changes_a_file,
        checks,
        reviews,
    })
}

/// The markers of the ruled questions on `task` that name one of `actions`: the start of the last
/// run each counted (`Last run: <nanoseconds>.`, as the tick writes it, [`marked_run`]).
pub fn ruled_markers(
    board: &dyn Board,
    task: TicketId,
    actions: &[ActionName],
) -> Result<Vec<RuledMarker>, PortError> {
    let questions = board.questions_on(task)?;
    let mut markers = Vec::new();
    for (question, ruling) in &questions {
        if ruling.is_none() {
            continue;
        }
        let text = crate::ports::NewQuestion::text_of(question);
        for action in actions {
            let Some(stamp) = marked_run(text, action) else {
                continue;
            };
            let Ok(nanos) = stamp.trim().parse::<u64>() else {
                continue;
            };
            markers.push(RuledMarker {
                action: action.clone(),
                last_run: UNIX_EPOCH + Duration::from_nanos(nanos),
            });
        }
    }
    Ok(markers)
}

#[cfg(test)]
mod tests;
