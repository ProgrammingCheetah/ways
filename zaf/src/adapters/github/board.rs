//! The board on GitHub Issues: board kind `github-issues`.

use std::time::SystemTime;

use super::GhCommand;
use super::command::number_from_url;
use super::notes::{LeaseNote, RULING_MARKER, marked, report_marker, unmarked};
use super::rows::{ISSUE_FIELDS, IssueRow, NumberRow};
use crate::domain::{Instance, IssueNumber, TaskLabel};
use crate::ports::{Board, ClaimOutcome, ClaimRecord, NewQuestion, PortError, Ticket};

/// The label every task carries.
const TASK_LABEL: &str = "task";
/// The label a question raised as a one-way decision carries beside `question`.
const DOOR_LABEL: &str = "door";
/// The most issues one list call reads.
const LIST_LIMIT: &str = "1000";
/// The state gh prints for an open issue.
const OPEN: &str = "OPEN";

/// A board kept as the issues of one GitHub repository.
///
/// A task is an issue labelled `task`, its epic the issue it is a sub-issue of;
/// its state is one [`TaskLabel`]. A claim is the `claimed` label plus a
/// `claimed by …` comment; a heartbeat is a new `heartbeat …` comment.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct GithubBoard {
    gh: GhCommand,
}

impl GithubBoard {
    fn issue(&self, number: IssueNumber) -> Result<IssueRow, PortError> {
        let number = number.get().to_string();
        self.gh
            .json(&["issue", "view", &number, "--json", ISSUE_FIELDS])
    }

    fn edit_labels(
        &self,
        number: IssueNumber,
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

    fn post_comment(&self, number: IssueNumber, body: &str) -> Result<(), PortError> {
        let number = number.get().to_string();
        self.gh
            .text(&["issue", "comment", &number, "--body", body])
            .map(drop)
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

impl IssueRow {
    fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|label| label.name == name)
    }

    fn task_labels(&self) -> impl Iterator<Item = TaskLabel> + '_ {
        self.labels
            .iter()
            .filter_map(|label| label.name.parse::<TaskLabel>().ok())
    }

    fn ticket(&self) -> Ticket {
        Ticket {
            number: IssueNumber::from(self.number),
            epic: self.parent.map(|parent| IssueNumber::from(parent.number)),
            title: self.title.clone(),
            body: self.body.clone(),
            label: self.task_labels().next(),
        }
    }

    /// The live claim: the latest claim comment, while the issue is labelled
    /// `claimed`, touched by the holder's later heartbeats.
    fn claim(&self) -> Option<ClaimRecord> {
        if !self.has_label(&TaskLabel::Claimed.to_string()) {
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
            task: IssueNumber::from(self.number),
            instance: instance.clone(),
            heartbeat,
        };
        Some(record)
    }

    /// The claim, which must belong to `instance`.
    fn claim_by(&self, instance: &Instance) -> Result<ClaimRecord, PortError> {
        let task = IssueNumber::from(self.number);
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
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        let ready = TaskLabel::Ready.to_string();
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
        Ok(rows.iter().map(IssueRow::ticket).collect())
    }

    fn task(&self, number: IssueNumber) -> Result<Ticket, PortError> {
        Ok(self.issue(number)?.ticket())
    }

    fn claim(&self, task: IssueNumber, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        let row = self.issue(task)?;
        if row.has_label(&TaskLabel::Claimed.to_string()) {
            return row.claim_by(instance).map(|_| ClaimOutcome::AlreadyHeld);
        }
        if !row.has_label(&TaskLabel::Ready.to_string()) {
            return Err(PortError::Rejected(format!("{task} is not ready")));
        }
        let note = LeaseNote::Claim {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())?;
        let claimed = TaskLabel::Claimed.to_string();
        let ready = TaskLabel::Ready.to_string();
        self.edit_labels(task, &[&claimed], &[&ready])?;
        Ok(ClaimOutcome::Claimed)
    }

    fn heartbeat(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError> {
        self.issue(task)?.claim_by(instance)?;
        let note = LeaseNote::Heartbeat {
            instance: instance.clone(),
            at: SystemTime::now(),
        };
        self.post_comment(task, &note.to_string())
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        let claimed = TaskLabel::Claimed.to_string();
        let mut numbers: Vec<NumberRow> = self.gh.json(&[
            "issue", "list", "--label", &claimed, "--state", "open", "--limit", LIST_LIMIT,
            "--json", "number",
        ])?;
        numbers.sort_by_key(|row| row.number);
        let mut claims = Vec::new();
        for number in numbers {
            let row = self.issue(IssueNumber::from(number.number))?;
            claims.extend(row.claim());
        }
        Ok(claims)
    }

    fn release(&self, task: IssueNumber) -> Result<(), PortError> {
        let claimed = TaskLabel::Claimed.to_string();
        if !self.issue(task)?.has_label(&claimed) {
            return Ok(());
        }
        let ready = TaskLabel::Ready.to_string();
        self.edit_labels(task, &[&ready], &[&claimed])
    }

    fn label(&self, task: IssueNumber, label: TaskLabel) -> Result<(), PortError> {
        let row = self.issue(task)?;
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

    fn comment(&self, issue: IssueNumber, body: &str) -> Result<(), PortError> {
        self.post_comment(issue, body)
    }

    fn report(&self, task: IssueNumber, instance: &Instance, body: &str) -> Result<(), PortError> {
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

    fn open_question(&self, question: &NewQuestion) -> Result<IssueNumber, PortError> {
        let body = format!("Raised on {}.", question.on);
        let question_label = TaskLabel::Question.to_string();
        let mut args = vec![
            "issue",
            "create",
            "--title",
            question.text.as_str(),
            "--body",
            body.as_str(),
            "--label",
            question_label.as_str(),
        ];
        if question.door {
            args.extend(["--label", DOOR_LABEL]);
        }
        let stdout = self.gh.text(&args)?;
        number_from_url(&stdout).map(IssueNumber::from)
    }

    fn record_ruling(&self, question: IssueNumber, ruling: &str) -> Result<(), PortError> {
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
        if row.state != OPEN {
            return Ok(());
        }
        let number = question.get().to_string();
        self.gh.text(&["issue", "close", &number]).map(drop)
    }
}
