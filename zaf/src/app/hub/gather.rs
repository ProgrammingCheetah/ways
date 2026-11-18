//! Reads the board, the ledger, the spawner's slots, the phases and the last reports into a
//! [`Snapshot`]. Nothing here writes: a failed read is an error, never a guessed row.
//!
//! A task's state as shown is where the core places it, worded as its label would be
//! ([`shown_state`], R5): a ticket in the lane shows `ready`, one that runs shows `claimed`. A task
//! the core does not place (no `[phases]`, or a type with no list) shows its label.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::SystemTime;

use super::Liveness;
use super::phases::plans_of;
use super::reports::{WORKING, last_report};
use super::{EpicRow, GroupRow, ModelRow, Snapshot, TicketRow};
use crate::app::actions::{PhaseUnreadable, SliceRecords, derive_phase, gather_phase_facts};
use crate::app::report::PriceTable;
use crate::app::report::epics::cost_of;
use crate::app::report::ledger::{LedgerRow, is_this_project};
use crate::domain::schedule::{Place, TicketFacts, WaitingOn, end_fact_holds, label_for};
use crate::domain::{Epic, EpicTask, Standing, TicketId, TicketState};
use crate::ports::{Board, Forge, Holder, NewQuestion, PortError, Vcs};

/// Why a frame could not be gathered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubError {
    /// The board failed.
    #[error("the board failed: {0}")]
    Board(PortError),
    /// A claimed task's phase could not be read.
    #[error("{0}")]
    Phase(#[from] PhaseUnreadable),
    /// The run folders could not be read.
    #[error("the run folders could not be read: {0}")]
    Spawn(String),
}

/// What [`gather`] reads from.
pub struct HubInputs<'a> {
    /// The board.
    pub board: &'a dyn Board,
    /// The forge, for the phases.
    pub forge: &'a dyn Forge,
    /// The version control, for the phases.
    pub vcs: &'a dyn Vcs,
    /// The slice records, for the phases.
    pub records: &'a SliceRecords,
    /// The project's state folder, where the last reports are.
    pub state_dir: &'a Path,
    /// Whether a recorded process still runs: what `zaf status` asks, so both count alike.
    pub alive: &'a dyn Fn(&Holder) -> bool,
    /// All the spawner's slots.
    pub slots_total: u32,
    /// Every row of the ledger, this machine's: the gather keeps this project's.
    pub ledger: &'a [LedgerRow],
    /// This project's name.
    pub project: &'a str,
    /// This project's root.
    pub root: &'a Path,
    /// What a run costs.
    pub prices: &'a PriceTable,
    /// The model each role runs on, by role name.
    pub models: &'a BTreeMap<String, String>,
    /// The time the frame is of.
    pub now: SystemTime,
    /// Where the core places each open ticket with a phase list at `now`; none when nothing was
    /// placed (no `[phases]`, or the gather failed), and every task shows its label.
    pub places: Option<&'a Places>,
}

/// Where the core places each ticket it places, by ticket.
pub type Places = BTreeMap<TicketId, Place>;

/// The state `task` shows: open, where the core places it ([`label_for`]), `claimed` saying
/// whether a claim holds it; its label when the core does not place it. None for a task that is
/// not open, or that the core places done.
pub fn shown_state(task: &EpicTask, places: Option<&Places>, claimed: bool) -> Option<TicketState> {
    if task.standing != Standing::Open {
        return None;
    }
    match places.and_then(|places| places.get(&task.number)) {
        Some(place) => Some(label_for(place, claimed)).filter(|state| *state != TicketState::Done),
        None => task.state,
    }
}

/// The colour of a task's line (epic #115 §2): the first of these six that holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineState {
    /// The task is done.
    Done,
    /// A question open on the task waits for the Engineer.
    Question,
    /// A phase run of it is live.
    Working,
    /// It waits on a ticket that is not done.
    Blocked,
    /// It has begun (a phase has ended or a run is recorded) and waits for the next tick.
    WaitingTick,
    /// Nothing of it has begun.
    NotStarted,
}

/// The line colour of `task`, worked out from where the core places it (`place`) and the facts
/// that placement read (`facts`).
///
/// A done task is `done` whatever else holds. A question open on the task is `question` even
/// while a run of it is live; `epic_questions` are the questions raised on the task's epic itself,
/// which colour no task (the Engineer's ruling on #128: the epic shows them). `working` is a live
/// run, `blocked` a wait on a ticket not done, `waiting_tick` any other task with a phase ended or
/// a run recorded, and `not_started` the rest. A task the core does not place (`place` is none)
/// reads its label: done, question, claimed (working), blocked, else `not_started`.
pub fn line_state(
    task: &EpicTask,
    place: Option<&Place>,
    facts: Option<&TicketFacts>,
    epic_questions: &BTreeSet<TicketId>,
) -> LineState {
    if task.standing == Standing::Done || matches!(place, Some(Place::Done)) {
        return LineState::Done;
    }
    let Some(place) = place else {
        return match task.state {
            Some(TicketState::Done) => LineState::Done,
            Some(TicketState::Question) => LineState::Question,
            Some(TicketState::Claimed) => LineState::Working,
            Some(TicketState::Blocked) => LineState::Blocked,
            _ => LineState::NotStarted,
        };
    };
    let waits = match place {
        Place::Waiting(waits) => Some(waits),
        _ => None,
    };
    let own_question = waits.is_some_and(|waits| {
        waits.iter().any(|waiting_on| {
            matches!(waiting_on, WaitingOn::Question(question) if !epic_questions.contains(question))
        })
    }) || facts.is_some_and(|facts| {
        facts
            .open_questions
            .iter()
            .any(|question| !epic_questions.contains(question))
    });
    if own_question {
        return LineState::Question;
    }
    if matches!(place, Place::Running(_)) {
        return LineState::Working;
    }
    if waits.is_some_and(|waits| {
        waits
            .iter()
            .any(|waiting_on| matches!(waiting_on, WaitingOn::Ticket(_)))
    }) {
        return LineState::Blocked;
    }
    if facts.is_some_and(has_begun) {
        LineState::WaitingTick
    } else {
        LineState::NotStarted
    }
}

/// Whether a phase of the ticket has ended or a run of it is recorded.
pub fn has_begun(facts: &TicketFacts) -> bool {
    !facts.runs.is_empty()
        || facts
            .phases
            .iter()
            .any(|phase| end_fact_holds(phase, facts))
}

/// Reads one snapshot: one row per open epic, with its claimed tasks and groups.
pub fn gather(inputs: &HubInputs) -> Result<Snapshot, HubError> {
    let trees = inputs.board.epics().map_err(HubError::Board)?;
    let claims = inputs.board.claims().map_err(HubError::Board)?;
    let questions = inputs.board.questions().map_err(HubError::Board)?;
    let live = Liveness::read(inputs.state_dir, inputs.slots_total, &claims, inputs.alive)?;
    let holders: HashMap<TicketId, String> = claims
        .iter()
        .map(|claim| (claim.task, who_is(claim.instance.role.as_ref(), inputs)))
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
    let mut epics = Vec::new();
    for tree in &trees {
        let asked = questions
            .iter()
            .filter(|question| {
                NewQuestion::raised_on(question.body.as_str()).is_some_and(|on| {
                    on == tree.id || tree.tasks.iter().any(|task| task.number == on)
                })
            })
            .count();
        epics.push(epic_row(
            inputs,
            tree,
            &holders,
            &live,
            asked,
            dollars.get(&tree.id.get()).copied().unwrap_or(0.0),
        )?);
    }
    Ok(Snapshot { epics, claims })
}

fn who_is(role: &str, inputs: &HubInputs) -> String {
    match inputs.models.get(role) {
        Some(model) => format!("{model} {role}"),
        None => role.to_owned(),
    }
}

/// What this project's ledger rows cost, by epic: the epic of the first task a row names that
/// an open epic holds, else the epic the row itself names.
pub(super) fn dollars_by_epic(
    ledger: &[LedgerRow],
    project: &str,
    root: &Path,
    prices: &PriceTable,
    epic_of: &HashMap<u64, u64>,
) -> HashMap<u64, f64> {
    let mut by_epic: HashMap<u64, f64> = HashMap::new();
    for row in ledger {
        if !is_this_project(&row.project, project, root) {
            continue;
        }
        let epic = row
            .tasks
            .iter()
            .find_map(|task| epic_of.get(task).copied())
            .or(row.epic);
        if let Some(epic) = epic {
            *by_epic.entry(epic).or_insert(0.0) += cost_of(row, prices);
        }
    }
    by_epic
}

/// The done tasks and the tasks that count (see [`EpicTask::counts`]): a task closed as not
/// planned counts for nothing.
pub(super) fn tally<'a>(tasks: impl Iterator<Item = &'a EpicTask>) -> (u32, u32) {
    tasks
        .filter(|task| task.counts())
        .fold((0, 0), |(done, total), task| {
            (done + u32::from(task.done()), total + 1)
        })
}

fn epic_row(
    inputs: &HubInputs,
    tree: &Epic,
    holders: &HashMap<TicketId, String>,
    live: &Liveness,
    questions: usize,
    dollars: f64,
) -> Result<EpicRow, HubError> {
    let (done, total) = tally(tree.tasks.iter());
    let shown =
        |task: &EpicTask| shown_state(task, inputs.places, holders.contains_key(&task.number));
    // Drawn as running: shown claimed, and the claim not held by an instance that has ended.
    let running = |task: &EpicTask| {
        shown(task) == Some(TicketState::Claimed) && !live.orphaned.contains(&task.number)
    };
    let ready = tree
        .tasks
        .iter()
        .filter(|task| shown(task) == Some(TicketState::Ready))
        .count();
    let mut row = EpicRow {
        number: tree.id.get(),
        title: tree.title.to_string(),
        stage: tree.stage(),
        done,
        total,
        dollars,
        slots_free: live.slots_free,
        slots_total: inputs.slots_total,
        ready: u32::try_from(ready).unwrap_or(u32::MAX),
        questions: u32::try_from(questions).unwrap_or(u32::MAX),
        tickets: Vec::new(),
        groups: Vec::new(),
    };
    let mut groups: BTreeMap<&str, Vec<&EpicTask>> = BTreeMap::new();
    for task in &tree.tasks {
        match &task.group {
            Some(name) if task.counts() => groups.entry(name.as_str()).or_default().push(task),
            Some(_) => {}
            None if running(task) => row.tickets.push(ticket_row(inputs, task, holders)?),
            None => {}
        }
    }
    for (name, tasks) in groups {
        let (done, total) = tally(tasks.iter().copied());
        let mut tickets = Vec::new();
        for task in tasks.iter().filter(|task| running(task)) {
            tickets.push(ticket_row(inputs, task, holders)?);
        }
        row.groups.push(GroupRow {
            name: name.to_owned(),
            done,
            total,
            tickets,
        });
    }
    Ok(row)
}

fn ticket_row(
    inputs: &HubInputs,
    task: &EpicTask,
    holders: &HashMap<TicketId, String>,
) -> Result<TicketRow, HubError> {
    let facts = gather_phase_facts(
        inputs.board,
        inputs.forge,
        inputs.vcs,
        inputs.records,
        task.number,
    )?;
    let (word, since) = match last_report(inputs.state_dir, task.number) {
        Some((word, made)) => (word, inputs.now.duration_since(made).ok()),
        None => (WORKING.to_owned(), None),
    };
    Ok(TicketRow {
        number: task.number.get(),
        title: task.title.to_string(),
        who: holders
            .get(&task.number)
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        phase: derive_phase(&facts),
        word,
        since,
        models: plans_of(inputs.state_dir, task.number)
            .into_iter()
            .map(|(plan, _)| ModelRow {
                iteration: plan.iteration(),
                current: plan.current().map(str::to_owned),
                role: plan.role,
                phases: plan.phases,
            })
            .collect(),
    })
}
