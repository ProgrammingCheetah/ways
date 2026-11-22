//! The board on Forgejo: board kind `forgejo-issues`.
//!
//! Tickets are the issues of one repository, read and written over the REST API through
//! [`ForgejoApi`]. Forgejo has no sub-issues and no issue types, so an epic holds its tasks
//! as issue dependencies (the epic depends on each task, the Engineer's ruling of
//! 2026-11-12), and a ticket's type and a task's state are exclusive scoped labels (see
//! [`super::labels`]). Comments, claims, questions, rulings, signatures and the kind labels
//! are the GitHub issues board's, read and written by the same shared code.

use std::time::SystemTime;

use serde_json::json;

use super::access::{self, Write};
use super::api::{ForgejoApi, Method};
use super::labels::{self, LabelSpec, RepoLabels};
use super::rows::{CommentRow, IssueRow, LabelRow};
use crate::adapters::issues::{
    DOOR_LABEL, EPIC_LABEL, QUESTION_LABEL, TASK_LABEL, can_wait, group, keeps_state_on_release,
    waits_on_ruling,
};
use crate::adapters::notes::{
    EpicNotes, LeaseNote, RULING_MARKER, SIGNATURE_TEXT, held_by, live_claim, marked,
    recorded_ruling, release_line, report_marker, signature_marker, unmarked,
};
use crate::domain::schedule::Priority;
use crate::domain::{
    Epic, EpicTask, Instance, NewEpic, NewTask, OpenTicket, Signature, Standing, StandingAt,
    Ticket, TicketId, TicketState,
};
use crate::ports::{
    AccessCheck, Board, ClaimOutcome, ClaimRecord, IssueTypes, NewQuestion, PortError,
};

/// A board kept as the issues of one Forgejo repository.
///
/// A task is an issue labelled `task`, its epic the issue labelled `epic` that depends on it;
/// its state is one `state/<state>` label. A claim is the `claimed` state plus a
/// `claimed by …` comment; a heartbeat is a new `heartbeat …` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoBoard {
    api: ForgejoApi,
}

impl From<ForgejoApi> for ForgejoBoard {
    fn from(api: ForgejoApi) -> Self {
        Self { api }
    }
}

impl ForgejoBoard {
    fn issue(&self, number: TicketId) -> Result<IssueRow, PortError> {
        self.api.get(&format!("issues/{}", number.get()))
    }

    /// An issue's comments, oldest first.
    fn comments(&self, number: TicketId) -> Result<Vec<CommentRow>, PortError> {
        let mut comments: Vec<CommentRow> =
            self.api.get(&format!("issues/{}/comments", number.get()))?;
        comments.sort_by_key(|comment| comment.id);
        Ok(comments)
    }

    /// The issues, open (`state = "open"`) or all (`"all"`), carrying every label in `labels`,
    /// in number order. Forgejo filters by label name and drops a name the repository lacks,
    /// so each row is checked for the labels again here.
    fn issues(&self, labels: &[&str], state: &str) -> Result<Vec<IssueRow>, PortError> {
        let names = labels
            .iter()
            .map(|label| super::api::query_value(label))
            .collect::<Vec<_>>()
            .join(",");
        let mut rows: Vec<IssueRow> = self
            .api
            .list(&format!("issues?state={state}&type=issues&labels={names}"))?;
        rows.retain(|row| !row.is_pull() && labels.iter().all(|label| row.has_label(label)));
        rows.sort_by_key(|row| row.number);
        Ok(rows)
    }

    /// The comments of each of `issues`, each oldest first, read together, in the order asked.
    fn comments_of(&self, issues: &[TicketId]) -> Result<Vec<Vec<CommentRow>>, PortError> {
        let endpoints: Vec<String> = issues
            .iter()
            .map(|issue| format!("issues/{}/comments", issue.get()))
            .collect();
        let mut all: Vec<Vec<CommentRow>> = self.api.get_all(&endpoints)?;
        for comments in &mut all {
            comments.sort_by_key(|comment| comment.id);
        }
        Ok(all)
    }

    /// The epic a task is filed under: the oldest issue labelled `epic` that depends on it.
    fn epic_of(&self, number: u64) -> Result<Option<TicketId>, PortError> {
        let blocked: Vec<IssueRow> = self.api.list(&format!("issues/{number}/blocks"))?;
        Ok(oldest_epic(&blocked))
    }

    /// The epic of each of `rows`, read together, in the order asked.
    fn epics_of(&self, rows: &[IssueRow]) -> Result<Vec<Option<TicketId>>, PortError> {
        let endpoints: Vec<String> = rows
            .iter()
            .map(|row| format!("issues/{}/blocks", row.number))
            .collect();
        let blocked: Vec<Vec<IssueRow>> = self.api.list_all(&endpoints)?;
        Ok(blocked.iter().map(|rows| oldest_epic(rows)).collect())
    }

    /// The ticket `row` reads as, in `state`, with its epic.
    fn ticket(&self, row: &IssueRow, state: Option<TicketState>) -> Result<Ticket, PortError> {
        Ok(ticket_of(row, state, self.epic_of(row.number)?))
    }

    /// The tickets of `rows`, each in the state `state` gives it, their epics read together.
    fn tickets_with(
        &self,
        rows: &[IssueRow],
        state: impl Fn(&IssueRow) -> Option<TicketState>,
    ) -> Result<Vec<Ticket>, PortError> {
        let epics = self.epics_of(rows)?;
        Ok(rows
            .iter()
            .zip(epics)
            .map(|(row, epic)| ticket_of(row, state(row), epic))
            .collect())
    }

    /// The open issues labelled `task` and `state/<state>`, in number order, each read in that
    /// state with its epic.
    fn open_tasks_in(&self, state: TicketState) -> Result<Vec<Ticket>, PortError> {
        let label = labels::state_label(state);
        let rows = self.issues(&[TASK_LABEL, &label], "open")?;
        self.tickets_with(&rows, |_| Some(state))
    }

    /// The tickets of `rows`, each in the first state its labels hold.
    fn tickets(&self, rows: &[IssueRow]) -> Result<Vec<Ticket>, PortError> {
        self.tickets_with(rows, |row| states(row).first().copied())
    }

    fn post_comment(&self, number: TicketId, body: &str) -> Result<(), PortError> {
        let endpoint = format!("issues/{}/comments", number.get());
        self.api
            .send::<serde_json::Value>(Method::Post, &endpoint, Some(&json!({ "body": body })))
            .map(drop)
    }

    /// Adds the state `add` and drops every state in `remove` on the issue read as `row`; a
    /// change that would leave it as it is makes no call. Forgejo drops the old state itself
    /// when the new one goes on, being exclusive; whatever state is still on it then (a label
    /// made by hand, not exclusive) is dropped one by one.
    fn change(
        &self,
        row: &IssueRow,
        add: Option<TicketState>,
        remove: &[TicketState],
    ) -> Result<(), PortError> {
        let present = states(row);
        let mut on: Vec<LabelRow> = row.labels.clone();
        if let Some(state) = add.filter(|state| !present.contains(state)) {
            let mut repo = RepoLabels::read(&self.api)?;
            let id = repo.ensure(&self.api, &LabelSpec::state(state))?;
            let endpoint = format!("issues/{}/labels", row.number);
            on = self
                .api
                .send(Method::Post, &endpoint, Some(&json!({ "labels": [id] })))?;
        }
        for label in &on {
            let Some(state) = labels::states([label.name.as_str()]).first().copied() else {
                continue;
            };
            if remove.contains(&state) && Some(state) != add {
                let endpoint = format!("issues/{}/labels/{}", row.number, label.id);
                self.api.send::<()>(Method::Delete, &endpoint, None)?;
            }
        }
        Ok(())
    }

    /// Returns the task a ruled question was raised on to `ready`, when it still waits:
    /// carries `question` and is not `done`. Returns the task it changed.
    fn return_waiting_task(&self, question: &IssueRow) -> Result<Option<TicketId>, PortError> {
        let Some(task) = NewQuestion::raised_on(&question.body) else {
            return Ok(None);
        };
        let row = self.issue(task)?;
        if !waits_on_ruling(row.has_label(EPIC_LABEL), &states(&row)) {
            return Ok(None);
        }
        self.change(&row, Some(TicketState::Ready), &[TicketState::Question])?;
        Ok(Some(task))
    }

    /// The epic `row` reads as, with its comments and the tasks it depends on.
    fn epic_from(&self, row: &IssueRow) -> Result<Epic, PortError> {
        let mut epics = self.epics_from(std::slice::from_ref(row))?;
        epics
            .pop()
            .ok_or_else(|| PortError::Backend(format!("#{} was read as no epic", row.number)))
    }

    /// The epic each of `rows` reads as. Every epic's comments and dependencies are independent
    /// reads, so all of them run together.
    fn epics_from(&self, rows: &[IssueRow]) -> Result<Vec<Epic>, PortError> {
        let ids: Vec<TicketId> = rows.iter().map(|row| TicketId::from(row.number)).collect();
        let endpoints: Vec<String> = rows
            .iter()
            .map(|row| format!("issues/{}/dependencies", row.number))
            .collect();
        let (comments, dependencies) = std::thread::scope(|scope| {
            let dependencies = scope.spawn(|| self.api.list_all::<IssueRow, _>(&endpoints));
            let comments = self.comments_of(&ids);
            let dependencies = dependencies
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            (comments, dependencies)
        });
        let (comments, dependencies) = (comments?, dependencies?);
        Ok(rows
            .iter()
            .zip(comments)
            .zip(dependencies)
            .map(|((row, comments), mut tasks)| {
                let notes = EpicNotes::read(
                    comments
                        .iter()
                        .map(|comment| (comment.body.as_str(), comment.posted())),
                );
                tasks.retain(|task| !task.is_pull() && task.has_label(TASK_LABEL));
                tasks.sort_by_key(|task| task.number);
                tasks.dedup_by_key(|task| task.number);
                Epic {
                    id: TicketId::from(row.number),
                    title: row.title.clone().into(),
                    body: row.body.clone().into(),
                    tasks: tasks.iter().map(epic_task).collect(),
                    signature: notes.signature,
                    planned_at: notes.planned_at,
                    challenged: notes.challenged,
                }
            })
            .collect())
    }
}

/// The oldest of `blocked` labelled `epic`.
fn oldest_epic(blocked: &[IssueRow]) -> Option<TicketId> {
    blocked
        .iter()
        .filter(|row| row.has_label(EPIC_LABEL))
        .map(|row| row.number)
        .min()
        .map(TicketId::from)
}

/// The ticket `row` reads as, in `state`, filed under `epic`.
fn ticket_of(row: &IssueRow, state: Option<TicketState>, epic: Option<TicketId>) -> Ticket {
    Ticket {
        id: TicketId::from(row.number),
        epic,
        title: row.title.clone().into(),
        body: row.body.clone().into(),
        state,
        url: row.html_url.clone().into(),
        kind: labels::kind(row.label_names()),
    }
}

/// The states `row`'s labels hold.
fn states(row: &IssueRow) -> Vec<TicketState> {
    labels::states(row.label_names())
}

/// Where an issue stands from its labels and whether it is open (see [`Standing::of_issue`]).
fn standing(row: &IssueRow, states: &[TicketState]) -> Standing {
    Standing::of_issue(
        row.is_open(),
        labels::dropped(row.label_names()),
        states.contains(&TicketState::Done),
    )
}

/// A task as its epic lists it.
fn epic_task(row: &IssueRow) -> EpicTask {
    let states = states(row);
    EpicTask {
        number: TicketId::from(row.number),
        title: row.title.clone().into(),
        body: row.body.clone().into(),
        url: row.html_url.clone().into(),
        state: states.first().copied(),
        standing: standing(row, &states),
        group: group(row.label_names()).map(Into::into),
        kind: labels::kind(row.label_names()),
        closed_at: row.closed(),
    }
}

impl Board for ForgejoBoard {
    /// The open issues labelled `epic`, each read with its comments and its dependencies: the
    /// tasks, open and closed, in number order.
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        let rows = self.issues(&[EPIC_LABEL], "open")?;
        self.epics_from(&rows)
    }

    /// One issue read: not there, a pull request, closed or not labelled `epic` is no open epic.
    fn epic(&self, id: TicketId) -> Result<Epic, PortError> {
        let not_open_epic = || PortError::NotFound(format!("{id} is not an open epic"));
        let row = match self.issue(id) {
            Ok(row) => row,
            Err(PortError::NotFound(_)) => return Err(not_open_epic()),
            Err(error) => return Err(error),
        };
        if row.is_pull() || !row.is_open() || !row.has_label(EPIC_LABEL) {
            return Err(not_open_epic());
        }
        self.epic_from(&row)
    }

    /// One comment on the epic: the signature's marker over a line a person reads.
    fn sign(&self, epic: TicketId, signature: &Signature) -> Result<(), PortError> {
        let body = marked(&signature_marker(signature), SIGNATURE_TEXT);
        self.comment(epic, &body)
    }

    /// The `type/<Type>` labels of the repository, and `Task`.
    fn issue_types(&self) -> Result<IssueTypes, PortError> {
        Ok(IssueTypes {
            repo: self.api.repo().to_string(),
            names: RepoLabels::read(&self.api)?.types(),
        })
    }

    /// Opens the issue with the labels `task`, `state/<state>`, `type/<Type>` and
    /// `group:<name>` (each created when the repository lacks it), then makes the epic depend
    /// on it. When the epic cannot be made to depend on it the issue is closed and the call
    /// fails, so filing again opens no second task beside an orphan.
    fn file_task(&self, task: &NewTask) -> Result<TicketId, PortError> {
        let mut repo = RepoLabels::read(&self.api)?;
        let mut specs = vec![
            LabelSpec::task(),
            LabelSpec::state(task.state),
            LabelSpec::of_type(&task.kind),
        ];
        specs.extend(task.group.as_ref().map(LabelSpec::group));
        let ids = specs
            .iter()
            .map(|spec| repo.ensure(&self.api, spec))
            .collect::<Result<Vec<u64>, PortError>>()?;
        let body = json!({
            "title": task.title.as_str(),
            "body": task.body.as_str(),
            "labels": ids,
        });
        let opened: IssueRow = self.api.send(Method::Post, "issues", Some(&body))?;
        let (owner, name) = self.api.owner_and_name()?;
        let dependency = json!({ "owner": owner, "repo": name, "index": opened.number });
        let endpoint = format!("issues/{}/dependencies", task.epic.get());
        let linked = self
            .api
            .send::<serde_json::Value>(Method::Post, &endpoint, Some(&dependency));
        if let Err(error) = linked {
            // No epic depends on the issue, so a retry would not find it and would file it
            // again: it is closed, and named when even that fails.
            let closed = self.api.send::<serde_json::Value>(
                Method::Patch,
                &format!("issues/{}", opened.number),
                Some(&json!({ "state": "closed" })),
            );
            return Err(match closed {
                Ok(_) => error,
                Err(_) => PortError::Backend(format!(
                    "{error}; #{} is left open under no epic: close it before filing again",
                    opened.number
                )),
            });
        }
        Ok(TicketId::from(opened.number))
    }

    /// Opens the issue with the label `epic` (created when the repository lacks it), the title
    /// and the body as given: open, with no state and no tasks.
    fn file_epic(&self, epic: &NewEpic) -> Result<TicketId, PortError> {
        let mut repo = RepoLabels::read(&self.api)?;
        let id = repo.ensure(&self.api, &LabelSpec::epic())?;
        let body = json!({
            "title": epic.title.as_str(),
            "body": epic.body.as_str(),
            "labels": [id],
        });
        let opened: IssueRow = self.api.send(Method::Post, "issues", Some(&body))?;
        Ok(TicketId::from(opened.number))
    }

    /// One issue read: one that is not there, or is a pull request, is missing; else `done` by
    /// its label, closed as not planned by its `not planned` or `duplicate` label, closed, or
    /// open.
    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        self.standing_at(number).map(|at| at.standing)
    }

    /// One issue read: [`Board::standing`]'s answer, and its `closed_at` once it is closed.
    fn standing_at(&self, number: TicketId) -> Result<StandingAt, PortError> {
        let missing = StandingAt {
            standing: Standing::Missing,
            closed_at: None,
        };
        let row = match self.issue(number) {
            Ok(row) => row,
            Err(PortError::NotFound(_)) => return Ok(missing),
            Err(error) => return Err(error),
        };
        if row.is_pull() {
            return Ok(missing);
        }
        Ok(StandingAt {
            standing: standing(&row, &states(&row)),
            closed_at: row.closed(),
        })
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.open_tasks_in(TicketState::Ready)
    }

    /// One list, as `ready` reads it: the open tasks labelled `state/blocked`, with their bodies.
    fn blocked(&self) -> Result<Vec<Ticket>, PortError> {
        self.open_tasks_in(TicketState::Blocked)
    }

    /// One list, as `ready` reads it: every open issue labelled `task`, less those labelled
    /// `state/done`, each in the first state its labels hold, with its body and its epic.
    /// One list of the open issues labelled `task`, then each one's epic, as `ready` reads it;
    /// each with its `priority/<n>` label and the time it opened, from the same list.
    fn open_tickets(&self) -> Result<Vec<OpenTicket>, PortError> {
        let mut rows = self.issues(&[TASK_LABEL], "open")?;
        rows.retain(|row| !states(row).contains(&TicketState::Done));
        let tickets = self.tickets(&rows)?;
        Ok(rows
            .iter()
            .zip(tickets)
            .map(|(row, ticket)| OpenTicket {
                ticket,
                priority: Priority::from_labels(row.label_names()),
                opened_at: row.opened(),
            })
            .collect())
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        let row = self.issue(number)?;
        self.ticket(&row, states(&row).first().copied())
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        let row = self.issue(task)?;
        let states = states(&row);
        if states.contains(&TicketState::Claimed) {
            let comments = self.comments(task)?;
            let claim = live_claim(task, true, comments.iter().map(|c| c.body.as_str()));
            return held_by(task, claim, instance).map(|_| ClaimOutcome::AlreadyHeld);
        }
        if !states.contains(&TicketState::Ready) {
            return Err(PortError::Rejected(format!("{task} is not ready")));
        }
        let note = LeaseNote::Claim {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())?;
        self.change(&row, Some(TicketState::Claimed), &[TicketState::Ready])?;
        Ok(ClaimOutcome::Claimed)
    }

    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        let row = self.issue(task)?;
        let claimed = states(&row).contains(&TicketState::Claimed);
        let comments = self.comments(task)?;
        let claim = live_claim(task, claimed, comments.iter().map(|c| c.body.as_str()));
        held_by(task, claim, instance)?;
        let note = LeaseNote::Heartbeat {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        let claimed = labels::state_label(TicketState::Claimed);
        let rows = self.issues(&[&claimed], "open")?;
        let ids: Vec<TicketId> = rows.iter().map(|row| TicketId::from(row.number)).collect();
        let all_comments = self.comments_of(&ids)?;
        let mut claims = Vec::new();
        for ((row, task), comments) in rows.iter().zip(ids).zip(all_comments) {
            let held = states(row).contains(&TicketState::Claimed);
            claims.extend(live_claim(
                task,
                held,
                comments.iter().map(|c| c.body.as_str()),
            ));
        }
        Ok(claims)
    }

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        let row = self.issue(task)?;
        let states = states(&row);
        let comments = self.comments(task)?;
        let claimed = states.contains(&TicketState::Claimed);
        let holder = live_claim(task, claimed, comments.iter().map(|c| c.body.as_str()))
            .map(|claim| claim.instance);
        if holder.as_ref() != Some(instance) {
            return Ok(());
        }
        let add = (!keeps_state_on_release(&states)).then_some(TicketState::Ready);
        self.change(&row, add, &[TicketState::Claimed])?;
        self.post_comment(task, &release_line(instance, SystemTime::now()))
    }

    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        let row = self.issue(task)?;
        let states = states(&row);
        if label == TicketState::Question && !can_wait(row.has_label(EPIC_LABEL), &states) {
            return Ok(());
        }
        let others: Vec<TicketState> = states
            .into_iter()
            .filter(|present| *present != label)
            .collect();
        self.change(&row, Some(label), &others)?;
        // Done closes the issue too; a closed issue carries no `not planned` label, so it reads as
        // completed.
        if label == TicketState::Done && row.is_open() {
            let endpoint = format!("issues/{}", task.get());
            self.api.send::<serde_json::Value>(
                Method::Patch,
                &endpoint,
                Some(&json!({ "state": "closed" })),
            )?;
        }
        Ok(())
    }

    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.post_comment(issue, body)
    }

    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        let marker = report_marker(instance);
        let report = marked(&marker, body);
        let comments = self.comments(task)?;
        let existing = comments
            .iter()
            .find(|comment| unmarked(&marker, &comment.body).is_some());
        match existing {
            Some(comment) if comment.body == report => Ok(()),
            Some(comment) => self
                .api
                .send::<serde_json::Value>(
                    Method::Patch,
                    &format!("issues/comments/{}", comment.id),
                    Some(&json!({ "body": report })),
                )
                .map(drop),
            None => self.post_comment(task, &report),
        }
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        let mut rows = self.issues(&[QUESTION_LABEL], "open")?;
        rows.retain(|row| !row.has_label(TASK_LABEL));
        self.tickets(&rows)
    }

    fn open_doors(&self) -> Result<Vec<TicketId>, PortError> {
        let mut rows = self.issues(&[QUESTION_LABEL, DOOR_LABEL], "open")?;
        rows.retain(|row| !row.has_label(TASK_LABEL));
        Ok(rows.iter().map(|row| TicketId::from(row.number)).collect())
    }

    /// Lists the `question` issues, open and closed, whose body says `Raised on #<task>`,
    /// oldest first, each with the text of its ruling comment when it has one.
    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        let mut rows = self.issues(&[QUESTION_LABEL], "all")?;
        rows.retain(|row| {
            !row.has_label(TASK_LABEL) && NewQuestion::raised_on(&row.body) == Some(task)
        });
        let tickets = self.tickets(&rows)?;
        let ids: Vec<TicketId> = rows.iter().map(|row| TicketId::from(row.number)).collect();
        let all_comments = self.comments_of(&ids)?;
        Ok(tickets
            .into_iter()
            .zip(all_comments)
            .map(|(ticket, comments)| {
                let ruling = recorded_ruling(comments.iter().map(|c| c.body.as_str()))
                    .map(|text| text.trim().to_owned());
                (ticket, ruling)
            })
            .collect())
    }

    /// Opens the issue with `question`, `state/question` and, for a door, `door`.
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        let mut repo = RepoLabels::read(&self.api)?;
        let mut specs = vec![
            LabelSpec::question(),
            LabelSpec::state(TicketState::Question),
        ];
        if question.door {
            specs.push(LabelSpec::door());
        }
        let ids = specs
            .iter()
            .map(|spec| repo.ensure(&self.api, spec))
            .collect::<Result<Vec<u64>, PortError>>()?;
        let body = json!({
            "title": question.title(),
            "body": question.body(),
            "labels": ids,
        });
        let opened: IssueRow = self.api.send(Method::Post, "issues", Some(&body))?;
        Ok(TicketId::from(opened.number))
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        let row = self.issue(question)?;
        let comments = self.comments(question)?;
        match recorded_ruling(comments.iter().map(|c| c.body.as_str())) {
            Some(existing) if existing.trim() != ruling.trim() => {
                let different_ruling_error =
                    PortError::Conflict(format!("{question} already has a different ruling"));
                return Err(different_ruling_error);
            }
            Some(_) => {}
            None => self.post_comment(question, &marked(RULING_MARKER, ruling))?,
        }
        if row.is_open() {
            let endpoint = format!("issues/{}", question.get());
            self.api.send::<serde_json::Value>(
                Method::Patch,
                &endpoint,
                Some(&json!({ "state": "closed" })),
            )?;
        }
        self.return_waiting_task(&row)
    }

    /// The instance's version, then the token: whether it reads the repository and may write
    /// its issues.
    fn access(&self) -> Vec<AccessCheck> {
        let granted = format!("reads and writes {}", self.api.repo());
        access::checks(&self.api, &[Write::Issues], &granted)
    }
}
