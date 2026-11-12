//! The labels the Forgejo board reads and writes, and the repository's label list, which the
//! board adds a label to the first time it needs one.
//!
//! A task's state is an exclusive scoped label, `state/<state>`, so Forgejo itself keeps one
//! state on an issue; a ticket's type is one too, `type/<Type>`, since Forgejo has no issue
//! types. An issue closed as not planned carries `not planned` (or `duplicate`), since
//! Forgejo keeps no close reason. The kind labels (`task`, `epic`, `question`, `door`,
//! `group:<name>`) are the GitHub issues board's.

use std::collections::BTreeSet;

use serde_json::json;

use super::api::{ForgejoApi, Method};
use super::rows::LabelRow;
use crate::adapters::issues::{DOOR_LABEL, GROUP_COLOR, GROUP_PREFIX, QUESTION_LABEL, TASK_LABEL};
use crate::domain::{DEFAULT_TASK_TYPE, GroupName, TicketState, TicketType};
use crate::ports::PortError;

/// The scope of the state labels: `state/ready`, `state/claimed`, ….
const STATE_SCOPE: &str = "state/";

/// The scope of the type labels: `type/Task`, `type/Bug`, ….
const TYPE_SCOPE: &str = "type/";

/// The label an issue closed as not planned carries.
pub const NOT_PLANNED_LABEL: &str = "not planned";

/// The label an issue closed as a duplicate carries.
pub const DUPLICATE_LABEL: &str = "duplicate";

/// The colour a type label is created in.
const TYPE_COLOR: &str = "c5def5";

/// The label that holds `state`: `state/<state>`.
pub fn state_label(state: TicketState) -> String {
    format!("{STATE_SCOPE}{state}")
}

/// The label that holds the type `kind`: `type/<kind>`.
pub fn type_label(kind: &TicketType) -> String {
    format!("{TYPE_SCOPE}{kind}")
}

/// The states among `labels`, in label order.
pub(super) fn states<'a>(labels: impl IntoIterator<Item = &'a str>) -> Vec<TicketState> {
    labels
        .into_iter()
        .filter_map(|label| label.strip_prefix(STATE_SCOPE)?.parse().ok())
        .collect()
}

/// The type in the first `type/<Type>` label among `labels`.
pub(super) fn kind<'a>(labels: impl IntoIterator<Item = &'a str>) -> Option<TicketType> {
    labels
        .into_iter()
        .find_map(|label| label.strip_prefix(TYPE_SCOPE))
        .filter(|kind| !kind.is_empty())
        .map(TicketType::from)
}

/// Whether `labels` say the issue was closed as not planned or as a duplicate.
pub(super) fn dropped<'a>(labels: impl IntoIterator<Item = &'a str>) -> bool {
    labels
        .into_iter()
        .any(|label| label == NOT_PLANNED_LABEL || label == DUPLICATE_LABEL)
}

/// A label as the board creates it when the repository lacks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LabelSpec {
    /// Its name.
    pub name: String,
    /// Its colour, six hex digits.
    color: &'static str,
    /// What a person reads beside it.
    description: &'static str,
    /// Whether it is exclusive within its scope.
    exclusive: bool,
}

impl LabelSpec {
    fn kind(name: &str, color: &'static str, description: &'static str) -> Self {
        Self {
            name: name.to_owned(),
            color,
            description,
            exclusive: false,
        }
    }

    /// `task`.
    pub fn task() -> Self {
        Self::kind(TASK_LABEL, "0075ca", "A zaf task")
    }

    /// `question`.
    pub fn question() -> Self {
        Self::kind(QUESTION_LABEL, "d876e3", "A question for the Engineer")
    }

    /// `door`.
    pub fn door() -> Self {
        Self::kind(DOOR_LABEL, "b60205", "A one-way decision")
    }

    /// `group:<name>`.
    pub fn group(group: &GroupName) -> Self {
        Self {
            name: format!("{GROUP_PREFIX}{group}"),
            color: GROUP_COLOR,
            description: "",
            exclusive: false,
        }
    }

    /// `state/<state>`, exclusive.
    pub fn state(state: TicketState) -> Self {
        let (color, description) = match state {
            TicketState::Ready => ("0e8a16", "Open for a claim"),
            TicketState::Claimed => ("fbca04", "Held by an instance under a lease"),
            TicketState::Blocked => ("bfbfbf", "Waiting on another task"),
            TicketState::Question => ("e99695", "Waiting on the Engineer"),
            TicketState::Done => ("5319e7", "Merged"),
        };
        Self {
            name: state_label(state),
            color,
            description,
            exclusive: true,
        }
    }

    /// `type/<Type>`, exclusive.
    pub fn of_type(kind: &TicketType) -> Self {
        Self {
            name: type_label(kind),
            color: TYPE_COLOR,
            description: "A zaf ticket type",
            exclusive: true,
        }
    }
}

/// The repository's labels as read once, with those the board created since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoLabels {
    rows: Vec<LabelRow>,
}

impl RepoLabels {
    /// Reads every label of the repository.
    pub fn read(api: &ForgejoApi) -> Result<Self, PortError> {
        Ok(Self {
            rows: api.list("labels")?,
        })
    }

    /// The id of the label named `name`, the oldest when two share the name.
    pub fn id(&self, name: &str) -> Option<u64> {
        self.rows
            .iter()
            .filter(|row| row.name == name)
            .map(|row| row.id)
            .min()
    }

    /// The types the repository defines: one per `type/<Type>` label, with the default type
    /// `Task`, whose label the board creates the first time it files one. By name.
    pub fn types(&self) -> Vec<TicketType> {
        let mut names: BTreeSet<String> = self
            .rows
            .iter()
            .filter_map(|row| row.name.strip_prefix(TYPE_SCOPE))
            .filter(|kind| !kind.is_empty())
            .map(str::to_owned)
            .collect();
        names.insert(DEFAULT_TASK_TYPE.to_owned());
        names.into_iter().map(TicketType::from).collect()
    }

    /// The id of the label `spec` names, creating it when the repository lacks it.
    ///
    /// Forgejo lets two labels share a name, so after creating one the list is read again:
    /// when an older label of that name turned up meanwhile (another writer created it at
    /// the same moment), the one just created is deleted and the older one used.
    pub fn ensure(&mut self, api: &ForgejoApi, spec: &LabelSpec) -> Result<u64, PortError> {
        if let Some(id) = self.id(&spec.name) {
            return Ok(id);
        }
        let body = json!({
            "name": spec.name,
            "color": spec.color,
            "description": spec.description,
            "exclusive": spec.exclusive,
        });
        let created: LabelRow = api.send(Method::Post, "labels", Some(&body))?;
        *self = Self::read(api)?;
        let oldest = self.id(&spec.name).unwrap_or(created.id);
        if oldest != created.id {
            api.send::<()>(Method::Delete, &format!("labels/{}", created.id), None)?;
            self.rows.retain(|row| row.id != created.id);
        } else if !self.rows.iter().any(|row| row.id == created.id) {
            self.rows.push(created);
        }
        Ok(oldest)
    }
}
