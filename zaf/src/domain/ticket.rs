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
}

impl Ticket {
    /// The task its body holds; an error for a body that is not a task (an epic, a question).
    pub fn template(&self) -> Result<TaskTemplate, TaskTemplateError> {
        self.body.as_str().parse()
    }

    /// The ticket as it leaves zaf: its id (as `number`), epic, title, state and link.
    pub fn summary(&self) -> TicketSummary {
        TicketSummary {
            number: self.id,
            epic: self.epic,
            title: self.title.clone(),
            state: self.state,
            url: self.url.clone(),
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
}

/// A ticket as every verb's `--json` prints it (see [`Ticket::summary`]): the id and the epic
/// as bare numbers, the state as its snake_case word.
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

#[cfg(test)]
mod tests;
