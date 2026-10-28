//! The in-memory board.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use crate::domain::{Instance, IssueNumber, TaskLabel};
use crate::ports::{Board, ClaimOutcome, ClaimRecord, NewQuestion, PortError, Ticket};

/// One issue and everything written on it.
#[derive(Debug, Clone)]
struct Issue {
    ticket: Ticket,
    comments: Vec<String>,
    reports: Vec<(Instance, String)>,
    claim: Option<ClaimRecord>,
    door: bool,
    question: bool,
    ruling: Option<String>,
    closed: bool,
}

#[derive(Debug, Default)]
struct State {
    issues: BTreeMap<IssueNumber, Issue>,
}

/// A board held in memory. Seed it with [`FakeBoard::add_ticket`].
#[derive(Debug, Default)]
pub struct FakeBoard {
    state: Mutex<State>,
}

impl FakeBoard {
    /// Puts a ticket on the board, replacing one with the same number.
    pub fn add_ticket(&self, ticket: Ticket) {
        let issue = Issue {
            ticket: ticket.clone(),
            comments: Vec::new(),
            reports: Vec::new(),
            claim: None,
            door: false,
            question: false,
            ruling: None,
            closed: false,
        };
        self.state().issues.insert(ticket.number, issue);
    }

    /// Puts an open question ticket on the board, labelled `question`, as `open_question` would.
    pub fn add_question(&self, ticket: Ticket) {
        let number = ticket.number;
        self.add_ticket(Ticket {
            label: Some(TaskLabel::Question),
            ..ticket
        });
        if let Some(issue) = self.state().issues.get_mut(&number) {
            issue.question = true;
        }
    }

    /// The comments on an issue, oldest first.
    pub fn comments(&self, issue: IssueNumber) -> Vec<String> {
        self.state()
            .issues
            .get(&issue)
            .map(|issue| issue.comments.clone())
            .unwrap_or_default()
    }

    /// The status comments on a task, one per instance.
    pub fn reports(&self, task: IssueNumber) -> Vec<(Instance, String)> {
        self.state()
            .issues
            .get(&task)
            .map(|issue| issue.reports.clone())
            .unwrap_or_default()
    }

    /// Whether a question was raised as a door, and its ruling if there is one.
    pub fn question(&self, question: IssueNumber) -> Option<(bool, Option<String>)> {
        self.state()
            .issues
            .get(&question)
            .map(|issue| (issue.door, issue.ruling.clone()))
    }

    /// Whether an issue is closed, as recording a ruling closes a question.
    pub fn is_closed(&self, issue: IssueNumber) -> bool {
        self.state()
            .issues
            .get(&issue)
            .is_some_and(|issue| issue.closed)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_found(issue: IssueNumber) -> PortError {
    PortError::NotFound(format!("issue {issue}"))
}

impl State {
    fn issue_mut(&mut self, number: IssueNumber) -> Result<&mut Issue, PortError> {
        self.issues
            .get_mut(&number)
            .ok_or_else(|| not_found(number))
    }
}

impl Board for FakeBoard {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        let ready = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.ticket.label == Some(TaskLabel::Ready))
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(ready)
    }

    fn task(&self, number: IssueNumber) -> Result<Ticket, PortError> {
        self.state()
            .issues
            .get(&number)
            .map(|issue| issue.ticket.clone())
            .ok_or_else(|| not_found(number))
    }

    fn claim(&self, task: IssueNumber, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        if let Some(claim) = &issue.claim {
            if claim.instance == *instance {
                return Ok(ClaimOutcome::AlreadyHeld);
            }
            return Err(PortError::Conflict(format!(
                "{task} is held by {}",
                claim.instance
            )));
        }
        if issue.ticket.label != Some(TaskLabel::Ready) {
            return Err(PortError::Rejected(format!("{task} is not ready")));
        }
        let claim = ClaimRecord {
            task,
            instance: instance.clone(),
            heartbeat: SystemTime::now(),
        };
        issue.claim = Some(claim);
        issue.ticket.label = Some(TaskLabel::Claimed);
        Ok(ClaimOutcome::Claimed)
    }

    fn heartbeat(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        let claim = issue
            .claim
            .as_mut()
            .filter(|claim| claim.instance == *instance)
            .ok_or_else(|| PortError::Conflict(format!("{instance} holds no claim on {task}")))?;
        claim.heartbeat = SystemTime::now();
        Ok(())
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        let claims = self
            .state()
            .issues
            .values()
            .filter_map(|issue| issue.claim.clone())
            .collect();
        Ok(claims)
    }

    fn release(&self, task: IssueNumber) -> Result<(), PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        if issue.claim.take().is_some() {
            issue.ticket.label = Some(TaskLabel::Ready);
        }
        Ok(())
    }

    fn label(&self, task: IssueNumber, label: TaskLabel) -> Result<(), PortError> {
        let mut state = self.state();
        state.issue_mut(task)?.ticket.label = Some(label);
        Ok(())
    }

    fn comment(&self, issue: IssueNumber, body: &str) -> Result<(), PortError> {
        let mut state = self.state();
        state.issue_mut(issue)?.comments.push(body.to_owned());
        Ok(())
    }

    fn report(&self, task: IssueNumber, instance: &Instance, body: &str) -> Result<(), PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        let existing = issue
            .reports
            .iter_mut()
            .find(|(author, _)| author == instance);
        match existing {
            Some((_, report)) => *report = body.to_owned(),
            None => issue.reports.push((instance.clone(), body.to_owned())),
        }
        Ok(())
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        let questions = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.question && !issue.closed)
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(questions)
    }

    fn open_question(&self, question: &NewQuestion) -> Result<IssueNumber, PortError> {
        let mut state = self.state();
        state.issue_mut(question.on)?;
        let highest = state
            .issues
            .keys()
            .next_back()
            .map_or(0, |number| number.get());
        let number = IssueNumber::from(highest + 1);
        let ticket = Ticket {
            number,
            epic: None,
            title: question.text.clone(),
            body: question.body(),
            label: Some(TaskLabel::Question),
        };
        let issue = Issue {
            ticket,
            comments: Vec::new(),
            reports: Vec::new(),
            claim: None,
            door: question.door,
            question: true,
            ruling: None,
            closed: false,
        };
        state.issues.insert(number, issue);
        Ok(number)
    }

    fn record_ruling(&self, question: IssueNumber, ruling: &str) -> Result<(), PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(question)?;
        match &issue.ruling {
            Some(existing) if existing == ruling => Ok(()),
            Some(_) => Err(PortError::Conflict(format!(
                "{question} already has a different ruling"
            ))),
            None => {
                issue.ruling = Some(ruling.to_owned());
                issue.closed = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
