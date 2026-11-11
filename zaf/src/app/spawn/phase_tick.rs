//! The phase steps of `zaf spawn --once`: steps 3 to 6 of the tick (docs/zaf-actions.md §5.1),
//! on every claimed or ready task whose type has a `[phases]` list.
//!
//! [`gather`] reads each such task's phase list, its size and its standing facts (the forge's word
//! on its pull request and its phase run folders). A task whose type has no list is the team's and
//! is left out here; [`phased`] says which is which, so the team plan leaves the phased ones out in
//! turn. A task that cannot be read is named in [`Gathered::unreadable`] and skipped, and the others
//! still go.
//!
//! [`run_steps`] then runs the steps in the order of §5.1 and merges what each did into one
//! [`TickOutcome`]: step 3 runs the ready `zaf`-kind actions ([`run_zaf_actions`]), step 4 launches
//! the ready waits ([`launch_waits`]), step 5 raises a question on each wait that failed with no
//! `on-fail` ([`ask_failed_waits`]), and step 6 starts at most one model phase
//! ([`start_planned`]). A failed model start records nothing as started and tries no other item;
//! it is named in [`PhaseTick::failed`], and the next tick plans it again.
//!
//! Nothing about a standing is stored: each tick derives it again from the forge and the run
//! folders, and only the steps' own acts (a run, a launch, a question) leave a trace.

use std::collections::BTreeMap;
use std::time::SystemTime;

use super::inline::{TickTask, run_zaf_actions};
use super::outcome::TickOutcome;
use super::phase::{Ports, listed};
use super::question::ask_failed_waits;
use super::start::{InUse, PlannedPhase, TaskStanding, start_planned};
use super::waits::launch_waits;
use crate::app::actions::{ActionName, gather_standing_facts};
use crate::domain::{Budget, DEFAULT_TASK_TYPE, TaskSize, Ticket, TicketId};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError};

/// The type `ticket` is read as: its issue type, or [`DEFAULT_TASK_TYPE`] when it has none.
fn task_type(ticket: &Ticket) -> &str {
    ticket
        .kind
        .as_ref()
        .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str())
}

/// Whether `ticket`'s type has a `[phases]` list: a phased task runs through the phase steps and
/// never goes to a team.
pub fn phased(manifest: &Manifest, ticket: &Ticket) -> bool {
    manifest.phase_list(task_type(ticket)).is_some()
}

/// One phased task as the steps read it: its list and facts, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseTask {
    /// Its phase list and standing facts.
    pub tick: TickTask,
    /// Its size, from its ticket: what a model phase of it costs.
    pub size: TaskSize,
}

/// A task whose standing could not be read, and why; it is skipped this tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// The task.
    pub task: TicketId,
    /// What could not be read.
    pub reason: String,
}

/// What [`gather`] read: the phased tasks, in task order, and the ones it could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gathered {
    /// Each claimed or ready task whose type has a phase list, the lowest number first.
    pub tasks: Vec<PhaseTask>,
    /// The tasks skipped because something about them could not be read.
    pub unreadable: Vec<Unreadable>,
}

/// Reads every claimed or ready task whose type has a phase list, the lowest number first (so a
/// tie in step 6 goes to the lower task, the #497 ruling), with its standing facts as of `now`.
///
/// `now` stands for when each task became ready for its first phase, which the board does not
/// keep. A task that cannot be read (the board, its phase list's actions, its template or its
/// standing facts) is named in [`Gathered::unreadable`] and the others are still read. Only the
/// board's lists of ready and claimed tasks failing fails the whole read. Nothing is written.
pub fn gather(ports: &Ports<'_>, now: SystemTime) -> Result<Gathered, PortError> {
    let mut tickets: BTreeMap<TicketId, Option<Ticket>> = ports
        .board
        .ready()?
        .into_iter()
        .map(|ticket| (ticket.id, Some(ticket)))
        .collect();
    for claim in ports.board.claims()? {
        tickets.entry(claim.task).or_insert(None);
    }
    let mut gathered = Gathered::default();
    for (task, ticket) in tickets {
        match read_task(ports, task, ticket, now) {
            Ok(Some(read)) => gathered.tasks.push(read),
            Ok(None) => {}
            Err(reason) => gathered.unreadable.push(Unreadable { task, reason }),
        }
    }
    Ok(gathered)
}

/// One task: none when its type has no phase list, an error naming what could not be read.
fn read_task(
    ports: &Ports<'_>,
    task: TicketId,
    ticket: Option<Ticket>,
    now: SystemTime,
) -> Result<Option<PhaseTask>, String> {
    let ticket = match ticket {
        Some(ticket) => ticket,
        None => ports.board.task(task).map_err(|error| error.to_string())?,
    };
    let Some(names) = ports.manifest.phase_list(task_type(&ticket)) else {
        return Ok(None);
    };
    let list = listed(ports.root, ports.home, names).map_err(|error| error.to_string())?;
    let size = ticket
        .template()
        .map_err(|error| format!("it is not a task: {error}"))?
        .size;
    let facts = gather_standing_facts(ports.forge, ports.root, task, now, ports.alive)
        .map_err(|error| error.reason)?;
    Ok(Some(PhaseTask {
        tick: TickTask { task, list, facts },
        size,
    }))
}

/// What the steps act through, each performing one act and returning once it is done or launched.
pub struct Steps<'a> {
    /// Step 3: runs `zaf do <action> <task>` and gives its exit code.
    pub run: &'a mut dyn FnMut(TicketId, &ActionName) -> u8,
    /// Step 4: launches a wait with `zaf spawn phase <task> <action>`, never waiting for it.
    pub launch: &'a mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    /// Step 6: starts the planned model phase with `zaf spawn phase`, never waiting for it.
    pub start: &'a mut dyn FnMut(&PlannedPhase) -> Result<(), String>,
    /// Where a wait that could not launch is logged.
    pub log: &'a mut dyn FnMut(&str),
}

/// What the phase steps did: the merged outcome, and every step that failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhaseTick {
    /// What steps 3 to 6 did.
    pub outcome: TickOutcome,
    /// What failed: the questions' board, or the model phase's start. Either fails the tick.
    pub failed: Vec<String>,
}

/// Runs steps 3 to 6 on `tasks`, in that order, and merges what each did into one outcome.
///
/// Step 6 plans its one model phase with `in_use` and `budget` from the standings as they were
/// derived for this tick. A start that fails leaves [`TickOutcome::started`] empty, tries no other
/// item and is named in [`PhaseTick::failed`]; a board that fails step 5 is named there too, and
/// step 6 still runs.
pub fn run_steps(
    board: &dyn Board,
    tasks: &[PhaseTask],
    in_use: &InUse,
    budget: &Budget,
    steps: &mut Steps<'_>,
) -> PhaseTick {
    let ticks: Vec<TickTask> = tasks.iter().map(|task| task.tick.clone()).collect();
    let mut tick = PhaseTick::default();
    tick.outcome.ran = run_zaf_actions(&ticks, steps.run);
    tick.outcome.waits = launch_waits(&ticks, steps.launch, steps.log);
    match ask_failed_waits(board, &ticks) {
        Ok(questions) => tick.outcome.questions = questions,
        Err(error) => tick
            .failed
            .push(format!("cannot raise a question: {error}")),
    }
    let standings: Vec<TaskStanding> = tasks
        .iter()
        .map(|task| TaskStanding::of(&task.tick, task.size))
        .collect();
    match start_planned(&standings, in_use, budget, steps.start) {
        Ok(started) => tick.outcome.started = started,
        Err(error) => tick.failed.push(error),
    }
    tick
}

#[cfg(test)]
mod tests;
