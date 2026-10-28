//! `zaf board ready|claim|report|question|ruling`: reading and writing the board.

use crate::domain::{Instance, IssueNumber, TaskLabel};
use crate::ports::{Board, ClaimOutcome, NewQuestion, PortError, Ticket};

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
        held: Vec<IssueNumber>,
    },
    /// The board refused or failed.
    #[error(transparent)]
    Port(#[from] PortError),
}

fn list(numbers: &[IssueNumber]) -> String {
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
    pub task: IssueNumber,
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
    let held: Vec<IssueNumber> = board
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
    pub task: IssueNumber,
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
pub fn question(board: &dyn Board, command: &NewQuestion) -> Result<IssueNumber, BoardError> {
    let number = board.open_question(command)?;
    board.label(command.on, TaskLabel::Question)?;
    Ok(number)
}

/// The command `zaf board ruling` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulingCommand {
    /// The question ruled on.
    pub question: IssueNumber,
    /// The Engineer's ruling.
    pub ruling: String,
}

/// `zaf board ruling`: records the ruling and closes the question.
/// The same ruling again succeeds quietly; a different one is a conflict.
pub fn ruling(board: &dyn Board, command: &RulingCommand) -> Result<(), BoardError> {
    Ok(board.record_ruling(command.question, &command.ruling)?)
}

#[cfg(test)]
mod tests;
