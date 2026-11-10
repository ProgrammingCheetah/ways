//! `zaf board ready|claim|report|question|ruling|done`: reading and writing the board.

use crate::app::actions::{Phase, PhaseUnreadable, SliceRecords, derive_phase, gather_phase_facts};
use crate::domain::{Instance, Ticket, TicketId, TicketState};
use crate::ports::{Board, ClaimOutcome, Forge, NewQuestion, PortError, Vcs};

/// Why a board verb stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardError {
    /// The instance already holds as many claims as its batch allows.
    #[error("{instance} already holds {} of {batch} claims: {}", held.len(), list(held))]
    BatchFull {
        /// Who asked.
        instance: Instance,
        /// The batch size the manifest gives its role.
        batch: u32,
        /// The tasks it holds.
        held: Vec<TicketId>,
    },
    /// `board done` found the task outside `land` with no merged pull request to show for it.
    #[error("{task} is in {phase}, not land")]
    NotInLand {
        /// The task.
        task: TicketId,
        /// The phase it is in.
        phase: Phase,
    },
    /// The task's phase could not be read, so nothing was changed.
    #[error(transparent)]
    Unreadable(#[from] PhaseUnreadable),
    /// The board refused or failed.
    #[error(transparent)]
    Port(#[from] PortError),
}

fn list(numbers: &[TicketId]) -> String {
    let numbers: Vec<String> = numbers.iter().map(ToString::to_string).collect();
    numbers.join(", ")
}

/// `zaf board ready`: the tasks open for a claim, in issue order.
pub fn ready(board: &dyn Board) -> Result<Vec<Ticket>, BoardError> {
    Ok(board.ready()?)
}

/// The command `zaf board claim` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimCommand {
    /// The task.
    pub task: TicketId,
    /// Who claims it.
    pub instance: Instance,
    /// How many claims the instance may hold at once: its role's batch size.
    pub batch: u32,
}

/// `zaf board claim`: claims a ready task, up to the instance's batch size.
///
/// Claiming one's own claim again succeeds with [`ClaimOutcome::AlreadyHeld`]
/// and never counts against the batch; a task held by another instance is a conflict.
pub fn claim(board: &dyn Board, command: &ClaimCommand) -> Result<ClaimOutcome, BoardError> {
    let held: Vec<TicketId> = board
        .claims()?
        .into_iter()
        .filter(|claim| claim.instance == command.instance)
        .map(|claim| claim.task)
        .collect();
    let already_held = held.contains(&command.task);
    let batch_full = usize::try_from(command.batch).is_ok_and(|batch| held.len() >= batch);
    if !already_held && batch_full {
        let batch_full_error = BoardError::BatchFull {
            instance: command.instance.clone(),
            batch: command.batch,
            held,
        };
        return Err(batch_full_error);
    }
    Ok(board.claim(command.task, &command.instance)?)
}

/// The command `zaf board report` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportCommand {
    /// The task or epic reported on.
    pub task: TicketId,
    /// Who reports.
    pub instance: Instance,
    /// The status.
    pub body: String,
}

/// What `zaf board report` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportOutcome {
    /// Whether the report also touched the instance's claim on the task.
    pub heartbeat: bool,
}

/// `zaf board report`: writes the instance's status comment, replacing its last one,
/// and touches its claim when it holds the task, so the lease stays live.
///
/// Reporting on an issue the instance holds no claim on (its epic) writes the report only.
pub fn report(board: &dyn Board, command: &ReportCommand) -> Result<ReportOutcome, BoardError> {
    board.report(command.task, &command.instance, &command.body)?;
    let heartbeat = match board.heartbeat(command.task, &command.instance) {
        Ok(()) => true,
        Err(PortError::Conflict(_)) => false,
        Err(error) => return Err(error.into()),
    };
    Ok(ReportOutcome { heartbeat })
}

/// `zaf board question`: opens a question ticket for the Engineer and labels
/// the issue it is raised on `question`, so that issue waits.
///
/// Asking the same text on the same issue again reuses the question still open
/// instead of opening a second one, and returns its number.
pub fn question(board: &dyn Board, command: &NewQuestion) -> Result<TicketId, BoardError> {
    let open = board
        .questions()?
        .into_iter()
        .find(|ticket| command.is_asked_by(ticket));
    let number = match open {
        Some(ticket) => ticket.id,
        None => board.open_question(command)?,
    };
    board.set_state(command.on, TicketState::Question)?;
    Ok(number)
}

/// The command `zaf board ruling` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulingCommand {
    /// The question ruled on.
    pub question: TicketId,
    /// The Engineer's ruling.
    pub ruling: String,
}

/// `zaf board ruling`: records the ruling, closes the question and returns the task
/// it was raised on to `ready`; that task is the one returned, if there was one.
/// The same ruling again succeeds quietly; a different one is a conflict.
pub fn ruling(board: &dyn Board, command: &RulingCommand) -> Result<Option<TicketId>, BoardError> {
    Ok(board.record_ruling(command.question, &command.ruling)?)
}

/// What `zaf board done` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoneOutcome {
    /// The task is now labelled `done`.
    Marked,
    /// The task was `done` already; nothing changed.
    AlreadyDone,
}

/// `zaf board done`: marks a task `done` once its work has landed.
///
/// In `land` the label is set, which takes `ready` and `claimed` off. Outside `land` it is set
/// only when the pull request recorded for the task has merged: `slice-merge` merges the base in
/// first, which moves the head and so takes the task out of `land` before it merges. A task
/// already `done` is left alone. Every other case, an open pull request and a task with no
/// recorded one included, is [`BoardError::NotInLand`], and a read that fails is an error:
/// either way nothing is changed.
pub fn done(
    board: &dyn Board,
    forge: &dyn Forge,
    vcs: &dyn Vcs,
    records: &SliceRecords,
    task: TicketId,
) -> Result<DoneOutcome, BoardError> {
    let facts = gather_phase_facts(board, forge, vcs, records, task)?;
    let phase = derive_phase(&facts);
    match phase {
        Phase::Done => return Ok(DoneOutcome::AlreadyDone),
        Phase::Land => {}
        _ => {
            let merged = match records.pr(task) {
                Some(pr) => forge.merged(pr)?,
                None => false,
            };
            if !merged {
                return Err(BoardError::NotInLand { task, phase });
            }
        }
    }
    board.set_state(task, TicketState::Done)?;
    Ok(DoneOutcome::Marked)
}

#[cfg(test)]
mod tests;
