//! The board: zaf's tickets over whichever backend holds them (GitHub issues today), with
//! claims, states, comments, questions and rulings.
//!
//! Every method speaks the domain's [`Ticket`]; how a backend saves one (labels, fields,
//! comments) is the backend's choice and never crosses this port.

use std::time::SystemTime;

use super::PortError;
use crate::domain::{
    Epic, Instance, NewTask, Signature, Standing, Ticket, TicketId, TicketState, TicketType,
};

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
    pub task: TicketId,
    /// Who holds it.
    pub instance: Instance,
    /// The last heartbeat; a claim silent for the lease window is reclaimed.
    pub heartbeat: SystemTime,
}

/// A question for the Engineer, raised on an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewQuestion {
    /// The issue it is raised on; that issue waits.
    pub on: TicketId,
    /// The question.
    pub text: String,
    /// Whether it is a one-way decision.
    pub door: bool,
}

/// The issue types a repository defines (see [`Board::issue_types`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueTypes {
    /// The repository, as the board names it.
    pub repo: String,
    /// Its issue types, by name.
    pub names: Vec<TicketType>,
}

impl IssueTypes {
    /// Whether the repository defines the type `kind`, by its exact name.
    pub fn defines(&self, kind: &TicketType) -> bool {
        self.names.contains(kind)
    }
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
    pub fn raised_on(body: &str) -> Option<TicketId> {
        let first = body.trim().lines().next()?;
        let rest = first.trim_end().strip_prefix("Raised on ")?;
        rest.strip_suffix('.')?.parse().ok()
    }

    /// The whole question a ticket carries: what follows the `Raised on` line of its body,
    /// or its title when the body holds nothing more (a question filed before bodies held it).
    pub fn text_of(ticket: &Ticket) -> &str {
        let body = ticket.body.as_str().trim();
        let rest = match body.split_once('\n') {
            Some((first, rest)) if Self::raised_on(first).is_some() => rest.trim(),
            _ => "",
        };
        if rest.is_empty() {
            ticket.title.as_str()
        } else {
            rest
        }
    }

    /// Whether `ticket` is this question already open: the same text raised on the same issue.
    /// Whether it was raised as a door does not matter; the question is the same.
    pub fn is_asked_by(&self, ticket: &Ticket) -> bool {
        Self::raised_on(ticket.body.as_str()) == Some(self.on)
            && Self::text_of(ticket).trim() == self.text.trim()
    }
}

/// The board the spawner and every instance read and write.
pub trait Board {
    /// Where one issue stands as a blocker (see [`Standing`]).
    ///
    /// Interface: `Board::standing(number) -> Result<Standing, PortError>`.
    ///
    /// An issue with no such number is [`Standing::Missing`], not an error. A closed issue
    /// is done unless it was closed as not planned or as a duplicate. A board that does not
    /// override this answers from [`Board::task`]: `done` when the ticket is labelled so,
    /// `missing` when there is no such ticket, else `open`; it cannot see a closed issue.
    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        match self.task(number) {
            Ok(ticket) if ticket.state == Some(TicketState::Done) => Ok(Standing::Done),
            Ok(_) => Ok(Standing::Open),
            Err(PortError::NotFound(_)) => Ok(Standing::Missing),
            Err(error) => Err(error),
        }
    }

    /// Every open epic, in id order, with its tasks (open and closed, in id order), its body,
    /// its latest signature and whether the challenger's report is on it: everything its
    /// [`stage`](Epic::stage) is derived from.
    ///
    /// A board that cannot list them answers [`PortError::Rejected`] unless it overrides this.
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        Err(PortError::Rejected(
            "this board cannot list its epics".to_owned(),
        ))
    }

    /// One open epic, as [`Board::epics`] lists it; [`PortError::NotFound`] when no open epic
    /// has that id.
    fn epic(&self, id: TicketId) -> Result<Epic, PortError> {
        self.epics()?
            .into_iter()
            .find(|epic| epic.id == id)
            .ok_or_else(|| PortError::NotFound(format!("{id} is not an open epic")))
    }

    /// Records the Engineer's signature on an epic. A later signature replaces an earlier one
    /// as the one [`Board::epics`] reads.
    ///
    /// A board that cannot hold one answers [`PortError::Rejected`] unless it overrides this.
    fn sign(&self, epic: TicketId, signature: &Signature) -> Result<(), PortError> {
        let _ = signature;
        Err(PortError::Rejected(format!(
            "this board cannot sign {epic}"
        )))
    }

    /// The issue types the board's repository defines, with the repository's name.
    ///
    /// A board that cannot list them answers [`PortError::Rejected`] unless it overrides this.
    fn issue_types(&self) -> Result<IssueTypes, PortError> {
        Err(PortError::Rejected(
            "this board cannot list its issue types".to_owned(),
        ))
    }

    /// Files a task under its epic, in its state, group and type, and returns its id.
    /// The issue opens with the task's type; a board that does not define it fails the call
    /// before anything is opened.
    ///
    /// A board that cannot file one answers [`PortError::Rejected`] unless it overrides this.
    fn file_task(&self, task: &NewTask) -> Result<TicketId, PortError> {
        Err(PortError::Rejected(format!(
            "this board cannot file a task under {}",
            task.epic
        )))
    }

    /// Tasks labelled `ready`, in issue order.
    fn ready(&self) -> Result<Vec<Ticket>, PortError>;

    /// One ticket by number.
    fn task(&self, number: TicketId) -> Result<Ticket, PortError>;

    /// Claims a ready task for an instance and labels it `claimed`.
    /// Claiming one's own claim again succeeds with [`ClaimOutcome::AlreadyHeld`];
    /// a claim held by another instance is a [`PortError::Conflict`].
    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError>;

    /// Touches the instance's claim so its lease stays live.
    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError>;

    /// Every live claim with its last heartbeat, for the spawner's lease check.
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError>;

    /// Drops `instance`'s claim on a task and comments `released from <instance> at <time>`.
    /// The task returns to `ready` unless it carries `question` or `done`, which it keeps.
    /// A task the instance does not hold is left as it is and the call succeeds.
    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError>;

    /// Sets a task's state label, replacing the one it had.
    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError>;

    /// Adds a comment to an issue.
    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError>;

    /// Writes the instance's status comment on a task; a second report replaces the first.
    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError>;

    /// The open question tickets, in issue order: issues labelled `question` that are not tasks.
    /// A task waiting on a question carries the label too, and is not one.
    fn questions(&self) -> Result<Vec<Ticket>, PortError>;

    /// Every question ticket raised on `task` (see [`NewQuestion::raised_on`]), open or ruled,
    /// oldest first, each with its ruling text when one was recorded.
    /// A ticket is raised on `task` when its body says `Raised on #<task>`.
    /// A ruled question stays listed: that is how a later reader learns it was settled.
    ///
    /// A board that cannot list them answers [`PortError::Rejected`] unless it overrides this.
    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        Err(PortError::Rejected(format!(
            "this board cannot list the questions raised on {task}"
        )))
    }

    /// Opens a question ticket for the Engineer and returns its number.
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError>;

    /// Records the Engineer's ruling on a question and closes it.
    /// The same ruling again succeeds quietly; a different one is a [`PortError::Conflict`].
    ///
    /// The task the question was raised on (see [`NewQuestion::raised_on`]) returns to
    /// `ready` when it carries `question`: the label is dropped and `ready` added.
    /// A task that is `done`, or no longer waits, is left alone.
    /// Returns the task that was returned to `ready`, if any.
    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError>;
}

#[cfg(test)]
mod tests;
