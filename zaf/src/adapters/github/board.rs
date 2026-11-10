//! The board on GitHub Issues: board kind `github-issues`.

use std::time::SystemTime;

use super::GhCommand;
use super::command::number_from_url;
use super::notes::{
    LeaseNote, RULING_MARKER, SIGNATURE_TEXT, marked, read_signature, release_line, report_author,
    report_marker, signature_marker, unmarked,
};
use super::rows::{ISSUE_FIELDS, IssueRow, NameRow, NumberRow, STANDING_FIELDS, StandingRow};
use crate::domain::{
    CHALLENGER_ROLE, Epic, EpicTask, Instance, Signature, Standing, Ticket, TicketId, TicketState,
};
use crate::manifest::RepoName;
use crate::ports::{Board, ClaimOutcome, ClaimRecord, NewQuestion, PortError};

/// The label every task carries.
const TASK_LABEL: &str = "task";
/// The label every epic carries.
const EPIC_LABEL: &str = "epic";
/// The label a question raised as a one-way decision carries beside `question`.
const DOOR_LABEL: &str = "door";
/// The most issues one list call reads.
const LIST_LIMIT: &str = "1000";
/// The state gh prints for an open issue.
const OPEN: &str = "OPEN";
/// What a pull request's URL holds and an issue's does not.
const PULL_PATH: &str = "/pull/";

/// The close reason of an issue closed as not planned.
const NOT_PLANNED: &str = "NOT_PLANNED";

/// The close reason of an issue closed as a duplicate.
const DUPLICATE: &str = "DUPLICATE";
/// The prefix of the label that puts a task in a group.
const GROUP_PREFIX: &str = "group:";

/// A board kept as the issues of one GitHub repository.
///
/// A task is an issue labelled `task`, its epic the issue it is a sub-issue of;
/// its state is one [`TicketState`]. A claim is the `claimed` label plus a
/// `claimed by …` comment; a heartbeat is a new `heartbeat …` comment.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct GithubBoard {
    gh: GhCommand,
}

impl GithubBoard {
    fn issue(&self, number: TicketId) -> Result<IssueRow, PortError> {
        let number = number.get().to_string();
        self.gh
            .json(&["issue", "view", &number, "--json", ISSUE_FIELDS])
    }

    fn edit_labels(
        &self,
        number: TicketId,
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), PortError> {
        let number = number.get().to_string();
        let mut args = vec!["issue", "edit", number.as_str()];
        let added = add.join(",");
        let removed = remove.join(",");
        if !add.is_empty() {
            args.extend(["--add-label", added.as_str()]);
        }
        if !remove.is_empty() {
            args.extend(["--remove-label", removed.as_str()]);
        }
        self.gh.text(&args).map(drop)
    }

    fn post_comment(&self, number: TicketId, body: &str) -> Result<(), PortError> {
        let number = number.get().to_string();
        self.gh
            .text(&["issue", "comment", &number, "--body", body])
            .map(drop)
    }

    /// Returns the task a ruled question was raised on to `ready`, when it still waits:
    /// carries `question` and is not `done`. Returns the task it changed.
    fn return_waiting_task(&self, question: &IssueRow) -> Result<Option<TicketId>, PortError> {
        let Some(task) = NewQuestion::raised_on(&question.body) else {
            return Ok(None);
        };
        let row = self.issue(task)?;
        let waiting = TicketState::Question.to_string();
        let done = TicketState::Done.to_string();
        if row.has_label(EPIC_LABEL) || row.has_label(&done) || !row.has_label(&waiting) {
            return Ok(None);
        }
        let ready = TicketState::Ready.to_string();
        self.edit_labels(task, &[&ready], &[&waiting])?;
        Ok(Some(task))
    }

    fn edit_comment(&self, url: &str, body: &str) -> Result<(), PortError> {
        let id = url
            .rsplit_once("#issuecomment-")
            .map(|(_, id)| id)
            .ok_or_else(|| PortError::Backend(format!("no comment id in `{url}`")))?;
        let endpoint = format!("issues/comments/{id}");
        let field = format!("body={body}");
        self.gh
            .api_text(&endpoint, &["--method", "PATCH", "-f", &field])
            .map(drop)
    }
}

/// Where an issue stands from its state, close reason and labels: `done` by its label;
/// else open; else closed as not planned or as a duplicate; else closed as completed, so done.
fn standing_of(state: &str, reason: Option<&str>, labels: &[NameRow]) -> Standing {
    let done = TicketState::Done.to_string();
    let never_done = matches!(reason, Some(NOT_PLANNED | DUPLICATE));
    if labels.iter().any(|label| label.name == done) {
        Standing::Done
    } else if state == OPEN {
        Standing::Open
    } else if never_done {
        Standing::ClosedNotPlanned
    } else {
        Standing::Done
    }
}

impl IssueRow {
    fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|label| label.name == name)
    }

    /// Whether a question raised on this issue makes it wait: a task that is `claimed` or
    /// `ready`. An epic and a task that was never ready carry no state a ruling could restore.
    fn can_wait(&self) -> bool {
        !self.has_label(EPIC_LABEL)
            && [TicketState::Claimed, TicketState::Ready]
                .iter()
                .any(|label| self.has_label(&label.to_string()))
    }

    /// The name in the first `group:<name>` label.
    fn group(&self) -> Option<String> {
        self.labels
            .iter()
            .find_map(|label| label.name.strip_prefix(GROUP_PREFIX))
            .map(str::to_owned)
    }

    fn task_labels(&self) -> impl Iterator<Item = TicketState> + '_ {
        self.labels
            .iter()
            .filter_map(|label| label.name.parse::<TicketState>().ok())
    }

    /// The ticket, its link the issue's page on `repo`.
    fn ticket(&self, repo: &RepoName) -> Ticket {
        Ticket {
            id: TicketId::from(self.number),
            epic: self.parent.map(|parent| TicketId::from(parent.number)),
            title: self.title.clone().into(),
            body: self.body.clone().into(),
            state: self.task_labels().next(),
            url: format!("https://github.com/{repo}/issues/{}", self.number).into(),
        }
    }

    /// The live claim: the latest claim comment, while the issue is labelled
    /// `claimed`, touched by the holder's later heartbeats.
    fn claim(&self) -> Option<ClaimRecord> {
        if !self.has_label(&TicketState::Claimed.to_string()) {
            return None;
        }
        let notes: Vec<LeaseNote> = self
            .comments
            .iter()
            .filter_map(|comment| comment.body.parse::<LeaseNote>().ok())
            .collect();
        let claim_index = notes
            .iter()
            .rposition(|note| matches!(note, LeaseNote::Claim { .. }))?;
        let LeaseNote::Claim { instance, at } = &notes[claim_index] else {
            return None;
        };
        let heartbeat = notes[claim_index..]
            .iter()
            .filter_map(|note| match note {
                LeaseNote::Heartbeat { instance: by, at } if by == instance => Some(*at),
                _ => None,
            })
            .fold(*at, SystemTime::max);
        let record = ClaimRecord {
            task: TicketId::from(self.number),
            instance: instance.clone(),
            heartbeat,
        };
        Some(record)
    }

    /// The claim, which must belong to `instance`.
    fn claim_by(&self, instance: &Instance) -> Result<ClaimRecord, PortError> {
        let task = TicketId::from(self.number);
        let claim = self
            .claim()
            .ok_or_else(|| PortError::Conflict(format!("{task} has no claim")))?;
        if claim.instance != *instance {
            let held_by_other_error =
                PortError::Conflict(format!("{task} is held by {}", claim.instance));
            return Err(held_by_other_error);
        }
        Ok(claim)
    }
}

impl Board for GithubBoard {
    /// Two list calls: the open epics with their bodies and comments, then every task, open
    /// and closed, filed under its parent. The latest signature comment is the signature; a
    /// status report by a challenger instance means the epic was challenged.
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        let mut epics: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            EPIC_LABEL,
            "--state",
            "open",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,body,labels,comments",
        ])?;
        epics.sort_by_key(|row| row.number);
        let mut tasks: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            TASK_LABEL,
            "--state",
            "all",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,labels,parent,state,stateReason",
        ])?;
        tasks.sort_by_key(|row| row.number);
        let epics = epics
            .iter()
            .map(|epic| Epic {
                id: TicketId::from(epic.number),
                title: epic.title.clone().into(),
                body: epic.body.clone().into(),
                tasks: tasks
                    .iter()
                    .filter(|task| {
                        task.parent
                            .is_some_and(|parent| parent.number == epic.number)
                    })
                    .map(|task| EpicTask {
                        number: TicketId::from(task.number),
                        state: task.task_labels().next(),
                        standing: standing_of(
                            &task.state,
                            task.state_reason.as_deref(),
                            &task.labels,
                        ),
                        group: task.group().map(Into::into),
                    })
                    .collect(),
                signature: epic
                    .comments
                    .iter()
                    .filter_map(|comment| read_signature(&comment.body))
                    .next_back(),
                challenged: epic.comments.iter().any(|comment| {
                    report_author(&comment.body)
                        .is_some_and(|author| author.role.as_str() == CHALLENGER_ROLE)
                }),
            })
            .collect();
        Ok(epics)
    }

    /// One comment on the epic: the signature's marker over a line a person reads.
    fn sign(&self, epic: TicketId, signature: &Signature) -> Result<(), PortError> {
        let body = marked(&signature_marker(signature), SIGNATURE_TEXT);
        self.comment(epic, &body)
    }

    /// One `gh issue view`: an issue that is not there, or is a pull request, is missing;
    /// else `done` by its label, closed without it, or open.
    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        let number = number.get().to_string();
        let viewed: Result<StandingRow, PortError> =
            self.gh
                .json(&["issue", "view", &number, "--json", STANDING_FIELDS]);
        let row = match viewed {
            Ok(row) => row,
            Err(PortError::NotFound(_)) => return Ok(Standing::Missing),
            Err(error) => return Err(error),
        };
        Ok(if row.url.contains(PULL_PATH) {
            Standing::Missing
        } else {
            standing_of(&row.state, row.state_reason.as_deref(), &row.labels)
        })
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        let ready = TicketState::Ready.to_string();
        let mut rows: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            TASK_LABEL,
            "--label",
            &ready,
            "--state",
            "open",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,body,labels,parent",
        ])?;
        rows.sort_by_key(|row| row.number);
        Ok(rows.iter().map(|row| row.ticket(self.gh.repo())).collect())
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        Ok(self.issue(number)?.ticket(self.gh.repo()))
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        let row = self.issue(task)?;
        if row.has_label(&TicketState::Claimed.to_string()) {
            return row.claim_by(instance).map(|_| ClaimOutcome::AlreadyHeld);
        }
        if !row.has_label(&TicketState::Ready.to_string()) {
            return Err(PortError::Rejected(format!("{task} is not ready")));
        }
        let note = LeaseNote::Claim {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())?;
        let claimed = TicketState::Claimed.to_string();
        let ready = TicketState::Ready.to_string();
        self.edit_labels(task, &[&claimed], &[&ready])?;
        Ok(ClaimOutcome::Claimed)
    }

    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.issue(task)?.claim_by(instance)?;
        let note = LeaseNote::Heartbeat {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        let claimed = TicketState::Claimed.to_string();
        let mut numbers: Vec<NumberRow> = self.gh.json(&[
            "issue", "list", "--label", &claimed, "--state", "open", "--limit", LIST_LIMIT,
            "--json", "number",
        ])?;
        numbers.sort_by_key(|row| row.number);
        let mut claims = Vec::new();
        for number in numbers {
            let row = self.issue(TicketId::from(number.number))?;
            claims.extend(row.claim());
        }
        Ok(claims)
    }

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        let row = self.issue(task)?;
        let holder = row.claim().map(|claim| claim.instance);
        if holder.as_ref() != Some(instance) {
            return Ok(());
        }
        let claimed = TicketState::Claimed.to_string();
        let ready = TicketState::Ready.to_string();
        let keeps_state = [TicketState::Question, TicketState::Done]
            .iter()
            .any(|label| row.has_label(&label.to_string()));
        let add: Vec<&str> = if keeps_state {
            Vec::new()
        } else {
            vec![ready.as_str()]
        };
        self.edit_labels(task, &add, &[&claimed])?;
        self.post_comment(task, &release_line(instance, SystemTime::now()))
    }

    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        let row = self.issue(task)?;
        if label == TicketState::Question && !row.can_wait() {
            return Ok(());
        }
        let wanted = label.to_string();
        let others: Vec<String> = row
            .task_labels()
            .filter(|present| *present != label)
            .map(|present| present.to_string())
            .collect();
        let add: Vec<&str> = if row.has_label(&wanted) {
            Vec::new()
        } else {
            vec![wanted.as_str()]
        };
        if add.is_empty() && others.is_empty() {
            return Ok(());
        }
        let remove: Vec<&str> = others.iter().map(String::as_str).collect();
        self.edit_labels(task, &add, &remove)
    }

    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.post_comment(issue, body)
    }

    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        let marker = report_marker(instance);
        let report = marked(&marker, body);
        let row = self.issue(task)?;
        let existing = row
            .comments
            .iter()
            .find(|comment| unmarked(&marker, &comment.body).is_some());
        match existing {
            Some(comment) if comment.body == report => Ok(()),
            Some(comment) => self.edit_comment(&comment.url, &report),
            None => self.post_comment(task, &report),
        }
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        let question = TicketState::Question.to_string();
        let mut rows: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            &question,
            "--state",
            "open",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,body,labels,parent",
        ])?;
        rows.retain(|row| !row.has_label(TASK_LABEL));
        rows.sort_by_key(|row| row.number);
        Ok(rows.iter().map(|row| row.ticket(self.gh.repo())).collect())
    }

    /// Lists the `question` issues, open and closed, whose body says `Raised on #<task>`,
    /// oldest first, each with the text of its ruling comment when it has one.
    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        let question = TicketState::Question.to_string();
        let mut rows: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            &question,
            "--state",
            "all",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,body,labels,parent,state,comments",
        ])?;
        rows.retain(|row| {
            !row.has_label(TASK_LABEL) && NewQuestion::raised_on(&row.body) == Some(task)
        });
        rows.sort_by_key(|row| row.number);
        let raised = rows
            .iter()
            .map(|row| {
                let ruling = row
                    .comments
                    .iter()
                    .find_map(|comment| unmarked(RULING_MARKER, &comment.body))
                    .map(|text| text.trim().to_owned());
                (row.ticket(self.gh.repo()), ruling)
            })
            .collect();
        Ok(raised)
    }

    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        let title = question.title();
        let body = question.body();
        let question_label = TicketState::Question.to_string();
        let mut args = vec![
            "issue",
            "create",
            "--title",
            title.as_str(),
            "--body",
            body.as_str(),
            "--label",
            question_label.as_str(),
        ];
        if question.door {
            args.extend(["--label", DOOR_LABEL]);
        }
        let stdout = self.gh.text(&args)?;
        number_from_url(&stdout).map(TicketId::from)
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        let row = self.issue(question)?;
        let recorded = row
            .comments
            .iter()
            .find_map(|comment| unmarked(RULING_MARKER, &comment.body));
        match recorded {
            Some(existing) if existing.trim() != ruling.trim() => {
                let different_ruling_error =
                    PortError::Conflict(format!("{question} already has a different ruling"));
                return Err(different_ruling_error);
            }
            Some(_) => {}
            None => self.post_comment(question, &marked(RULING_MARKER, ruling))?,
        }
        if row.state == OPEN {
            let number = question.get().to_string();
            self.gh.text(&["issue", "close", &number]).map(drop)?;
        }
        self.return_waiting_task(&row)
    }
}
