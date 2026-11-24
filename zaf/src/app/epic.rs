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

use super::release::{self, DueRelease};
use super::spawn::SpawnError;
use super::spawn::runs::{self, Run};
use crate::domain::screen::{Line, Meaning, clean};
use crate::domain::{
    BodyHash, Epic, EpicStage, EpicTask, GroupName, NewEpic, Signature, Standing, TicketBody,
    TicketId, TicketState, TicketTitle, TicketType, all_signed, of_version, with_release,
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
    /// The epic's title is empty.
    #[error("an epic needs a title")]
    EmptyTitle,
    /// The epic's body is empty.
    #[error("an epic needs a body")]
    EmptyBody,
    /// The version is empty or has a space or a line break in it, so no first line could name it.
    #[error("`{0}` is not a version a Release line can hold")]
    BadVersion(String),
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

impl EpicLine {
    /// [`Self::render`] as a screen line: the id a noun, the stage word in its meaning's colour
    /// (`unsigned` waits on the Engineer, `done` is done), `(edited since signed)` a failure, the
    /// tally dim and the title, board text, cleaned of control characters. Its pieces joined are
    /// [`Self::render`] for any title with none.
    pub fn line(&self) -> Line {
        let mut line: Line = vec![
            (Meaning::Noun, self.number.to_string().into()),
            (Meaning::Dim, " ".into()),
            (stage_meaning(self.stage), self.stage.to_string().into()),
        ];
        if self.signature_void {
            line.push((Meaning::Dim, " ".into()));
            line.push((Meaning::Failed, "(edited since signed)".into()));
        }
        line.push((
            Meaning::Dim,
            format!(" {} of {}  ", self.done, self.total).into(),
        ));
        line.push((Meaning::Bold, clean(self.title.as_ref()).into()));
        line
    }
}

/// What a stage word means: an epic nobody signed waits on the Engineer, a finished one is done,
/// and one in between is dim, as the chair shows it.
fn stage_meaning(stage: EpicStage) -> Meaning {
    match stage {
        EpicStage::Unsigned => Meaning::Waiting,
        EpicStage::Done => Meaning::Done,
        EpicStage::Signed | EpicStage::Planned | EpicStage::Challenged => Meaning::Dim,
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
    /// Its type's name, `null` when it has none.
    #[serde(rename = "type")]
    pub kind: Option<TicketType>,
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
            kind: task.kind.clone(),
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

impl TaskLine {
    /// [`Self::render`] as a screen line, indented by `indent`: the id a noun, the state and
    /// group dim, the title cleaned of control characters.
    pub fn line(&self, indent: &str) -> Line {
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
        vec![
            (Meaning::Dim, indent.to_owned().into()),
            (Meaning::Noun, self.number.to_string().into()),
            (Meaning::Dim, format!(" {state}{group}  ").into()),
            (Meaning::Bold, clean(self.title.as_ref()).into()),
        ]
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
    /// Its body as it reads now, which `--more` prints under the lines; never in the JSON.
    #[serde(skip)]
    pub body: TicketBody,
}

impl EpicView {
    /// The epic's line, then each task indented under it.
    pub fn render(&self) -> String {
        let mut lines = vec![self.epic.render()];
        lines.extend(self.tasks.iter().map(|task| format!("  {}", task.render())));
        lines.join("\n")
    }

    /// [`Self::render`] as screen lines: the epic's, then each task's indented under it.
    pub fn lines(&self) -> Vec<Line> {
        let mut lines = vec![self.epic.line()];
        lines.extend(self.tasks.iter().map(|task| task.line("  ")));
        lines
    }

    /// [`Self::render`], then a blank line and the epic's body: what `zaf epic show --more` prints,
    /// so a reader needs no tool of the board's own to read the epic.
    pub fn render_more(&self) -> String {
        format!("{}\n\n{}", self.render(), self.body.as_str().trim_end())
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
        body: epic.body,
    })
}

/// Every open epic and where it stands.
pub fn list(board: &dyn Board) -> Result<Vec<EpicLine>, EpicError> {
    let epics = board.epics().map_err(EpicError::Board)?;
    Ok(epics.iter().map(EpicLine::from).collect())
}

/// `zaf epic list`: the open epics, and after them the releases whose objectives or design are
/// due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicList {
    /// Every open epic and where it stands.
    pub epics: Vec<EpicLine>,
    /// The releases due, oldest first; none when nothing is due.
    pub due: Vec<DueRelease>,
}

impl EpicList {
    /// The lines printed: each epic's line, then each due release's `release <v>: <phase> due`
    /// and the way on. With no release due, the epic lines alone.
    pub fn lines(&self) -> Vec<Line> {
        let epics = self.epics.iter().map(EpicLine::line);
        let due = self
            .due
            .iter()
            .flat_map(|due| release::due_lines(&due.version, due.phase.name()));
        epics.chain(due).collect()
    }
}

/// `list_with_due(board, due)`: every open epic, and the releases due as `due` reads them from
/// those epics (the same read `zaf status` makes).
pub fn list_with_due(
    board: &dyn Board,
    due: impl FnOnce(&[Epic]) -> Vec<DueRelease>,
) -> Result<EpicList, EpicError> {
    let epics = board.epics().map_err(EpicError::Board)?;
    let due = due(&epics);
    Ok(EpicList {
        epics: epics.iter().map(EpicLine::from).collect(),
        due,
    })
}

/// What `zaf epic file` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    /// The epic was opened.
    Filed(TicketId),
    /// An open epic of the version has that title already; nothing was opened.
    AlreadyFiled(TicketId),
}

impl FileOutcome {
    /// The epic, filed now or before.
    pub fn epic(self) -> TicketId {
        match self {
            Self::Filed(epic) | Self::AlreadyFiled(epic) => epic,
        }
    }
}

/// Files an epic of `version`: its body is `Release: <version>` as the first line, then `body`.
///
/// An empty title or body is refused before the board is read. An open epic of the version with
/// the same title is handed back and nothing is opened, so the verb run again files nothing twice.
pub fn file(
    board: &dyn Board,
    version: &str,
    title: &str,
    body: &str,
) -> Result<FileOutcome, EpicError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(EpicError::EmptyTitle);
    }
    if body.trim().is_empty() {
        return Err(EpicError::EmptyBody);
    }
    if version.is_empty() || version.contains(char::is_whitespace) {
        return Err(EpicError::BadVersion(version.to_owned()));
    }
    let title = TicketTitle::from(title);
    let epics = match board.epics() {
        Ok(epics) => epics,
        // A board that cannot list epics cannot be told apart from one that holds none: the
        // filing then answers with its own sentence.
        Err(PortError::Rejected(_)) => Vec::new(),
        Err(error) => return Err(EpicError::Board(error)),
    };
    if let Some(filed) = of_version(&epics, version).find(|epic| epic.title == title) {
        return Ok(FileOutcome::AlreadyFiled(filed.id));
    }
    let epic = NewEpic {
        title,
        body: with_release(version, body.trim_end()).into(),
    };
    board
        .file_epic(&epic)
        .map(FileOutcome::Filed)
        .map_err(EpicError::Board)
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

/// The version whose build a signature just written makes ready to start: the version `epic`
/// is of, when every open epic of it is signed with `signature` counted for `epic` (a board that
/// lists its comments a moment late does not hide it). `None` for an epic of no version, or while
/// another epic of its version is unsigned.
///
/// Interface: consumes [`all_signed`] (I8); used by `zaf epic sign` (M19).
pub fn build_ready(
    board: &dyn Board,
    epic: TicketId,
    signature: Signature,
) -> Result<Option<String>, EpicError> {
    let mut epics = board.epics().map_err(EpicError::Board)?;
    let Some(signed) = epics.iter_mut().find(|open| open.id == epic) else {
        return Ok(None);
    };
    signed.signature = Some(signature);
    let Some(version) = signed.release().map(str::to_owned) else {
        return Ok(None);
    };
    Ok(all_signed(&epics, &version).then_some(version))
}

pub mod check;
pub mod wait;

#[cfg(test)]
mod tests;
