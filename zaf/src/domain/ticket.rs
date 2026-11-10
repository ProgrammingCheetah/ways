//! The ticket: zaf's own shape for a unit of work on the board, whichever backend holds it.
//!
//! A [`Ticket`] is the domain entity every verb reads; how a backend saves it (labels, fields,
//! comments) is the backend's choice and never shows here. A [`TicketSummary`] is the one
//! shape a ticket leaves zaf in: every `--json` prints it, whichever backend holds the ticket.

use super::{TaskTemplate, TaskTemplateError, TicketId, TicketState};
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

#[cfg(test)]
mod tests;
