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

/// The most characters a question's title holds; GitHub refuses a title over 256.
pub const QUESTION_TITLE_LIMIT: usize = 200;

impl NewQuestion {
    /// The title of the ticket that carries this question: the text itself when it fits in
    /// [`QUESTION_TITLE_LIMIT`] characters, else its first words, cut at a word.
    /// A text with no word break inside the limit is cut at the limit.
    pub fn title(&self) -> String {
        let Some((cut, _)) = self.text.char_indices().nth(QUESTION_TITLE_LIMIT) else {
            return self.text.clone();
        };
        let head = &self.text[..cut];
        if self.text[cut..].starts_with(char::is_whitespace) {
            return head.trim_end().to_owned();
        }
        match head.rfind(char::is_whitespace) {
            Some(space) if !head[..space].trim_end().is_empty() => {
                head[..space].trim_end().to_owned()
            }
            _ => head.to_owned(),
        }
    }

    /// The body of the ticket that carries this question: where it was raised, then the
    /// whole text, so a title cut short loses nothing.
    pub fn body(&self) -> String {
        let raised = format!("Raised on {}.", self.on);
        if self.text.trim().is_empty() {
            return raised;
        }
        format!("{raised}\n\n{}", self.text.trim())
    }

    /// The issue a question ticket was raised on, read back from its [`body`](Self::body):
    /// the first line says where, whatever follows.
    pub fn raised_on(body: &str) -> Option<IssueNumber> {
        let first = body.trim().lines().next()?;
        let rest = first.trim_end().strip_prefix("Raised on ")?;
        rest.strip_suffix('.')?.parse().ok()
    }

    /// The whole question a ticket carries: what follows the `Raised on` line of its body,
    /// or its title when the body holds nothing more (a question filed before bodies held it).
    pub fn text_of(ticket: &Ticket) -> &str {
        let body = ticket.body.trim();
        let rest = match body.split_once('\n') {
            Some((first, rest)) if Self::raised_on(first).is_some() => rest.trim(),
            _ => "",
        };
        if rest.is_empty() { &ticket.title } else { rest }
    }

    /// Whether `ticket` is this question already open: the same text raised on the same issue.
    /// Whether it was raised as a door does not matter; the question is the same.
    pub fn is_asked_by(&self, ticket: &Ticket) -> bool {
        Self::raised_on(&ticket.body) == Some(self.on)
            && Self::text_of(ticket).trim() == self.text.trim()
    }
}

/// A sub-issue of an epic as the hub reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTask {
    /// Its issue.
    pub number: IssueNumber,
    /// Its state label, if it has one.
    pub label: Option<TaskLabel>,
    /// Whether the issue is closed.
    pub closed: bool,
    /// The name in its `group:<name>` label, if it has one.
    pub group: Option<String>,
}

/// An open epic and its sub-issues, closed ones included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTree {
    /// Its issue.
    pub number: IssueNumber,
    /// Its title.
    pub title: String,
    /// Its tasks, in issue order.
    pub tasks: Vec<EpicTask>,
}

/// Where an issue stands as a blocker: whether it can still close, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    /// The issue is open and not labelled `done`: it may still close.
    Open,
    /// The issue is labelled `done`.
    Done,
    /// The issue is closed without `done`: not planned, or folded into a unit.
    ClosedNotDone,
    /// There is no such issue, or the number is a pull request.
    Missing,
}

/// The board the spawner and every instance read and write.
pub trait Board {
    /// Where one issue stands as a blocker (see [`IssueState`]).
    ///
    /// An issue with no such number is [`IssueState::Missing`], not an error. A board that
    /// does not override this answers from [`Board::task`]: `done` when the ticket is labelled
    /// so, `missing` when there is no such ticket, else `open`; it cannot see a closed issue.
    fn issue_state(&self, number: IssueNumber) -> Result<IssueState, PortError> {
        match self.task(number) {
            Ok(ticket) if ticket.label == Some(TaskLabel::Done) => Ok(IssueState::Done),
            Ok(_) => Ok(IssueState::Open),
            Err(PortError::NotFound(_)) => Ok(IssueState::Missing),
            Err(error) => Err(error),
        }
    }

    /// Every open epic with its sub-issues, open and closed, in issue order.
    ///
    /// A board that cannot list them answers [`PortError::Rejected`] unless it overrides this.
    fn epic_trees(&self) -> Result<Vec<EpicTree>, PortError> {
        Err(PortError::Rejected(
            "this board cannot list its epics".to_owned(),
        ))
    }

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

    /// Every question ticket raised on `task` (see [`NewQuestion::raised_on`]), open or ruled,
    /// oldest first, each with its ruling text when one was recorded.
    /// A ticket is raised on `task` when its body says `Raised on #<task>`.
    /// A ruled question stays listed: that is how a later reader learns it was settled.
    ///
    /// A board that cannot list them answers [`PortError::Rejected`] unless it overrides this.
    fn questions_on(&self, task: IssueNumber) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        Err(PortError::Rejected(format!(
            "this board cannot list the questions raised on {task}"
        )))
    }

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
