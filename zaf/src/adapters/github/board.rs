//! The board on GitHub: board kinds `github-issues` and `github-projects`.
//!
//! Both keep tickets as the issues of one repository, with their comments, claims, questions,
//! sub-issues and kind labels. They differ only in where a task's state lives, which a
//! [`StateStore`] decides: labels on the issue, or a field of a GitHub Project.

use std::collections::BTreeMap;
use std::time::SystemTime;

use super::GhCommand;
use super::command::{GraphqlValue, number_from_url};
use super::rows::{
    ISSUE_FIELDS, IssueRow, NameRow, STANDING_FIELDS, StandingRow, TICKET_LIST_FIELDS,
};
use super::state::{LabelState, ProjectRef, ProjectState, StateStore};
use crate::adapters::issues::{
    DOOR_LABEL, EPIC_LABEL, GROUP_COLOR, GROUP_PREFIX, QUESTION_LABEL, TASK_LABEL, can_wait, group,
    keeps_state_on_release, waits_on_ruling,
};
use crate::adapters::notes::{
    EpicNotes, LeaseNote, RULING_MARKER, SIGNATURE_TEXT, held_by, live_claim, marked,
    recorded_ruling, release_line, report_marker, signature_marker, unmarked,
};
use crate::domain::{
    Epic, EpicTask, Instance, NewTask, Signature, Standing, Ticket, TicketId, TicketState,
    TicketType,
};
use crate::manifest::RepoName;
use crate::ports::{Board, ClaimOutcome, ClaimRecord, IssueTypes, NewQuestion, PortError};

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

/// The issue types a repository defines.
const ISSUE_TYPES: &str = "query($owner: String!, $name: String!) { \
    repository(owner: $owner, name: $name) { issueTypes(first: 50) { nodes { name } } } }";

/// What [`ISSUE_TYPES`] returns.
#[derive(Debug, serde::Deserialize)]
struct TypesData {
    repository: Option<TypesRepository>,
}

/// The repository of a [`TypesData`]; an owner without issue types returns none.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypesRepository {
    issue_types: Option<TypeNodes>,
}

/// The nodes of the `issueTypes` connection.
#[derive(Debug, serde::Deserialize)]
struct TypeNodes {
    nodes: Vec<NameRow>,
}

/// A board kept as the issues of one GitHub repository.
///
/// A task is an issue labelled `task`, its epic the issue it is a sub-issue of;
/// its state is one [`TicketState`], kept where `S` keeps it. A claim is the `claimed`
/// state plus a `claimed by …` comment; a heartbeat is a new `heartbeat …` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubBoard<S = LabelState> {
    gh: GhCommand,
    store: S,
}

/// The board on GitHub Projects: the issues of one repository, each task's state in a
/// single-select field of a Project.
pub type GithubProjectBoard = GithubBoard<ProjectState>;

impl From<GhCommand> for GithubBoard<LabelState> {
    /// The issues board: state as labels.
    fn from(gh: GhCommand) -> Self {
        Self {
            gh,
            store: LabelState,
        }
    }
}

impl GithubBoard<ProjectState> {
    /// The Projects board: the issues of `gh`'s repository, their states in `project`.
    pub fn on_project(gh: GhCommand, project: ProjectRef) -> Self {
        Self {
            gh,
            store: ProjectState::new(project),
        }
    }
}

impl<S: StateStore> GithubBoard<S> {
    /// The states the issue read as `row` has.
    fn states(&self, row: &IssueRow) -> Result<Vec<TicketState>, PortError> {
        self.store.states(&self.gh, row.number, &row.labels)
    }

    /// The state of each issue in `rows`, read at once.
    fn states_of(&self, rows: &[IssueRow]) -> Result<BTreeMap<u64, TicketState>, PortError> {
        let issues: Vec<(u64, &[NameRow])> = rows
            .iter()
            .map(|row| (row.number, row.labels.as_slice()))
            .collect();
        self.store.states_of(&self.gh, &issues)
    }

    /// Adds `add` and drops `remove` on the issue read as `row`.
    fn change(
        &self,
        row: &IssueRow,
        add: Option<TicketState>,
        remove: &[TicketState],
    ) -> Result<(), PortError> {
        self.store
            .change(&self.gh, row.number, &row.labels, add, remove)
    }

    /// The tickets of `rows`, each with its state.
    fn tickets(&self, rows: &[IssueRow]) -> Result<Vec<Ticket>, PortError> {
        let states = self.states_of(rows)?;
        Ok(rows
            .iter()
            .map(|row| row.ticket(self.gh.repo(), states.get(&row.number).copied()))
            .collect())
    }

    fn issue(&self, number: TicketId) -> Result<IssueRow, PortError> {
        let number = number.get().to_string();
        self.gh
            .json(&["issue", "view", &number, "--json", ISSUE_FIELDS])
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
        let states = self.states(&row)?;
        if !waits_on_ruling(row.has_label(EPIC_LABEL), &states) {
            return Ok(None);
        }
        self.change(&row, Some(TicketState::Ready), &[TicketState::Question])?;
        Ok(Some(task))
    }

    /// Creates the label when the repository has none by that name; one already there is
    /// left as it is.
    fn ensure_label(&self, name: &str) -> Result<(), PortError> {
        match self
            .gh
            .text(&["label", "create", name, "--color", GROUP_COLOR])
        {
            Ok(_) => Ok(()),
            Err(PortError::Backend(message)) if message.contains("already exists") => Ok(()),
            Err(error) => Err(error),
        }
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

/// Where an issue stands from its state, close reason and whether it is in the `done`
/// state (see [`Standing::of_issue`]): closed as not planned or as a duplicate is dropped.
fn standing_of(state: &str, reason: Option<&str>, done: bool) -> Standing {
    let dropped = matches!(reason, Some(NOT_PLANNED | DUPLICATE));
    Standing::of_issue(state == OPEN, dropped, done)
}

impl IssueRow {
    fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|label| label.name == name)
    }

    /// The name in the first `group:<name>` label.
    fn group(&self) -> Option<String> {
        group(self.labels.iter().map(|label| label.name.as_str()))
    }

    /// The ticket in `state`, its link the issue's page on `repo`.
    fn ticket(&self, repo: &RepoName, state: Option<TicketState>) -> Ticket {
        Ticket {
            id: TicketId::from(self.number),
            epic: self.parent.map(|parent| TicketId::from(parent.number)),
            title: self.title.clone().into(),
            body: self.body.clone().into(),
            state,
            url: format!("https://github.com/{repo}/issues/{}", self.number).into(),
            kind: self.kind(),
        }
    }

    /// The name of the issue's type; none for an issue without one.
    fn kind(&self) -> Option<TicketType> {
        self.issue_type
            .as_ref()
            .map(|issue_type| TicketType::from(issue_type.name.as_str()))
    }

    /// The live claim, while the issue is `claimed`: the latest claim comment, touched by
    /// the holder's later heartbeats.
    fn claim(&self, claimed: bool) -> Option<ClaimRecord> {
        let comments = self.comments.iter().map(|comment| comment.body.as_str());
        live_claim(TicketId::from(self.number), claimed, comments)
    }

    /// The claim, while the issue is `claimed`, which must belong to `instance`.
    fn claim_by(&self, claimed: bool, instance: &Instance) -> Result<ClaimRecord, PortError> {
        held_by(TicketId::from(self.number), self.claim(claimed), instance)
    }
}

impl<S: StateStore> Board for GithubBoard<S> {
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
            "number,title,labels,parent,state,stateReason,issueType",
        ])?;
        tasks.sort_by_key(|row| row.number);
        let states = self.states_of(&tasks)?;
        let epics = epics
            .iter()
            .map(|epic| {
                let notes = EpicNotes::read(epic.comments.iter().map(|comment| {
                    let posted = comment
                        .created_at
                        .as_deref()
                        .and_then(|at| humantime::parse_rfc3339(at).ok());
                    (comment.body.as_str(), posted)
                }));
                Epic {
                    id: TicketId::from(epic.number),
                    title: epic.title.clone().into(),
                    body: epic.body.clone().into(),
                    tasks: tasks
                        .iter()
                        .filter(|task| {
                            task.parent
                                .is_some_and(|parent| parent.number == epic.number)
                        })
                        .map(|task| {
                            let state = states.get(&task.number).copied();
                            EpicTask {
                                number: TicketId::from(task.number),
                                title: task.title.clone().into(),
                                state,
                                standing: standing_of(
                                    &task.state,
                                    task.state_reason.as_deref(),
                                    state == Some(TicketState::Done),
                                ),
                                group: task.group().map(Into::into),
                                kind: task.kind(),
                            }
                        })
                        .collect(),
                    signature: notes.signature,
                    planned_at: notes.planned_at,
                    challenged: notes.challenged,
                }
            })
            .collect();
        Ok(epics)
    }

    /// The issue types the repository defines, one GraphQL read.
    fn issue_types(&self) -> Result<IssueTypes, PortError> {
        let repo = self.gh.repo().as_ref();
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| PortError::Backend(format!("`{repo}` is not owner/name")))?;
        let data: TypesData = self.gh.graphql(
            ISSUE_TYPES,
            &[
                ("owner", GraphqlValue::Text(owner.to_owned())),
                ("name", GraphqlValue::Text(name.to_owned())),
            ],
        )?;
        let repository = data
            .repository
            .ok_or_else(|| PortError::NotFound(format!("no repository {repo}")))?;
        Ok(IssueTypes {
            repo: repo.to_owned(),
            names: repository
                .issue_types
                .map(|types| types.nodes)
                .unwrap_or_default()
                .into_iter()
                .map(|row| TicketType::from(row.name))
                .collect(),
        })
    }

    /// Opens the issue with the task's type and the label `task` (and `group:<name>`, created
    /// when the repository lacks it), files it as a sub-issue of its epic, then gives it its
    /// state where the board keeps states.
    fn file_task(&self, task: &NewTask) -> Result<TicketId, PortError> {
        let mut labels = vec![TASK_LABEL.to_owned()];
        if let Some(group) = &task.group {
            let label = format!("{GROUP_PREFIX}{group}");
            self.ensure_label(&label)?;
            labels.push(label);
        }
        let joined = labels.join(",");
        let stdout = self.gh.text(&[
            "issue",
            "create",
            "--title",
            task.title.as_str(),
            "--body",
            task.body.as_str(),
            "--label",
            &joined,
            "--type",
            task.kind.as_str(),
        ])?;
        let number = number_from_url(&stdout)?;
        let database_id = self
            .gh
            .api_text(&format!("issues/{number}"), &["--jq", ".id"])?;
        let field = format!("sub_issue_id={}", database_id.trim());
        self.gh.api_text(
            &format!("issues/{}/sub_issues", task.epic.get()),
            &["--method", "POST", "-F", &field],
        )?;
        let rows: Vec<NameRow> = labels.into_iter().map(|name| NameRow { name }).collect();
        self.store
            .change(&self.gh, number, &rows, Some(task.state), &[])?;
        Ok(TicketId::from(number))
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
        if row.url.contains(PULL_PATH) {
            return Ok(Standing::Missing);
        }
        let states = self.store.states(&self.gh, row.number, &row.labels)?;
        Ok(standing_of(
            &row.state,
            row.state_reason.as_deref(),
            states.contains(&TicketState::Done),
        ))
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        let mut rows = self.store.in_state(
            &self.gh,
            TicketState::Ready,
            Some(TASK_LABEL),
            TICKET_LIST_FIELDS,
        )?;
        rows.sort_by_key(|row| row.number);
        Ok(rows
            .iter()
            .map(|row| row.ticket(self.gh.repo(), Some(TicketState::Ready)))
            .collect())
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        let row = self.issue(number)?;
        let state = self.states(&row)?.first().copied();
        Ok(row.ticket(self.gh.repo(), state))
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        let row = self.issue(task)?;
        let states = self.states(&row)?;
        if states.contains(&TicketState::Claimed) {
            return row
                .claim_by(true, instance)
                .map(|_| ClaimOutcome::AlreadyHeld);
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
        let claimed = self.states(&row)?.contains(&TicketState::Claimed);
        row.claim_by(claimed, instance)?;
        let note = LeaseNote::Heartbeat {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        let mut numbers: Vec<u64> = self
            .store
            .in_state(&self.gh, TicketState::Claimed, None, "number")?
            .iter()
            .map(|row| row.number)
            .collect();
        numbers.sort_unstable();
        let mut claims = Vec::new();
        for number in numbers {
            let row = self.issue(TicketId::from(number))?;
            let claimed = self.states(&row)?.contains(&TicketState::Claimed);
            claims.extend(row.claim(claimed));
        }
        Ok(claims)
    }

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        let row = self.issue(task)?;
        let states = self.states(&row)?;
        let holder = row
            .claim(states.contains(&TicketState::Claimed))
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
        let states = self.states(&row)?;
        if label == TicketState::Question && !can_wait(row.has_label(EPIC_LABEL), &states) {
            return Ok(());
        }
        let others: Vec<TicketState> = states
            .into_iter()
            .filter(|present| *present != label)
            .collect();
        self.change(&row, Some(label), &others)
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
        let mut rows: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            QUESTION_LABEL,
            "--state",
            "open",
            "--limit",
            LIST_LIMIT,
            "--json",
            TICKET_LIST_FIELDS,
        ])?;
        rows.retain(|row| !row.has_label(TASK_LABEL));
        rows.sort_by_key(|row| row.number);
        self.tickets(&rows)
    }

    /// Lists the `question` issues, open and closed, whose body says `Raised on #<task>`,
    /// oldest first, each with the text of its ruling comment when it has one.
    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        let mut rows: Vec<IssueRow> = self.gh.json(&[
            "issue",
            "list",
            "--label",
            QUESTION_LABEL,
            "--state",
            "all",
            "--limit",
            LIST_LIMIT,
            "--json",
            "number,title,body,labels,parent,state,comments,issueType",
        ])?;
        rows.retain(|row| {
            !row.has_label(TASK_LABEL) && NewQuestion::raised_on(&row.body) == Some(task)
        });
        rows.sort_by_key(|row| row.number);
        let tickets = self.tickets(&rows)?;
        let raised = rows
            .iter()
            .zip(tickets)
            .map(|(row, ticket)| {
                let ruling =
                    recorded_ruling(row.comments.iter().map(|comment| comment.body.as_str()))
                        .map(|text| text.trim().to_owned());
                (ticket, ruling)
            })
            .collect();
        Ok(raised)
    }

    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        let title = question.title();
        let body = question.body();
        let mut args = vec![
            "issue",
            "create",
            "--title",
            title.as_str(),
            "--body",
            body.as_str(),
            "--label",
            QUESTION_LABEL,
        ];
        if question.door {
            args.extend(["--label", DOOR_LABEL]);
        }
        let stdout = self.gh.text(&args)?;
        let number = number_from_url(&stdout)?;
        // The question label holds the state on the issues board; a Project holds it apart.
        let labels = [NameRow {
            name: QUESTION_LABEL.to_owned(),
        }];
        self.store
            .change(&self.gh, number, &labels, Some(TicketState::Question), &[])?;
        Ok(TicketId::from(number))
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        let row = self.issue(question)?;
        let recorded = recorded_ruling(row.comments.iter().map(|comment| comment.body.as_str()));
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
