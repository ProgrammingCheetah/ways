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
//! cannot be read. Once per gather it reads the integration branch's checks
//! ([`Forge::branch_checks`], for the branch [`integration_branch`] names) and hands them to every
//! ticket, then lets the core order the merges of the tickets that share that branch
//! ([`order_merges`]): what a ticket at its merge
//! waits on beyond itself (R29, R30). A branch read that fails fails nothing: the base is unknown,
//! which holds no merge back, and the failure is named in [`Gathered::warnings`]. A ticket the
//! branch would hold is also asked whether a ruled question lets it through at that head
//! ([`ruled_through`]). It keeps what it listed for the tick's label sync ([`super::labels`]): every
//! open ticket with its label and the open questions by the issue each was raised on; and, for the
//! trace's count, the done tasks of the open epics whose type has a phase list. Nothing here
//! writes, and nothing here decides.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ask::{base_deadline_marker, marked_run, no_check_marker};
use super::{lock, record};
use crate::adapters::process;
use crate::app::actions::{
    ActionKind, ListedPhase, SliceRecords, VERSION_VAR, live_phases, parse, resolve,
};
use crate::app::spawn::blockers::{blockers, finished_epic_standing};
use crate::app::spawn::phase::{Ports, listed};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::schedule::{
    ActionName, BaseFacts, CheckFact, CheckResult, Dependency, EpicFacts, GroupBase, HeadEnd,
    MergeFacts, NO_CHECK_DEADLINE, NOWHERE_LIMIT, Phase, PullFacts, ReviewFact, RuledMarker,
    RunRecord, TicketFacts, Trees, at_merge, order_merges,
};
use crate::domain::{
    ChangedFiles, CommitId, DEFAULT_TASK_TYPE, Epic, EpicTask, OpenTicket, Standing, StandingAt,
    Ticket, TicketId, WorkflowFilter, checks_expected,
};
use crate::manifest::BranchesSection;
use crate::ports::{Board, BranchName, Check, CheckState, Forge, PortError, PrNumber};

/// The placeholder for the release version in a branch pattern.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The placeholder for the epic in a branch pattern.
const EPIC_PLACEHOLDER: &str = "{epic}";

/// The placeholder for the task in a branch pattern.
const TASK_PLACEHOLDER: &str = "{task}";

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
    /// labels that name no level, or more than one; an integration branch whose checks could not
    /// be read; and a ticket held by that branch whose ruled questions could not be read.
    pub warnings: Vec<String>,
    /// The integration branch and what the forge says of it, as every ticket's facts carry it;
    /// none when no version names the branch, no ticket was gathered, or the read failed.
    pub base: Option<Base>,
    /// How many phases each task type with a phase list has, folded as the core reads them, by
    /// the type's name: what a done task, which the gather holds no facts of, has ended. A type
    /// whose list could not be read is absent.
    pub phase_totals: BTreeMap<String, u32>,
}

/// The integration branch of a gather, and what the forge says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The branch.
    pub branch: BranchName,
    /// The commit it is at and the checks there; unknown when the forge has no such branch.
    pub facts: BaseFacts,
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
            base: None,
            phase_totals: BTreeMap::new(),
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
    /// The workflows read at each base so far, none for a base whose read failed: each base is
    /// read once per gather ([`expects_a_check`]).
    workflows: RefCell<BTreeMap<BranchName, Option<Vec<WorkflowFilter>>>>,
}

/// Reads the facts of every open ticket whose type has a phase list (see the module above).
///
/// The open tickets are read once; the open epics and the open questions are the ones given, read
/// by the caller once for everything it does ([`read_lists`]), so a gather never asks the board
/// for them. A board that fails the read of the tickets is the error, and nothing is gathered.
/// The live model runs are counted from the run folders ([`model_runs`]).
pub fn gather(
    ports: &Ports<'_>,
    epics: &[Epic],
    questions: &[Ticket],
) -> Result<Gathered, PortError> {
    let open = ports.board.open_tickets()?;
    let shared = shared(epics, questions);
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
        match read_ticket(ports, &shared, &mut standings, &mut gathered.warnings, open) {
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
    let (base, warnings) = merging(ports, &mut gathered.tickets, None);
    gathered.base = base;
    gathered.warnings.extend(warnings);
    gathered.open = OpenTicket::tickets(open);
    gathered.phase_totals = phase_totals(ports, &shared.tasks);
    Ok(gathered)
}

/// The phase count of each type among `tasks` that has a phase list, folded as the core reads it
/// ([`phases_of`]); a type whose actions cannot be listed is left out.
fn phase_totals(ports: &Ports<'_>, tasks: &[EpicTask]) -> BTreeMap<String, u32> {
    let kinds: BTreeSet<&str> = tasks
        .iter()
        .map(|task| {
            task.kind
                .as_ref()
                .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str())
        })
        .collect();
    kinds
        .into_iter()
        .filter_map(|kind| {
            let names = ports.manifest.phase_list(kind)?;
            let list = listed(ports.root, ports.home, names).ok()?;
            let total = u32::try_from(phases_of(&list).len()).ok()?;
            Some((kind.to_owned(), total))
        })
        .collect()
}

/// The integration branch every ticket of `version` merges into: `[branches] integration` with
/// `{version}` filled. None when the pattern needs a version and there is none, when it names the
/// task or the epic (then there is no one branch), or when the name cannot be handed to a forge.
pub fn integration_branch(branches: &BranchesSection, version: Option<&str>) -> Option<BranchName> {
    let pattern = branches.integration.as_ref();
    let name = if pattern.contains(VERSION_PLACEHOLDER) {
        pattern.replace(
            VERSION_PLACEHOLDER,
            version.filter(|version| !version.is_empty())?,
        )
    } else {
        pattern.to_owned()
    };
    let branch = BranchName::from(name);
    (!branch.as_str().contains('{') && branch.is_usable()).then_some(branch)
}

/// Fills what the merge of each of `tickets` reads beyond the ticket itself
/// ([`MergeFacts`]): the integration branch's checks, read once ([`Forge::branch_checks`]) and
/// handed to every ticket, and the order of the merges among the tickets of one integration
/// branch ([`order_by_branch`]). Gives the branch as read; none with no ticket, or no branch to
/// name ([`integration_branch`]). Each ticket the branch would hold also learns whether the
/// Engineer ruled it through at that head ([`ruled_through`]). A branch read that fails leaves
/// every base unknown, and a ticket whose ruled questions cannot be read stays held; each is a
/// warning, in words, beside what was read. The merges are ordered either way.
///
/// Each ticket is handed the branch as R29 judges it: the branch as read, or, for a merge in a
/// tick's group while the branch is still at the group's own last commit, the group's base
/// ([`GroupBase::judged`]). The base given back is the branch as read.
fn merging(
    ports: &Ports<'_>,
    tickets: &mut [TicketFacts],
    group: Option<&GroupBase>,
) -> (Option<Base>, Vec<String>) {
    order_by_branch(&ports.manifest.branches, tickets);
    if tickets.is_empty() {
        return (None, Vec::new());
    }
    let Some(branch) = integration_branch(&ports.manifest.branches, ports.version) else {
        return (None, Vec::new());
    };
    let facts = match read_base(ports.forge, &branch) {
        Ok(facts) => facts,
        Err(reason) => {
            let warning = format!(
                "the checks of the integration branch {branch} were not read, so no merge waits \
                 on them: {reason}"
            );
            return (None, vec![warning]);
        }
    };
    let judged = group.map_or_else(|| facts.clone(), |group| group.judged(&facts));
    let mut warnings = Vec::new();
    for ticket in tickets {
        ticket.merging.base = judged.clone();
        match ruled_through(ports.board, ticket) {
            Ok(ruled_at) => ticket.merging.ruled_at = ruled_at,
            Err(error) => warnings.push(format!(
                "{}: its ruled questions were not read, so it still waits on the integration \
                 branch {branch}: {error}",
                ticket.ticket
            )),
        }
    }
    (Some(Base { branch, facts }), warnings)
}

/// The head of the integration branch `facts`'s ticket is ruled through, if it is: the branch's
/// checks would hold its merge ([`at_merge`], [`BaseFacts::awaited`]) and a ruled question on the
/// ticket ends with the marker of that head ([`base_deadline_marker`]). The ticket's questions
/// are read only for a ticket the branch would hold, so most gathers read none.
fn ruled_through(board: &dyn Board, facts: &TicketFacts) -> Result<Option<CommitId>, PortError> {
    let Some(head) = facts.merging.base.awaited().filter(|_| at_merge(facts)) else {
        return Ok(None);
    };
    let ruled = ruled_ending(board, facts.ticket, &base_deadline_marker(head))?;
    Ok(ruled.then(|| head.clone()))
}

/// The head the Engineer ruled `task` through with no check: the pull request's, when it has no
/// check at all and a ruled question on the ticket ends with the marker of that head
/// ([`no_check_marker`]). The board is read only for a pull request that is open at a head with no
/// check and that opened [`NO_CHECK_DEADLINE`] before `now` or earlier, so most gathers read
/// nothing: a no-check question is asked only once the wait at the head is that old, and the
/// wait's clock is never earlier than the pull request's opening, so a younger pull request has no
/// such question yet. A ruling on an older head matches nothing at this one.
fn ruled_no_check(
    board: &dyn Board,
    task: TicketId,
    pull: Option<&PullFacts>,
    now: SystemTime,
) -> Result<Option<CommitId>, PortError> {
    let Some(head) = pull
        .filter(|pull| pull.merged_at.is_none() && pull.checks.is_empty())
        .filter(|pull| {
            pull.opened_at
                .checked_add(NO_CHECK_DEADLINE)
                .is_some_and(|due| due <= now)
        })
        .and_then(|pull| pull.head.as_ref())
    else {
        return Ok(None);
    };
    let ruled = ruled_ending(board, task, &no_check_marker(head))?;
    Ok(ruled.then(|| head.clone()))
}

/// Whether a ruled question on `task` ends with `marker`.
fn ruled_ending(board: &dyn Board, task: TicketId, marker: &str) -> Result<bool, PortError> {
    Ok(board
        .questions_on(task)?
        .iter()
        .filter(|(_, ruling)| ruling.is_some())
        .any(|(question, _)| {
            crate::ports::NewQuestion::text_of(question)
                .trim()
                .ends_with(marker)
        }))
}

/// Orders the merges of `tickets` ([`order_merges`]) among those that share an integration
/// branch, and leaves the tickets in ticket order: all of them under the usual pattern, which
/// names the version alone; the tickets of each epic apart under one that names `{epic}`; and
/// none under one that names `{task}`, where every ticket has a branch of its own.
fn order_by_branch(branches: &BranchesSection, tickets: &mut [TicketFacts]) {
    let pattern = branches.integration.as_ref();
    if pattern.contains(TASK_PLACEHOLDER) {
        return;
    }
    if !pattern.contains(EPIC_PLACEHOLDER) {
        order_merges(tickets);
        return;
    }
    let epic_of = |facts: &TicketFacts| facts.epic.map(|epic| epic.id);
    tickets.sort_by_key(|facts| (epic_of(facts), facts.ticket));
    for of_one_epic in tickets.chunk_by_mut(|one, other| epic_of(one) == epic_of(other)) {
        order_merges(of_one_epic);
    }
    tickets.sort_by_key(|facts| facts.ticket);
}

/// The forge's facts about `branch`, as the core reads them: the commit it is at and the checks
/// there. A branch the forge does not have is unknown. A finished check with no time is an
/// error, never a guess.
pub fn read_base(forge: &dyn Forge, branch: &BranchName) -> Result<BaseFacts, String> {
    let read = forge
        .branch_checks(branch)
        .map_err(|error| error.to_string())?;
    let Some(head) = read.head else {
        return Ok(BaseFacts::default());
    };
    Ok(BaseFacts {
        head: Some(head),
        checks: check_facts(read.checks)?,
        reached_at: read.reached_at,
    })
}

/// [`gather`] for the project `composition` loads, over `board`, as the tick gathers it: its
/// manifest, its forge, its root, its state folder, the zaf home's actions and this machine's
/// process table. None with no `[phases]`, where nothing is placed. A forge that cannot be built,
/// or a read every ticket needs that fails, is the error, in words. `version` names the
/// integration branch; with none, it is the one this process was started under (`ZAF_VERSION`),
/// or else the one the project's running scheduler ticks for, so `zaf status` and the hub place a
/// ticket as the tick does.
pub fn gather_for(
    composition: &Composition,
    board: &dyn Board,
    version: Option<&str>,
    epics: &[Epic],
    questions: &[Ticket],
) -> Result<Option<Gathered>, String> {
    let manifest = composition.manifest();
    if manifest.phases.is_empty() {
        return Ok(None);
    }
    let forge = composition.forge().map_err(|error| error.to_string())?;
    // A project with no [vcs] counts getting nowhere by commit id, as before.
    let vcs = composition.vcs().ok();
    let root = composition.root();
    let state_dir = composition.state_dir();
    // With no home there is no machine tier, so the project's actions alone are looked for.
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    let version = version
        .map(str::to_owned)
        .or_else(|| std::env::var(VERSION_VAR).ok())
        .filter(|version| !version.is_empty())
        .or_else(|| lock::running(&state_dir, &process::alive).map(|holding| holding.version));
    // A filter that cannot be built leaves every check expected, as a failed read does.
    let ci_filter = composition.ci_filter().ok();
    let ports = Ports {
        board,
        forge: forge.as_ref(),
        ci_filter: ci_filter.as_deref(),
        manifest,
        root,
        home: &home,
        state_dir: &state_dir,
        alive: &process::alive,
        version: version.as_deref(),
        now: SystemTime::now(),
        vcs: vcs.as_deref(),
    };
    gather(&ports, epics, questions)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// One ticket gathered as [`gather`] gathers each: its facts and what is read beside them, or none
/// when it is no open ticket whose type has a phase list (done, closed, or of a type with no
/// list). The open tickets, the open epics and the open questions are read as for every ticket.
/// A ticket the tick would skip is the [`Skip`] the tick names it by: one that cannot run, or
/// one something about which fails to read, the lists every ticket shares included.
///
/// A ticket at its merge ([`at_merge`]) is placed with what the other tickets say, as the tick
/// places it: the other open tickets are then read too, those that cannot be read left out, and
/// the integration branch's checks, a read that fails leaving the base unknown. Any other ticket
/// reads nothing more.
pub fn gather_one(
    ports: &Ports<'_>,
    task: TicketId,
) -> Result<Option<(TicketFacts, Beside)>, Skip> {
    gather_one_in(ports, task, None)
}

/// [`gather_one`] for a merge in a tick's group: the re-check before the merge, which judges R29
/// at the group's base while the branch is still at the group's own last commit (`group`,
/// [`GroupBase::judged`]), and as the branch reads at any other head. With no `group` it is
/// [`gather_one`].
pub fn gather_one_in(
    ports: &Ports<'_>,
    task: TicketId,
    group: Option<&GroupBase>,
) -> Result<Option<(TicketFacts, Beside)>, Skip> {
    let unread = |error: PortError| Skip::Unread(error.to_string());
    let open = ports.board.open_tickets().map_err(unread)?;
    let Some(ticket) = open.iter().find(|open| open.ticket.id == task) else {
        return Ok(None);
    };
    let (epics, questions) = read_lists(ports.board).map_err(unread)?;
    let shared = shared(&epics, &questions);
    let mut standings = BTreeMap::new();
    // A ruling that cannot be read leaves the ticket waiting on its checks, which is the refusal
    // the caller gives; the tick's gather is the one that words it.
    let mut warnings = Vec::new();
    let Some((facts, beside)) = read_ticket(ports, &shared, &mut standings, &mut warnings, ticket)?
    else {
        return Ok(None);
    };
    if !at_merge(&facts) {
        return Ok(Some((facts, beside)));
    }
    let mut tickets = vec![facts];
    for other in open.iter().filter(|open| open.ticket.id != task) {
        if let Ok(Some((facts, _))) =
            read_ticket(ports, &shared, &mut standings, &mut warnings, other)
        {
            tickets.push(facts);
        }
    }
    // A base that cannot be read is unknown, which holds nothing back, as in the tick.
    let _ = merging(ports, &mut tickets, group);
    let at = tickets.iter().position(|facts| facts.ticket == task);
    Ok(at.map(|at| (tickets.swap_remove(at), beside)))
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

/// [`gather`] over the lists `ports.board` gives, read here once: what a test with no verb above
/// it to read them uses.
#[cfg(test)]
pub(crate) fn gather_listing(ports: &Ports<'_>) -> Result<Gathered, PortError> {
    let (epics, questions) = read_lists(ports.board)?;
    gather(ports, &epics, &questions)
}

/// The open epics and the open questions, one read each: what a verb that shows or places tickets
/// reads at its start and hands down. A board that cannot list its epics at all
/// ([`PortError::Rejected`]) has none, which holds nothing back.
pub fn read_lists(board: &dyn Board) -> Result<(Vec<Epic>, Vec<Ticket>), PortError> {
    let epics = match board.epics() {
        Ok(epics) => epics,
        Err(PortError::Rejected(_)) => Vec::new(),
        Err(error) => return Err(error),
    };
    Ok((epics, board.questions()?))
}

/// The reads every ticket shares, from the open epics and the open questions given.
fn shared(epics: &[Epic], questions: &[Ticket]) -> Shared {
    Shared {
        tasks: epics.iter().flat_map(|epic| epic.tasks.clone()).collect(),
        epics: epics
            .iter()
            .map(|epic| (epic.id, epic_facts(epic)))
            .collect(),
        questions: questions_by_issue(questions),
        listed: epics.to_vec(),
        workflows: RefCell::default(),
    }
}

/// Whether a check is expected on a pull request that changes `changed` (D17): the rule of
/// [`checks_expected`] over the workflows at the integration branch, which every pull request of
/// the gather merges into. The workflows are read once per branch and kept in `shared`. Every
/// doubt expects a check: no filter port, no one integration branch to name, or a read that
/// fails (kept as failed, so it is not asked again).
fn expects_a_check(
    ports: &Ports<'_>,
    shared: &Shared,
    changed: &ChangedFiles,
    ticket: &Ticket,
) -> bool {
    // The slice's branch, for a workflow that runs on a push to it; a name that cannot be filled
    // is no name, which a branch filter reads as a check expected.
    let slice = crate::app::slice::fill(&ports.manifest.branches.task, ticket, ports.version).ok();
    let Some(filter) = ports.ci_filter else {
        return true;
    };
    let Some(base) = integration_branch(&ports.manifest.branches, ports.version) else {
        return true;
    };
    let mut read = shared.workflows.borrow_mut();
    let workflows = read
        .entry(base)
        .or_insert_with_key(|base| filter.workflows(base).ok());
    workflows.as_deref().is_none_or(|workflows| {
        checks_expected(
            workflows,
            &changed.paths,
            changed.cut_short,
            slice.as_deref(),
        )
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
/// A read of its ruled no-check questions that fails ([`ruled_no_check`]) is a line in `warnings`
/// and leaves the ticket waiting on its checks.
fn read_ticket(
    ports: &Ports<'_>,
    shared: &Shared,
    standings: &mut BTreeMap<TicketId, StandingAt>,
    warnings: &mut Vec<String>,
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
                let at = finished_epic_standing(blocker, at, &shared.listed);
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
    let mut pull = match recorded {
        Some(pr) => Some(
            read_pull(ports.forge, pr)
                .map_err(|reason| Skip::Unread(format!("pull request {pr}: {reason}")))?,
        ),
        None => None,
    };
    // Whether a check is expected matters only to an open pull request with none at its head, so
    // the workflows are read for no other.
    if let Some(open) = pull
        .as_mut()
        .filter(|pull| pull.merged_at.is_none() && pull.checks.is_empty())
    {
        open.checks_expected = expects_a_check(ports, shared, &open.changed_files, ticket);
    }
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
    let trees = read_trees(
        ports,
        pull.as_ref().and_then(|pull| pull.head.as_ref()),
        &runs,
        &counting,
    );
    let on_epic = shared.questions.get(&epic_id).cloned().unwrap_or_default();
    let no_check_ruled = match ruled_no_check(ports.board, ticket.id, pull.as_ref(), ports.now) {
        Ok(ruled) => ruled,
        Err(error) => {
            warnings.push(format!(
                "{}: its ruled questions were not read, so it still waits on the checks of its \
                 pull request's head: {error}",
                ticket.id
            ));
            None
        }
    };
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
        // Filled for all the tickets at once, when every one is read ([`merging`]).
        merging: MergeFacts::default(),
        no_check_ruled,
        trees,
    };
    let beside = Beside {
        list,
        epic: epic_id,
        on_epic,
    };
    Ok(Some((facts, beside)))
}

/// The trees of the pull request's head and of the heads the runs of the `counting` actions began
/// and ended at, for R18 to count by tree ([`Trees`]). Read from the VCS now and stored nowhere. A
/// commit whose tree cannot be read is left out, and counts by its id; with no VCS or no head
/// nothing is read.
fn read_trees(
    ports: &Ports<'_>,
    head: Option<&CommitId>,
    runs: &[RunRecord],
    counting: &[ActionName],
) -> Trees {
    let mut trees = Trees::default();
    let (Some(vcs), Some(head)) = (ports.vcs, head) else {
        return trees;
    };
    let commits: BTreeSet<&CommitId> = runs
        .iter()
        .filter(|run| counting.contains(&run.action))
        .flat_map(|run| {
            let left = match &run.head_end {
                HeadEnd::Left(left) => left.as_ref(),
                HeadEnd::Unknown => None,
            };
            [run.head.as_ref(), left]
        })
        .flatten()
        .chain(std::iter::once(head))
        .collect();
    let commits: Vec<&CommitId> = commits.into_iter().collect();
    for (commit, tree) in vcs.trees_of(ports.root, &commits) {
        trees.insert(commit, tree);
    }
    trees
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
    // One read of the pull request gives its times, head and merged state; no memory lives past it.
    let request = forge.pull_request(pr).map_err(failed)?;
    let merged_at = if request.merged {
        Some(
            request
                .merged_at
                .ok_or_else(|| "it is merged but the forge gives no merge time".to_owned())?,
        )
    } else {
        None
    };
    let changed_files = forge.changed_files(pr).map_err(failed)?;
    let checks = check_facts(
        forge
            .checks_at(pr, request.head.as_deref())
            .map_err(failed)?,
    )?;
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
        head: request.head.map(crate::domain::CommitId::from),
        changed_files,
        checks,
        reviews,
        mergeable: request.mergeable,
        // Settled by the caller, which reads the workflows only for a pull request with no check.
        checks_expected: true,
    })
}

/// The forge's checks as the core reads them, in the forge's order. A finished check with no
/// time is an error, never a guess.
fn check_facts(checks: Vec<Check>) -> Result<Vec<CheckFact>, String> {
    checks
        .into_iter()
        .map(|check| {
            let result = match (check.state, check.finished_at) {
                (CheckState::Pending, _) => CheckResult::Pending,
                (CheckState::Passed, Some(at)) => CheckResult::Passed(at),
                (CheckState::Failed, Some(at)) => CheckResult::Failed(at),
                (CheckState::Passed | CheckState::Failed, None) => {
                    return Err(format!("check {} finished with no time", check.name));
                }
            };
            Ok(CheckFact {
                name: check.name,
                result,
            })
        })
        .collect()
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
