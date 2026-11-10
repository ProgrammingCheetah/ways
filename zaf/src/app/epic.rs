//! `zaf epic`: the open epics and where each stands, and the Engineer's signature on one.
//!
//! Nothing here is stored. A stage is derived from the board on every read (ruling 48), and
//! signing writes one thing, the hash of the body as it reads now, so an edit voids it.
//!
//! Only the Engineer signs. Whether this process runs inside an instance is judged by process
//! ancestry, never by the environment: a launch this machine runs holds a process in its run
//! folder ([`super::spawn::runs`]), and a sign whose process, or any process above it, is one
//! of those holders is refused.

use std::path::Path;
use std::time::SystemTime;

use super::spawn::SpawnError;
use super::spawn::runs::{self, Run};
use crate::domain::{
    BodyHash, Epic, EpicStage, EpicTask, GroupName, Signature, Standing, TicketId, TicketState,
    TicketTitle,
};
use crate::ports::{Board, Holder, PortError};

/// Why an epic verb could not do its work.
#[derive(Debug, thiserror::Error)]
pub enum EpicError {
    /// The board failed.
    #[error(transparent)]
    Board(PortError),
    /// No open epic has the id.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// This process runs inside an instance, and only the Engineer signs.
    #[error("{instance} runs above this process; only the Engineer signs an epic")]
    InsideInstance {
        /// The instance found above.
        instance: String,
    },
    /// The run folders could not be read, so no one can tell who runs.
    #[error(transparent)]
    Runs(#[from] SpawnError),
}

/// An open epic as `zaf epic list` prints it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EpicLine {
    /// Its id.
    pub number: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Where it stands.
    pub stage: EpicStage,
    /// Its tasks done.
    pub done: usize,
    /// Its tasks that count.
    pub total: usize,
    /// Whether it was signed and edited since, so it must be signed again.
    pub signature_void: bool,
}

impl From<&Epic> for EpicLine {
    fn from(epic: &Epic) -> Self {
        let (done, total) = epic.tally();
        Self {
            number: epic.id,
            title: epic.title.clone(),
            stage: epic.stage(),
            done,
            total,
            signature_void: epic.signature_void(),
        }
    }
}

impl EpicLine {
    /// One line: `#12 challenged 3 of 7  <title>`, with `(edited since signed)` when void.
    pub fn render(&self) -> String {
        let void = if self.signature_void {
            " (edited since signed)"
        } else {
            ""
        };
        format!(
            "{} {}{void} {} of {}  {}",
            self.number, self.stage, self.done, self.total, self.title
        )
    }
}

/// One task of an epic as `zaf epic show` prints it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TaskLine {
    /// Its id.
    pub number: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Its state, if it has one.
    pub state: Option<TicketState>,
    /// Whether it is closed: done, or not planned.
    pub closed: Option<&'static str>,
    /// Its group, if it has one.
    pub group: Option<GroupName>,
}

impl From<&EpicTask> for TaskLine {
    fn from(task: &EpicTask) -> Self {
        Self {
            number: task.number,
            title: task.title.clone(),
            state: task.state,
            closed: match task.standing {
                Standing::Open | Standing::Missing => None,
                Standing::Done => Some("done"),
                Standing::ClosedNotPlanned => Some("not_planned"),
            },
            group: task.group.clone(),
        }
    }
}

impl TaskLine {
    /// One line: `#13 ready  <title>`, `#14 closed not planned  <title>`, with `[group]`.
    pub fn render(&self) -> String {
        let state = match (self.closed, self.state) {
            (Some("not_planned"), _) => "closed not planned".to_owned(),
            (Some(word), _) => word.to_owned(),
            (None, Some(state)) => state.to_string(),
            (None, None) => "no state".to_owned(),
        };
        let group = self
            .group
            .as_ref()
            .map(|group| format!(" [{group}]"))
            .unwrap_or_default();
        format!("{} {state}{group}  {}", self.number, self.title)
    }
}

/// An open epic as `zaf epic show` prints it: its line, then one line per task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EpicView {
    /// The epic's line.
    #[serde(flatten)]
    pub epic: EpicLine,
    /// Its tasks, closed ones too, in id order.
    pub tasks: Vec<TaskLine>,
}

impl EpicView {
    /// The epic's line, then each task indented under it.
    pub fn render(&self) -> String {
        let mut lines = vec![self.epic.render()];
        lines.extend(self.tasks.iter().map(|task| format!("  {}", task.render())));
        lines.join("\n")
    }
}

/// One open epic with its tasks, closed ones too; a ticket that is no open epic is refused.
pub fn show(board: &dyn Board, id: TicketId) -> Result<EpicView, EpicError> {
    let epic = match board.epic(id) {
        Ok(epic) => epic,
        Err(PortError::NotFound(_)) => return Err(EpicError::NotAnEpic(id)),
        Err(error) => return Err(EpicError::Board(error)),
    };
    let mut tasks: Vec<TaskLine> = epic.tasks.iter().map(TaskLine::from).collect();
    tasks.sort_by_key(|task| task.number);
    Ok(EpicView {
        epic: EpicLine::from(&epic),
        tasks,
    })
}

/// Every open epic and where it stands.
pub fn list(board: &dyn Board) -> Result<Vec<EpicLine>, EpicError> {
    let epics = board.epics().map_err(EpicError::Board)?;
    Ok(epics.iter().map(EpicLine::from).collect())
}

/// What signing did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutcome {
    /// The signature was recorded on this body.
    Signed(BodyHash),
    /// The epic's signature already holds this body; nothing was written.
    AlreadySigned(BodyHash),
}

/// The instance this process runs inside: a running launch whose holder is in `ancestors`
/// (this process and every process above it). None when no running launch is above.
pub fn instance_above(
    ancestors: &[Holder],
    runs: &[Run],
    alive: &dyn Fn(&Holder) -> bool,
) -> Option<String> {
    runs.iter()
        .filter(|run| run.running(alive))
        .find(|run| {
            run.holder
                .as_ref()
                .is_some_and(|holder| ancestors.contains(holder))
        })
        .map(|run| {
            run.record
                .as_ref()
                .map_or_else(|| run.slug.clone(), |record| record.instance.clone())
        })
}

/// Who may sign: nobody when an instance runs above this process.
pub struct SignGuard<'a> {
    /// The project's state folder, where the run folders are.
    pub state_dir: &'a Path,
    /// This process and every process above it.
    pub ancestors: &'a [Holder],
    /// Whether a recorded process still runs.
    pub alive: &'a dyn Fn(&Holder) -> bool,
}

/// Signs `epic` as its body reads now, at `at`, unless an instance runs above this process.
/// A signature that already holds the body is left as it is.
pub fn sign(
    board: &dyn Board,
    epic: TicketId,
    guard: &SignGuard,
    at: SystemTime,
) -> Result<SignOutcome, EpicError> {
    let runs = runs::read_all(guard.state_dir)?;
    if let Some(instance) = instance_above(guard.ancestors, &runs, guard.alive) {
        return Err(EpicError::InsideInstance { instance });
    }
    let read = match board.epic(epic) {
        Ok(read) => read,
        Err(PortError::NotFound(_)) => return Err(EpicError::NotAnEpic(epic)),
        Err(error) => return Err(EpicError::Board(error)),
    };
    let signature = Signature::of(&read.body, at);
    if read.signed() {
        return Ok(SignOutcome::AlreadySigned(signature.hash));
    }
    board.sign(epic, &signature).map_err(EpicError::Board)?;
    Ok(SignOutcome::Signed(signature.hash))
}

#[cfg(test)]
mod tests;
