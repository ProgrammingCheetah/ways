//! The in-memory board.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use super::calls::Calls;
use crate::domain::schedule::Priority;
use crate::domain::{
    CHALLENGER_ROLE, Epic, EpicRecord, EpicTask, GroupName, Instance, NewEpic, NewTask, OpenTicket,
    RecordComment, Signature, Standing, StandingAt, Ticket, TicketId, TicketState, TicketType,
    report_graph,
};
use crate::ports::{
    AccessCheck, Board, ClaimOutcome, ClaimRecord, IssueTypes, Login, NewQuestion, PortError,
};

/// A comment on an in-memory issue, with the login that posted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeComment {
    /// Who posted it.
    pub author: Login,
    /// What it says.
    pub body: String,
}

/// The login the in-memory board posts as until told otherwise ([`FakeBoard::set_own_login`]).
pub const OWN_LOGIN: &str = "zaf-bot";

/// One issue and everything written on it.
#[derive(Debug, Clone)]
struct Issue {
    ticket: Ticket,
    comments: Vec<FakeComment>,
    /// Each status report: who posted it, for which instance, and its text.
    reports: Vec<(Login, Instance, String)>,
    claim: Option<ClaimRecord>,
    door: bool,
    question: bool,
    ruling: Option<String>,
    closed: bool,
    /// Closed as not planned (or as a duplicate), not as completed.
    not_planned: bool,
    epic: bool,
    group: Option<GroupName>,
    /// Each signature and who posted it, oldest first.
    signatures: Vec<(Login, Signature)>,
    /// The labels a person put on it that the board's own fields do not hold (`priority/1`).
    labels: Vec<String>,
    /// When it was opened; none unless told.
    opened_at: Option<SystemTime>,
    /// When it was closed; none unless told, and none while it is open.
    closed_at: Option<SystemTime>,
}

impl Issue {
    /// An issue holding `ticket`, with nothing written on it.
    fn of(ticket: Ticket) -> Self {
        Self {
            ticket,
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
            signatures: Vec::new(),
            labels: Vec::new(),
            opened_at: None,
            closed_at: None,
        }
    }

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
    /// Whether a read of the questions raised on a ticket fails ([`FakeBoard::fail_question_reads`]).
    question_reads_fail: bool,
    /// How many reads of the questions raised on a ticket were made.
    question_reads: usize,
    /// The login the board posts as; [`OWN_LOGIN`] when none is set.
    own_login: Option<Login>,
    /// The logins the manifest lists for the board; none lists none.
    trusted: Vec<Login>,
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
    calls: Calls,
}

impl FakeBoard {
    /// Puts a ticket on the board, replacing one with the same number.
    pub fn add_ticket(&self, ticket: Ticket) {
        let id = ticket.id;
        self.state().issues.insert(id, Issue::of(ticket));
    }

    /// Puts a label on an issue, as a person would on the forge: one the board's own fields do not
    /// hold, such as `priority/1`. A label already there is not put twice.
    pub fn add_label(&self, id: TicketId, label: &str) {
        if let Some(issue) = self.state().issues.get_mut(&id)
            && !issue.labels.iter().any(|present| present == label)
        {
            issue.labels.push(label.to_owned());
        }
    }

    /// The labels a person put on an issue ([`FakeBoard::add_label`]), in the order put.
    pub fn labels(&self, id: TicketId) -> Vec<String> {
        self.state()
            .issues
            .get(&id)
            .map(|issue| issue.labels.clone())
            .unwrap_or_default()
    }

    /// Records when an issue was opened, as the forge would.
    pub fn set_opened_at(&self, id: TicketId, at: SystemTime) {
        if let Some(issue) = self.state().issues.get_mut(&id) {
            issue.opened_at = Some(at);
        }
    }

    /// Closes an issue as completed at `at`, as closing it on the forge then would.
    pub fn close_at(&self, id: TicketId, at: SystemTime) {
        if let Some(issue) = self.state().issues.get_mut(&id) {
            issue.closed = true;
            issue.closed_at = Some(at);
        }
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

    /// Puts an open epic on the board that is signed on its body but not challenged yet.
    pub fn add_signed_epic(&self, number: TicketId, title: &str) {
        self.add_epic(number, title);
        let mut state = self.state();
        let own = state.own();
        if let Some(issue) = state.issues.get_mut(&number) {
            let signature = Signature::of(&issue.ticket.body, SystemTime::UNIX_EPOCH);
            issue.signatures.push((own, signature));
        }
    }

    /// Puts an open epic on the board that the teams may build: signed on its body and
    /// challenged (a challenger's report is on it).
    pub fn add_challenged_epic(&self, number: TicketId, title: &str) {
        self.add_epic(number, title);
        let mut state = self.state();
        let own = state.own();
        if let Some(issue) = state.issues.get_mut(&number) {
            let signature = Signature::of(&issue.ticket.body, SystemTime::UNIX_EPOCH);
            issue.signatures.push((own.clone(), signature));
            let challenger = Instance {
                role: CHALLENGER_ROLE.into(),
                epic: number,
                number: 1.into(),
            };
            issue.reports.push((own, challenger, "pass".to_owned()));
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

    /// Opens a closed issue again, as a person reopening it would.
    pub fn reopen(&self, issue: TicketId) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            issue.closed = false;
            issue.not_planned = false;
            issue.closed_at = None;
        }
    }

    /// How an issue is closed: none while it is open, `Some(true)` closed as completed,
    /// `Some(false)` closed as not planned. An issue the board does not hold is none.
    pub fn closed_as(&self, issue: TicketId) -> Option<bool> {
        let state = self.state();
        let issue = state.issues.get(&issue)?;
        issue.closed.then_some(!issue.not_planned)
    }

    /// Closes an issue as not planned, as folding a task into a unit or dropping it would.
    pub fn close_not_planned(&self, issue: TicketId) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            issue.closed = true;
            issue.not_planned = true;
        }
    }

    /// Makes every read of the questions raised on one ticket ([`Board::questions_on`]) fail.
    pub fn fail_question_reads(&self) {
        self.state().question_reads_fail = true;
    }

    /// How many times the questions raised on one ticket were read ([`Board::questions_on`]).
    pub fn question_reads(&self) -> usize {
        self.state().question_reads
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
            .map(|issue| issue.comments.iter().map(|c| c.body.clone()).collect())
            .unwrap_or_default()
    }

    /// The comments on an issue with the login that posted each, oldest first.
    pub fn comment_rows(&self, issue: TicketId) -> Vec<FakeComment> {
        self.state()
            .issues
            .get(&issue)
            .map(|issue| issue.comments.clone())
            .unwrap_or_default()
    }

    /// Posts a comment on an issue as `author`, as that user would on the forge. A comment the
    /// board posts itself ([`Board::comment`]) is by its own login.
    pub fn comment_as(&self, issue: TicketId, author: &str, body: &str) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            issue.comments.push(FakeComment {
                author: Login::from(author),
                body: body.to_owned(),
            });
        }
    }

    /// Posts a signature comment on an epic as `author`, as that user would on the forge. A
    /// signature the board posts itself ([`Board::sign`]) is by its own login.
    pub fn sign_as(&self, epic: TicketId, author: &str, signature: &Signature) {
        if let Some(issue) = self.state().issues.get_mut(&epic) {
            let signed = (Login::from(author), signature.clone());
            issue.signatures.push(signed);
        }
    }

    /// Posts `instance`'s status report on an issue as `author`, as that user would on the
    /// forge by writing a comment under the instance's report marker. A report the board posts
    /// itself ([`Board::report`]) is by its own login.
    pub fn report_as(&self, issue: TicketId, author: &str, instance: &Instance, body: &str) {
        if let Some(issue) = self.state().issues.get_mut(&issue) {
            let report = (Login::from(author), instance.clone(), body.to_owned());
            issue.reports.push(report);
        }
    }

    /// Sets the login the board posts as, [`OWN_LOGIN`] until then.
    pub fn set_own_login(&self, login: &str) {
        self.state().own_login = Some(Login::from(login));
    }

    /// Sets the logins the manifest lists for the board, as `[board] trusted_logins` does.
    pub fn set_trusted_logins(&self, logins: &[&str]) {
        self.state().trusted = logins.iter().copied().map(Login::from).collect();
    }

    /// The status comments on a task, one per instance.
    pub fn reports(&self, task: TicketId) -> Vec<(Instance, String)> {
        self.state()
            .issues
            .get(&task)
            .map(|issue| {
                let reports = issue.reports.iter();
                reports
                    .map(|(_, instance, body)| (instance.clone(), body.clone()))
                    .collect()
            })
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

    /// How many times the port's method `method` was called (`epics`, `task`, ...); 0 for one never
    /// called. Only calls through [`Board`] count, not the methods that arrange a scene.
    pub fn calls(&self, method: &str) -> usize {
        self.calls.of(method)
    }

    /// Sets every count [`FakeBoard::calls`] reads to 0.
    pub fn reset_calls(&self) {
        self.calls.reset();
    }

    /// Every open epic, as [`Board::epics`] lists it, without counting a call.
    fn open_epics(&self) -> Vec<Epic> {
        let state = self.state();
        state
            .issues
            .values()
            .filter(|issue| issue.epic && !issue.closed)
            .map(|epic| (epic, state.trusted()))
            .map(|(epic, trusted)| Epic {
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
                        body: task.ticket.body.clone(),
                        url: task.ticket.url.clone(),
                        state: task.ticket.state,
                        standing: task.standing(),
                        group: task.group.clone(),
                        kind: task.ticket.kind.clone(),
                        closed_at: task.closed.then_some(task.closed_at).flatten(),
                    })
                    .collect(),
                signature: epic
                    .signatures
                    .iter()
                    .rfind(|(author, _)| trusted.contains(author))
                    .map(|(_, signature)| signature.clone()),
                // The fake keeps no time on a comment.
                planned_at: None,
                challengers: epic
                    .reports
                    .iter()
                    .filter(|(author, instance, _)| {
                        instance.role.as_str() == CHALLENGER_ROLE && trusted.contains(author)
                    })
                    .map(|(_, _, body)| report_graph(body))
                    .collect(),
                records: epic_records(epic, &trusted),
            })
            .collect()
    }

    /// The issue types the repository defines, without counting a call.
    fn defined_types(&self) -> IssueTypes {
        let names = self.state().types.clone().unwrap_or_else(|| {
            DEFAULT_ISSUE_TYPES
                .iter()
                .map(|name| TicketType::from(*name))
                .collect()
        });
        IssueTypes {
            repo: REPO.to_owned(),
            names,
        }
    }

    /// Where an issue stands and when it closed, without counting a call.
    fn standing_of(&self, number: TicketId) -> StandingAt {
        let state = self.state();
        state.issues.get(&number).map_or(
            StandingAt {
                standing: Standing::Missing,
                closed_at: None,
            },
            |issue| StandingAt {
                standing: issue.standing(),
                closed_at: issue.closed.then_some(issue.closed_at).flatten(),
            },
        )
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The plan records on `epic` that one of `trusted` posted, in posting order, as a real board
/// reads them. The fake keeps no time on a comment.
fn epic_records(epic: &Issue, trusted: &[Login]) -> Vec<EpicRecord> {
    let mut records = Vec::new();
    for comment in &epic.comments {
        if trusted.contains(&comment.author)
            && let Some(read) = RecordComment::read(&comment.body)
        {
            read.file_into(&mut records, None);
        }
    }
    records
}

fn not_found(issue: TicketId) -> PortError {
    PortError::NotFound(format!("issue {issue}"))
}

impl State {
    /// The number after the highest one on the board.
    fn next_id(&self) -> TicketId {
        let highest = self.issues.keys().next_back().map_or(0, |id| id.get());
        TicketId::from(highest + 1)
    }

    /// The login the board posts as.
    fn own(&self) -> Login {
        self.own_login
            .clone()
            .unwrap_or_else(|| Login::from(OWN_LOGIN))
    }

    /// The trusted logins: the board's own login, then each listed one that is not it.
    fn trusted(&self) -> Vec<Login> {
        let own = self.own();
        let listed: Vec<Login> = self
            .trusted
            .iter()
            .filter(|login| **login != own)
            .cloned()
            .collect();
        std::iter::once(own).chain(listed).collect()
    }

    fn issue_mut(&mut self, number: TicketId) -> Result<&mut Issue, PortError> {
        self.issues
            .get_mut(&number)
            .ok_or_else(|| not_found(number))
    }
}

impl Board for FakeBoard {
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        self.calls.record("epics");
        Ok(self.open_epics())
    }

    fn epic(&self, id: TicketId) -> Result<Epic, PortError> {
        self.calls.record("epic");
        self.open_epics()
            .into_iter()
            .find(|epic| epic.id == id)
            .ok_or_else(|| PortError::NotFound(format!("{id} is not an open epic")))
    }

    fn issue_types(&self) -> Result<IssueTypes, PortError> {
        self.calls.record("issue_types");
        Ok(self.defined_types())
    }

    fn access(&self) -> Vec<AccessCheck> {
        self.calls.record("access");
        Vec::new()
    }

    fn file_task(&self, task: &NewTask) -> Result<TicketId, PortError> {
        self.calls.record("file_task");
        let types = self.defined_types();
        if !types.defines(&task.kind) {
            return Err(PortError::Rejected(format!(
                "{} has no issue type {}",
                types.repo, task.kind
            )));
        }
        // The id, the ticket and its group are set under one lock, so two calls at once
        // never share an id.
        let mut state = self.state();
        state.issue_mut(task.epic)?;
        let id = state.next_id();
        let mut issue = Issue::of(Ticket {
            id,
            epic: Some(task.epic),
            title: task.title.clone(),
            body: task.body.clone(),
            state: Some(task.state),
            url: link(id).into(),
            kind: Some(task.kind.clone()),
        });
        issue.group = task.group.clone();
        state.issues.insert(id, issue);
        Ok(id)
    }

    fn file_epic(&self, epic: &NewEpic) -> Result<TicketId, PortError> {
        self.calls.record("file_epic");
        // The id, the ticket and its epic mark are set under one lock: no other call takes
        // the id, and no reader sees the epic as a task.
        let mut state = self.state();
        let id = state.next_id();
        let mut issue = Issue::of(Ticket {
            id,
            epic: None,
            title: epic.title.clone(),
            body: epic.body.clone(),
            state: None,
            url: link(id).into(),
            kind: None,
        });
        issue.epic = true;
        state.issues.insert(id, issue);
        Ok(id)
    }

    fn sign(&self, epic: TicketId, signature: &Signature) -> Result<(), PortError> {
        self.calls.record("sign");
        let mut state = self.state();
        let own = state.own();
        let signed = (own, signature.clone());
        state.issue_mut(epic)?.signatures.push(signed);
        Ok(())
    }

    fn drop_task(&self, task: TicketId) -> Result<(), PortError> {
        self.calls.record("drop_task");
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        if !issue.closed {
            issue.closed = true;
            issue.not_planned = true;
        }
        Ok(())
    }

    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        self.calls.record("standing");
        Ok(self.standing_of(number).standing)
    }

    fn standing_at(&self, number: TicketId) -> Result<StandingAt, PortError> {
        self.calls.record("standing_at");
        Ok(self.standing_of(number))
    }

    /// The time a test set with [`FakeBoard::set_opened_at`]; none when it set none or the
    /// issue is not there.
    fn opened_at(&self, number: TicketId) -> Result<Option<SystemTime>, PortError> {
        self.calls.record("opened_at");
        Ok(self
            .state()
            .issues
            .get(&number)
            .and_then(|issue| issue.opened_at))
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.calls.record("ready");
        let ready = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.ticket.state == Some(TicketState::Ready))
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(ready)
    }

    /// The open issues in the state `blocked`, in number order.
    fn blocked(&self) -> Result<Vec<Ticket>, PortError> {
        self.calls.record("blocked");
        let blocked = self
            .state()
            .issues
            .values()
            .filter(|issue| !issue.closed && issue.ticket.state == Some(TicketState::Blocked))
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(blocked)
    }

    /// The open tasks not in `done`, in number order: every ticket added that is no epic and no
    /// question, whatever its state, each with its `priority/<n>` label and the time it opened.
    fn open_tickets(&self) -> Result<Vec<OpenTicket>, PortError> {
        self.calls.record("open_tickets");
        let open = self
            .state()
            .issues
            .values()
            .filter(|issue| !issue.epic && !issue.question && !issue.closed)
            .filter(|issue| issue.ticket.state != Some(TicketState::Done))
            .map(|issue| OpenTicket {
                ticket: issue.ticket.clone(),
                priority: Priority::from_labels(issue.labels.iter().map(String::as_str)),
                opened_at: issue.opened_at,
            })
            .collect();
        Ok(open)
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        self.calls.record("task");
        self.state()
            .issues
            .get(&number)
            .map(|issue| issue.ticket.clone())
            .ok_or_else(|| not_found(number))
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        self.calls.record("claim");
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
        self.calls.record("heartbeat");
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
        self.calls.record("claims");
        let claims = self
            .state()
            .issues
            .values()
            .filter_map(|issue| issue.claim.clone())
            .collect();
        Ok(claims)
    }

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.calls.record("release");
        let mut state = self.state();
        let own = state.own();
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
        issue.comments.push(FakeComment {
            author: own,
            body: format!("released from {instance} at {at}"),
        });
        Ok(())
    }

    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        self.calls.record("set_state");
        let mut state = self.state();
        let issue = state.issue_mut(task)?;
        issue.ticket.state = Some(label);
        // Done closes the issue too, as completed.
        if label == TicketState::Done && !issue.closed {
            issue.closed = true;
            issue.not_planned = false;
        }
        Ok(())
    }

    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.calls.record("comment");
        let mut state = self.state();
        let author = state.own();
        state.issue_mut(issue)?.comments.push(FakeComment {
            author,
            body: body.to_owned(),
        });
        Ok(())
    }

    fn trusted_logins(&self) -> Result<Vec<Login>, PortError> {
        self.calls.record("trusted_logins");
        Ok(self.state().trusted())
    }

    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.calls.record("report");
        let mut state = self.state();
        let own = state.own();
        let issue = state.issue_mut(task)?;
        let existing = issue
            .reports
            .iter_mut()
            .find(|(_, reporter, _)| reporter == instance);
        match existing {
            Some((_, _, report)) => *report = body.to_owned(),
            None => issue.reports.push((own, instance.clone(), body.to_owned())),
        }
        Ok(())
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.calls.record("questions");
        let questions = self
            .state()
            .issues
            .values()
            .filter(|issue| issue.question && !issue.closed)
            .map(|issue| issue.ticket.clone())
            .collect();
        Ok(questions)
    }

    fn open_doors(&self) -> Result<Vec<TicketId>, PortError> {
        self.calls.record("open_doors");
        let mut doors: Vec<TicketId> = self
            .state()
            .issues
            .iter()
            .filter(|(_, issue)| issue.question && issue.door && !issue.closed)
            .map(|(id, _)| *id)
            .collect();
        doors.sort();
        Ok(doors)
    }

    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        self.calls.record("questions_on");
        let mut state = self.state();
        state.question_reads += 1;
        if state.question_reads_fail {
            return Err(PortError::Unavailable("the board is down".to_owned()));
        }
        let raised = state
            .issues
            .values()
            .filter(|issue| issue.question)
            .filter(|issue| NewQuestion::raised_on(issue.ticket.body.as_str()) == Some(task))
            .map(|issue| (issue.ticket.clone(), issue.ruling.clone()))
            .collect();
        Ok(raised)
    }

    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        self.calls.record("open_question");
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
            door: question.door,
            question: true,
            ..Issue::of(ticket)
        };
        state.issues.insert(number, issue);
        Ok(number)
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        self.calls.record("record_ruling");
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
