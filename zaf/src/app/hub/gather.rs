//! Reads the board, the ledger, the spawner's slots, the phases and the last reports into a
//! [`Snapshot`]. Nothing here writes: a failed read is an error, never a guessed row.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::SystemTime;

use super::reports::{WORKING, last_report};
use super::{EpicRow, GroupRow, Snapshot, TicketRow};
use crate::app::actions::{PhaseUnreadable, SliceRecords, derive_phase, gather_phase_facts};
use crate::app::report::PriceTable;
use crate::app::report::epics::cost_of;
use crate::app::report::ledger::{LedgerRow, is_this_project};
use crate::domain::{IssueNumber, TaskLabel};
use crate::ports::{Board, EpicTask, EpicTree, Forge, NewQuestion, PortError, Vcs};

/// Why a frame could not be gathered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HubError {
    /// The board failed.
    #[error("the board failed: {0}")]
    Board(PortError),
    /// A claimed task's phase could not be read.
    #[error("{0}")]
    Phase(#[from] PhaseUnreadable),
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
    /// The spawner's slots with no instance in them.
    pub slots_free: u32,
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
}

/// Reads one snapshot: one row per open epic, with its claimed tasks and groups.
pub fn gather(inputs: &HubInputs) -> Result<Snapshot, HubError> {
    let trees = inputs.board.epic_trees().map_err(HubError::Board)?;
    let claims = inputs.board.claims().map_err(HubError::Board)?;
    let questions = inputs.board.questions().map_err(HubError::Board)?;
    let holders: HashMap<IssueNumber, String> = claims
        .iter()
        .map(|claim| (claim.task, who_is(claim.instance.role.as_ref(), inputs)))
        .collect();
    let epic_of: HashMap<u64, u64> = trees
        .iter()
        .flat_map(|tree| {
            tree.tasks
                .iter()
                .map(move |task| (task.number.get(), tree.number.get()))
        })
        .collect();
    let dollars = dollars_by_epic(inputs, &epic_of);
    let mut epics = Vec::new();
    for tree in &trees {
        let asked = questions
            .iter()
            .filter(|question| {
                NewQuestion::raised_on(&question.body).is_some_and(|on| {
                    on == tree.number || tree.tasks.iter().any(|task| task.number == on)
                })
            })
            .count();
        epics.push(epic_row(
            inputs,
            tree,
            &holders,
            asked,
            dollars.get(&tree.number.get()).copied().unwrap_or(0.0),
        )?);
    }
    Ok(Snapshot { epics })
}

fn who_is(role: &str, inputs: &HubInputs) -> String {
    match inputs.models.get(role) {
        Some(model) => format!("{model} {role}"),
        None => role.to_owned(),
    }
}

/// What this project's ledger rows cost, by epic: the epic of the first task a row names that
/// an open epic holds, else the epic the row itself names.
fn dollars_by_epic(inputs: &HubInputs, epic_of: &HashMap<u64, u64>) -> HashMap<u64, f64> {
    let mut by_epic: HashMap<u64, f64> = HashMap::new();
    for row in inputs.ledger {
        if !is_this_project(&row.project, inputs.project, inputs.root) {
            continue;
        }
        let epic = row
            .tasks
            .iter()
            .find_map(|task| epic_of.get(task).copied())
            .or(row.epic);
        if let Some(epic) = epic {
            *by_epic.entry(epic).or_insert(0.0) += cost_of(row, inputs.prices);
        }
    }
    by_epic
}

/// Whether a task counts toward its epic: an open one, or one that is done. A closed task that
/// is not done counts for nothing.
fn counts(task: &EpicTask) -> bool {
    !task.closed || task.label == Some(TaskLabel::Done)
}

fn is_done(task: &EpicTask) -> bool {
    task.label == Some(TaskLabel::Done)
}

fn tally<'a>(tasks: impl Iterator<Item = &'a EpicTask>) -> (u32, u32) {
    tasks
        .filter(|task| counts(task))
        .fold((0, 0), |(done, total), task| {
            (done + u32::from(is_done(task)), total + 1)
        })
}

fn epic_row(
    inputs: &HubInputs,
    tree: &EpicTree,
    holders: &HashMap<IssueNumber, String>,
    questions: usize,
    dollars: f64,
) -> Result<EpicRow, HubError> {
    let (done, total) = tally(tree.tasks.iter());
    let ready = tree
        .tasks
        .iter()
        .filter(|task| !task.closed && task.label == Some(TaskLabel::Ready))
        .count();
    let mut row = EpicRow {
        number: tree.number.get(),
        done,
        total,
        dollars,
        slots_free: inputs.slots_free,
        slots_total: inputs.slots_total,
        ready: u32::try_from(ready).unwrap_or(u32::MAX),
        questions: u32::try_from(questions).unwrap_or(u32::MAX),
        tickets: Vec::new(),
        groups: Vec::new(),
    };
    let mut groups: BTreeMap<&str, Vec<&EpicTask>> = BTreeMap::new();
    for task in &tree.tasks {
        match &task.group {
            Some(name) if counts(task) => groups.entry(name).or_default().push(task),
            Some(_) => {}
            None if is_claimed(task) => row.tickets.push(ticket_row(inputs, task, holders)?),
            None => {}
        }
    }
    for (name, tasks) in groups {
        let (done, total) = tally(tasks.iter().copied());
        let mut tickets = Vec::new();
        for task in tasks.iter().filter(|task| is_claimed(task)) {
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

fn is_claimed(task: &EpicTask) -> bool {
    !task.closed && task.label == Some(TaskLabel::Claimed)
}

fn ticket_row(
    inputs: &HubInputs,
    task: &EpicTask,
    holders: &HashMap<IssueNumber, String>,
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
        who: holders
            .get(&task.number)
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        phase: derive_phase(&facts),
        word,
        since,
    })
}
