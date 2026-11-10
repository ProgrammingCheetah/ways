//! `zaf board ready|claim|report|handoff|question|ruling|done|file`: reading and writing the
//! board, and the hand-off on a task's pull request.

use crate::app::actions::{Phase, PhaseUnreadable, SliceRecords, derive_phase, gather_phase_facts};
use crate::domain::{
    Instance, NewTask, TaskTemplateError, Ticket, TicketId, TicketState, TicketType,
};
use crate::ports::{Board, ClaimOutcome, Forge, NewQuestion, PortError, PrNumber, Vcs};

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
    /// `board handoff` found no pull request recorded for the task; nothing was written.
    #[error("{0} has no recorded pull request")]
    NoPullRequest(TicketId),
    /// `board file` was given a body outside the task template; nothing was filed.
    #[error("the body is not a task: {0}")]
    NotATask(#[from] TaskTemplateError),
    /// `board file` was given a state a task does not open in.
    #[error("a task opens ready or blocked, not {0}")]
    NotAnOpeningState(TicketState),
    /// `board file` was given a type the repository does not define; nothing was filed. It
    /// prints as `zaf board file: <repo> has no issue type <name>; it has <types>`.
    #[error("{repo} has no issue type {name}; it has {}", type_list(types))]
    NoSuchType {
        /// The repository, as the board names it.
        repo: String,
        /// The type asked for.
        name: TicketType,
        /// The types the repository does define.
        types: Vec<TicketType>,
    },
    /// The board refused or failed.
    #[error(transparent)]
    Port(#[from] PortError),
}

fn type_list(types: &[TicketType]) -> String {
    if types.is_empty() {
        return "none".to_owned();
    }
    let names: Vec<&str> = types.iter().map(TicketType::as_str).collect();
    names.join(", ")
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

/// `zaf board handoff`: writes the one hand-off comment on the task's recorded pull request,
/// replacing the one it has, and returns that pull request.
///
/// A task with no recorded pull request is [`BoardError::NoPullRequest`] and nothing is written.
pub fn handoff(
    forge: &dyn Forge,
    records: &SliceRecords,
    task: TicketId,
    text: &str,
) -> Result<PrNumber, BoardError> {
    let pr = records.pr(task).ok_or(BoardError::NoPullRequest(task))?;
    forge.set_handoff(pr, text)?;
    Ok(pr)
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

/// What `zaf board file` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    /// The task was opened under its epic.
    Filed(TicketId),
    /// The epic already has an open or done task with that title; nothing was opened.
    AlreadyFiled(TicketId),
}

impl FileOutcome {
    /// The task, filed now or before.
    pub fn task(self) -> TicketId {
        match self {
            Self::Filed(task) | Self::AlreadyFiled(task) => task,
        }
    }
}

/// `zaf board file`: opens a task under an open epic, in the task template, ready or blocked.
///
/// A body outside the template, or a type the repository does not define
/// ([`BoardError::NoSuchType`], naming the types it does), is refused before anything is
/// written. Filing a title the epic already counts (open or done) returns that task, so a
/// planning instance run again files nothing twice.
pub fn file(board: &dyn Board, task: &NewTask) -> Result<FileOutcome, BoardError> {
    if !matches!(task.state, TicketState::Ready | TicketState::Blocked) {
        return Err(BoardError::NotAnOpeningState(task.state));
    }
    task.body.as_str().parse::<crate::domain::TaskTemplate>()?;
    let types = board.issue_types()?;
    if !types.defines(&task.kind) {
        return Err(BoardError::NoSuchType {
            repo: types.repo,
            name: task.kind.clone(),
            types: types.names,
        });
    }
    let epic = board.epic(task.epic)?;
    if let Some(filed) = epic
        .tasks
        .iter()
        .find(|filed| filed.counts() && filed.title == task.title)
    {
        return Ok(FileOutcome::AlreadyFiled(filed.number));
    }
    Ok(FileOutcome::Filed(board.file_task(task)?))
}
