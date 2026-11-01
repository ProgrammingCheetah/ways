//! The board: tickets, claims under a lease, labels, comments, questions and rulings.

use std::time::SystemTime;

use super::PortError;
use crate::domain::{Instance, IssueNumber, TaskLabel};

/// A ticket as the board holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// Its issue.
    pub number: IssueNumber,
    /// The epic it belongs to; none for a question or an epic itself.
    pub epic: Option<IssueNumber>,
    /// Its title.
    pub title: String,
    /// Its body; a task's body is a [`crate::domain::TaskTemplate`].
    pub body: String,
    /// Its state label, if it has one.
    pub label: Option<TaskLabel>,
}

/// What a claim did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The task was ready and is now held.
    Claimed,
    /// The same instance already held it; nothing changed.
    AlreadyHeld,
}

/// A live claim and the last time its holder touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRecord {
    /// The claimed task.
    pub task: IssueNumber,
    /// Who holds it.
    pub instance: Instance,
    /// The last heartbeat; a claim silent for the lease window is reclaimed.
    pub heartbeat: SystemTime,
}

/// A question for the Engineer, raised on an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewQuestion {
    /// The issue it is raised on; that issue waits.
    pub on: IssueNumber,
    /// The question.
    pub text: String,
    /// Whether it is a one-way decision.
    pub door: bool,
}

impl NewQuestion {
    /// The body of the ticket that carries this question: where it was raised.
    pub fn body(&self) -> String {
        format!("Raised on {}.", self.on)
    }

    /// The issue a question ticket was raised on, read back from its [`body`](Self::body).
    pub fn raised_on(body: &str) -> Option<IssueNumber> {
        let rest = body.trim().strip_prefix("Raised on ")?;
        rest.strip_suffix('.')?.parse().ok()
    }

    /// Whether `ticket` is this question already open: the same text raised on the same issue.
    /// Whether it was raised as a door does not matter; the question is the same.
    pub fn is_asked_by(&self, ticket: &Ticket) -> bool {
        ticket.title == self.text && ticket.body == self.body()
    }
}

/// The board the spawner and every instance read and write.
pub trait Board {
    /// Tasks labelled `ready`, in issue order.
    fn ready(&self) -> Result<Vec<Ticket>, PortError>;

    /// One ticket by number.
    fn task(&self, number: IssueNumber) -> Result<Ticket, PortError>;

    /// Claims a ready task for an instance and labels it `claimed`.
    /// Claiming one's own claim again succeeds with [`ClaimOutcome::AlreadyHeld`];
    /// a claim held by another instance is a [`PortError::Conflict`].
    fn claim(&self, task: IssueNumber, instance: &Instance) -> Result<ClaimOutcome, PortError>;

    /// Touches the instance's claim so its lease stays live.
    fn heartbeat(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError>;

    /// Every live claim with its last heartbeat, for the spawner's lease check.
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError>;

    /// Drops `instance`'s claim on a task and comments `released from <instance> at <time>`.
    /// The task returns to `ready` unless it carries `question` or `done`, which it keeps.
    /// A task the instance does not hold is left as it is and the call succeeds.
    fn release(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError>;

    /// Sets a task's state label, replacing the one it had.
    fn label(&self, task: IssueNumber, label: TaskLabel) -> Result<(), PortError>;

    /// Adds a comment to an issue.
    fn comment(&self, issue: IssueNumber, body: &str) -> Result<(), PortError>;

    /// Writes the instance's status comment on a task; a second report replaces the first.
    fn report(&self, task: IssueNumber, instance: &Instance, body: &str) -> Result<(), PortError>;

    /// The open question tickets, in issue order: issues labelled `question` that are not tasks.
    /// A task waiting on a question carries the label too, and is not one.
    fn questions(&self) -> Result<Vec<Ticket>, PortError>;

    /// Opens a question ticket for the Engineer and returns its number.
    fn open_question(&self, question: &NewQuestion) -> Result<IssueNumber, PortError>;

    /// Records the Engineer's ruling on a question and closes it.
    /// The same ruling again succeeds quietly; a different one is a [`PortError::Conflict`].
    ///
    /// The task the question was raised on (see [`NewQuestion::raised_on`]) returns to
    /// `ready` when it carries `question`: the label is dropped and `ready` added.
    /// A task that is `done`, or no longer waits, is left alone.
    /// Returns the task that was returned to `ready`, if any.
    fn record_ruling(
        &self,
        question: IssueNumber,
        ruling: &str,
    ) -> Result<Option<IssueNumber>, PortError>;
}

#[cfg(test)]
mod tests;
