//! The ticket: zaf's own shape for a unit of work on the board, whichever backend holds it.
//!
//! A [`Ticket`] is the domain entity every verb reads; how a backend saves it (labels, fields,
//! comments) is the backend's choice and never shows here. A [`TicketSummary`] is the one
//! shape a ticket leaves zaf in: every `--json` prints it, whichever backend holds the ticket.

use super::{GroupName, TaskTemplate, TaskTemplateError, TicketId, TicketState};
use crate::newtype::text_newtype;

text_newtype! {
    /// A ticket's title.
    TicketTitle
}

text_newtype! {
    /// A ticket's body; a task's body is a [`crate::domain::TaskTemplate`].
    TicketBody
}

text_newtype! {
    /// Where a person opens a ticket; empty when the backend has no link for it.
    TicketUrl
}

text_newtype! {
    /// A ticket's type, the name of its forge issue type as the forge spells it (`Task`, `Bug`).
    TicketType
}

/// The type a task is filed as when its filer names none.
pub const DEFAULT_TASK_TYPE: &str = "Task";

impl TicketType {
    /// The type of a task filed with no `--type`: [`DEFAULT_TASK_TYPE`].
    pub fn task() -> Self {
        Self::from(DEFAULT_TASK_TYPE)
    }
}

/// A ticket as zaf presents it, whichever backend holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// zaf's id for it; the backend maps it to its storage (an issue's number on GitHub).
    pub id: TicketId,
    /// The epic it belongs to; none for a question or an epic itself.
    pub epic: Option<TicketId>,
    /// Its title.
    pub title: TicketTitle,
    /// Its body.
    pub body: TicketBody,
    /// Its state, if it has one.
    pub state: Option<TicketState>,
    /// Where a person opens it.
    pub url: TicketUrl,
    /// Its type, the forge's issue type (`Ticket.kind: Option<TicketType>`); none when the
    /// issue has no type.
    pub kind: Option<TicketType>,
}

impl Ticket {
    /// The task its body holds; an error for a body that is not a task (an epic, a question).
    pub fn template(&self) -> Result<TaskTemplate, TaskTemplateError> {
        self.body.as_str().parse()
    }

    /// The ticket as it leaves zaf: its id (as `number`), epic, title, state, link and type.
    pub fn summary(&self) -> TicketSummary {
        TicketSummary {
            number: self.id,
            epic: self.epic,
            title: self.title.clone(),
            state: self.state,
            url: self.url.clone(),
            kind: self.kind.clone(),
        }
    }
}

/// A task to file under an epic, as [`crate::ports::Board::file_task`] opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    /// The epic it belongs to.
    pub epic: TicketId,
    /// Its title.
    pub title: TicketTitle,
    /// Its body, in the task template.
    pub body: TicketBody,
    /// The state it opens in.
    pub state: TicketState,
    /// Its group, if it has one.
    pub group: Option<GroupName>,
    /// Its type, the forge issue type it opens with (`NewTask.kind: TicketType`); a backend
    /// opens it with this type or fails.
    pub kind: TicketType,
}

/// A ticket as every verb's `--json` prints it (see [`Ticket::summary`]): the id and the epic
/// as bare numbers, the state as its snake_case word, the type as its name or `null`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TicketSummary {
    /// zaf's id.
    pub number: TicketId,
    /// Its epic's id, if it has one.
    pub epic: Option<TicketId>,
    /// Its title.
    pub title: TicketTitle,
    /// Its state, if it has one.
    pub state: Option<TicketState>,
    /// Where a person opens it.
    pub url: TicketUrl,
    /// Its type's name, `null` when it has none; printed as `"type": "<name>"`.
    #[serde(rename = "type")]
    pub kind: Option<TicketType>,
}

/// Where an issue stands as a blocker: whether it can still close, and how.
///
/// Read from the issue itself, so a blocker counts as done the moment its issue closes, with
/// no label to add by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The issue is open and not labelled `done`: it may still close.
    Open,
    /// The issue is labelled `done`, or closed as completed.
    Done,
    /// The issue is closed as not planned or as a duplicate (a task folded into a unit is
    /// closed so): its work will never be done here.
    ClosedNotPlanned,
    /// There is no such issue, or the number is a pull request.
    Missing,
}

impl Standing {
    /// Where an issue that exists stands: done when it is in the `done` state; else open while
    /// it is open; else closed as not planned when it was `dropped` (closed as not planned or
    /// as a duplicate); else closed as completed, so done.
    pub fn of_issue(open: bool, dropped: bool, done: bool) -> Self {
        if done {
            Self::Done
        } else if open {
            Self::Open
        } else if dropped {
            Self::ClosedNotPlanned
        } else {
            Self::Done
        }
    }
}

#[cfg(test)]
mod tests;
