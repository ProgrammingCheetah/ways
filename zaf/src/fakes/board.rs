//! The in-memory board.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use crate::domain::{
    CHALLENGER_ROLE, Epic, EpicTask, GroupName, Instance, NewTask, Signature, Standing, Ticket,
    TicketId, TicketState, TicketType,
};
use crate::ports::{Board, ClaimOutcome, ClaimRecord, IssueTypes, NewQuestion, PortError};

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
    /// Closed as not planned (or as a duplicate), not as completed.
    not_planned: bool,
    epic: bool,
    group: Option<GroupName>,
    signature: Option<Signature>,
}

impl Issue {
    /// Where it stands: `done` by its label, else open, else closed as not planned, else
    /// closed as completed and so done.
    fn standing(&self) -> Standing {
        if self.ticket.state == Some(TicketState::Done) {
            Standing::Done
        } else if !self.closed {
            Standing::Open
        } else if self.not_planned {
            Standing::ClosedNotPlanned
        } else {
            Standing::Done
        }
    }
}

#[derive(Debug, Default)]
struct State {
    issues: BTreeMap<TicketId, Issue>,
    /// The issue types the repository defines; none until [`FakeBoard::set_issue_types`],
    /// when it defines [`DEFAULT_ISSUE_TYPES`].
    types: Option<Vec<TicketType>>,
}

/// The issue types the in-memory repository defines until told otherwise: GitHub's own.
pub const DEFAULT_ISSUE_TYPES: [&str; 3] = ["Task", "Bug", "Feature"];

/// What the in-memory board calls its repository.
pub const REPO: &str = "board.invalid/repo";

/// Where the in-memory board says a ticket it opened can be seen: an address that leads
/// nowhere, so nothing mistakes it for a real link.
pub fn link(number: TicketId) -> String {
    format!("https://board.invalid/tickets/{}", number.get())
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
            not_planned: false,
            epic: false,
            group: None,
            signature: None,
        };
        self.state().issues.insert(ticket.id, issue);
    }

    /// Puts an open epic on the board, as [`Board::epics`] lists it.
    pub fn add_epic(&self, number: TicketId, title: &str) {
        self.add_ticket(Ticket {
            id: number,
            epic: None,
            title: title.into(),
            body: "".into(),
            state: None,
            url: "".into(),
            kind: None,
        });
        if let Some(issue) = self.state().issues.get_mut(&number) {
            issue.epic = true;
        }
    }

    /// Puts an open epic on the board that the teams may build: signed on its body and
    /// challenged (a challenger's report is on it).
    pub fn add_challenged_epic(&self, number: TicketId, title: &str) {
        self.add_epic(number, title);
        let mut state = self.state();
        if let Some(issue) = state.issues.get_mut(&number) {
            issue.signature = Some(Signature::of(&issue.ticket.body, SystemTime::UNIX_EPOCH));
            let challenger = Instance {
                role: CHALLENGER_ROLE.into(),
                epic: number,
                number: 1.into(),
            };
            issue.reports.push((challenger, "pass".to_owned()));
        }
    }

    /// Replaces a ticket's body, as editing it on the forge would.
    pub fn set_body(&self, id: TicketId, body: &str) {
        if let Some(issue) = self.state().issues.get_mut(&id) {
            issue.ticket.body = body.into();
        }
    }

    /// Gives an issue a type, as setting its issue type on the forge would.
    pub fn set_type(&self, id: TicketId, kind: &str) {
        if let Some(issue) = self.state().issues.get_mut(&id) {
            issue.ticket.kind = Some(kind.into());
        }
    }

    /// Sets the issue types the repository defines, replacing [`DEFAULT_ISSUE_TYPES`].
    pub fn set_issue_types(&self, names: &[&str]) {
        self.state().types = Some(names.iter().map(|name| TicketType::from(*name)).collect());
    }

    /// Gives a task the `group:<name>` label.
    pub fn set_group(&self, task: TicketId, group: &str) {
        if let Some(issue) = self.state().issues.get_mut(&task) {
            issue.group = Some(group.into());
        }
    }

    /// Closes an issue as completed, without any ruling, as closing it on the forge would.
    pub fn close(&self, issue: TicketId) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            issue.closed = true;
        }
    }

    /// Closes an issue as not planned, as folding a task into a unit or dropping it would.
    pub fn close_not_planned(&self, issue: TicketId) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            issue.closed = true;
            issue.not_planned = true;
        }
    }

    /// Puts an open question ticket on the board, labelled `question`, as `open_question` would.
    pub fn add_question(&self, ticket: Ticket) {
        let number = ticket.id;
        self.add_ticket(Ticket {
            state: Some(TicketState::Question),
            ..ticket
        });
        if let Some(issue) = self.state().issues.get_mut(&number) {
            issue.question = true;
        }
    }

    /// The comments on an issue, oldest first.
    pub fn comments(&self, issue: TicketId) -> Vec<String> {
        self.state()
            .issues
            .get(&issue)
            .map(|issue| issue.comments.clone())
            .unwrap_or_default()
    }

    /// The status comments on a task, one per instance.
    pub fn reports(&self, task: TicketId) -> Vec<(Instance, String)> {
        self.state()
            .issues
            .get(&task)
            .map(|issue| issue.reports.clone())
            .unwrap_or_default()
    }

    /// Whether a question was raised as a door, and its ruling if there is one.
    pub fn question(&self, question: TicketId) -> Option<(bool, Option<String>)> {
        self.state()
            .issues
            .get(&question)
            .map(|issue| (issue.door, issue.ruling.clone()))
    }

    /// Whether an issue is closed, as recording a ruling closes a question.
    pub fn is_closed(&self, issue: TicketId) -> bool {
        self.state()
            .issues
            .get(&issue)
            .is_some_and(|issue| issue.closed)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_found(issue: TicketId) -> PortError {
    PortError::NotFound(format!("issue {issue}"))
}

impl State {
    fn issue_mut(&mut self, number: TicketId) -> Result<&mut Issue, PortError> {
        self.issues
            .get_mut(&number)
            .ok_or_else(|| not_found(number))
    }
}

impl Board for FakeBoard {
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        let state = self.state();
        let epics = state
            .issues
            .values()
            .filter(|issue| issue.epic && !issue.closed)
            .map(|epic| Epic {
                id: epic.ticket.id,
                title: epic.ticket.title.clone(),
                body: epic.ticket.body.clone(),
                tasks: state
                    .issues
                    .values()
                    .filter(|task| task.ticket.epic == Some(epic.ticket.id))
                    .map(|task| EpicTask {
                        number: task.ticket.id,
                        title: task.ticket.title.clone(),
                        state: task.ticket.state,
                        standing: task.standing(),
                        group: task.group.clone(),
                        kind: task.ticket.kind.clone(),
                    })
                    .collect(),
                signature: epic.signature.clone(),
                // The fake keeps no time on a comment.
                planned_at: None,
                challenged: epic
                    .reports
                    .iter()
                    .any(|(instance, _)| instance.role.as_str() == CHALLENGER_ROLE),
            })
            .collect();
        Ok(epics)
    }

    fn issue_types(&self) -> Result<IssueTypes, PortError> {
        let names = self.state().types.clone().unwrap_or_else(|| {
            DEFAULT_ISSUE_TYPES
                .iter()
                .map(|name| TicketType::from(*name))
                .collect()
        });
        Ok(IssueTypes {
            repo: REPO.to_owned(),
            names,
        })
    }

    fn file_task(&self, task: &NewTask) -> Result<TicketId, PortError> {
        let types = self.issue_types()?;
        if !types.defines(&task.kind) {
            return Err(PortError::Rejected(format!(
                "{} has no issue type {}",
                types.repo, task.kind
            )));
        }
        let id = {
            let mut state = self.state();
            state.issue_mut(task.epic)?;
            let highest = state.issues.keys().next_back().map_or(0, |id| id.get());
            TicketId::from(highest + 1)
        };
        self.add_ticket(Ticket {
            id,
            epic: Some(task.epic),
            title: task.title.clone(),
            body: task.body.clone(),
            state: Some(task.state),
            url: link(id).into(),
            kind: Some(task.kind.clone()),
        });
        if let Some(group) = &task.group {
            self.set_group(id, group.as_str());
        }
        Ok(id)
    }

    fn sign(&self, epic: TicketId, signature: &Signature) -> Result<(), PortError> {
        let mut state = self.state();
        state.issue_mut(epic)?.signature = Some(signature.clone());
        Ok(())
    }

    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        let state = self.state();
        Ok(state
            .issues
            .get(&number)
            .map_or(Standing::Missing, Issue::standing))
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        let ready = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.ticket.state == Some(TicketState::Ready))
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(ready)
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        self.state()
            .issues
            .get(&number)
            .map(|issue| issue.ticket.clone())
            .ok_or_else(|| not_found(number))
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
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
        if issue.ticket.state != Some(TicketState::Ready) {
            return Err(PortError::Rejected(format!("{task} is not ready")));
        }
        let claim = ClaimRecord {
            task,
            instance: instance.clone(),
            heartbeat: SystemTime::now(),
        };
        issue.claim = Some(claim);
        issue.ticket.state = Some(TicketState::Claimed);
        Ok(ClaimOutcome::Claimed)
    }

    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
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

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        if !issue
            .claim
            .as_ref()
            .is_some_and(|claim| claim.instance == *instance)
        {
            return Ok(());
        }
        issue.claim = None;
        if !matches!(
            issue.ticket.state,
            Some(TicketState::Question | TicketState::Done)
        ) {
            issue.ticket.state = Some(TicketState::Ready);
        }
        let at = humantime::format_rfc3339_seconds(SystemTime::now());
        issue
            .comments
            .push(format!("released from {instance} at {at}"));
        Ok(())
    }

    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        let mut state = self.state();
        state.issue_mut(task)?.ticket.state = Some(label);
        Ok(())
    }

    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        let mut state = self.state();
        state.issue_mut(issue)?.comments.push(body.to_owned());
        Ok(())
    }

    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
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

    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        let raised = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.question)
            .filter(|issue| NewQuestion::raised_on(issue.ticket.body.as_str()) == Some(task))
            .map(|issue| (issue.ticket.clone(), issue.ruling.clone()))
            .collect();
        Ok(raised)
    }

    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        let mut state = self.state();
        state.issue_mut(question.on)?;
        let highest = state
            .issues
            .keys()
            .next_back()
            .map_or(0, |number| number.get());
        let number = TicketId::from(highest + 1);
        let ticket = Ticket {
            id: number,
            epic: None,
            title: question.title().into(),
            body: question.body().into(),
            state: Some(TicketState::Question),
            url: link(number).into(),
            kind: None,
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
            not_planned: false,
            epic: false,
            group: None,
            signature: None,
        };
        state.issues.insert(number, issue);
        Ok(number)
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        let mut state = self.state();
        let issue = state.issue_mut(question)?;
        match &issue.ruling {
            Some(existing) if existing != ruling => {
                let different_ruling_error =
                    PortError::Conflict(format!("{question} already has a different ruling"));
                return Err(different_ruling_error);
            }
            Some(_) => {}
            None => {
                issue.ruling = Some(ruling.to_owned());
                issue.closed = true;
            }
        }
        let Some(task) = NewQuestion::raised_on(issue.ticket.body.as_str()) else {
            return Ok(None);
        };
        let waiting = state.issue_mut(task)?;
        if waiting.ticket.state != Some(TicketState::Question) {
            return Ok(None);
        }
        waiting.ticket.state = Some(TicketState::Ready);
        waiting.claim = None;
        Ok(Some(task))
    }
}

#[cfg(test)]
mod tests;
