//! Where the GitHub board keeps each task's state: in labels on the issue (board kind
//! `github-issues`), or in a single-select field of a GitHub Project the issues are on (board
//! kind `github-projects`). Everything else (comments, claims, questions, sub-issues, the
//! `task`, `epic`, `question` and `door` kind labels) lives on the issues either way.
//!
//! A [`StateStore`] answers three things: the states an issue has, the open issues in one
//! state, and a change of state. Labels may hold more than one state label at once; a Project
//! field holds one value, so it reads as at most one.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

use super::command::{GhCommand, GraphqlValue};
use super::rows::{IssueRow, NameRow, NumberRow};
use crate::domain::TicketState;
use crate::ports::PortError;

/// The most issues one list call reads.
const LIST_LIMIT: &str = "1000";

/// The most Project items one GraphQL page reads; GitHub allows 100.
const PAGE: u64 = 100;

/// Where the board keeps each task's state.
pub trait StateStore: fmt::Debug + Send + Sync {
    /// The states the issue `issue` has, its labels as read.
    fn states(
        &self,
        gh: &GhCommand,
        issue: u64,
        labels: &[NameRow],
    ) -> Result<Vec<TicketState>, PortError>;

    /// The state of every issue in `issues` (number and labels), read at once.
    fn states_of(
        &self,
        gh: &GhCommand,
        issues: &[(u64, &[NameRow])],
    ) -> Result<BTreeMap<u64, TicketState>, PortError>;

    /// The open issues in `state` that carry the label `kind` when one is named, read with
    /// at least the list fields `fields`.
    fn in_state(
        &self,
        gh: &GhCommand,
        state: TicketState,
        kind: Option<&str>,
        fields: &str,
    ) -> Result<Vec<IssueRow>, PortError>;

    /// Adds `add` to the issue and drops every state in `remove`; a change that would leave the
    /// issue as it is makes no call.
    fn change(
        &self,
        gh: &GhCommand,
        issue: u64,
        labels: &[NameRow],
        add: Option<TicketState>,
        remove: &[TicketState],
    ) -> Result<(), PortError>;
}

/// The states in `labels`, in label order.
fn label_states(labels: &[NameRow]) -> Vec<TicketState> {
    labels
        .iter()
        .filter_map(|label| label.name.parse::<TicketState>().ok())
        .collect()
}

/// State as labels: `ready`, `claimed`, `blocked`, `question`, `done` on the issue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LabelState;

impl StateStore for LabelState {
    fn states(
        &self,
        _: &GhCommand,
        _: u64,
        labels: &[NameRow],
    ) -> Result<Vec<TicketState>, PortError> {
        Ok(label_states(labels))
    }

    fn states_of(
        &self,
        _: &GhCommand,
        issues: &[(u64, &[NameRow])],
    ) -> Result<BTreeMap<u64, TicketState>, PortError> {
        Ok(issues
            .iter()
            .filter_map(|(number, labels)| {
                label_states(labels).first().map(|state| (*number, *state))
            })
            .collect())
    }

    fn in_state(
        &self,
        gh: &GhCommand,
        state: TicketState,
        kind: Option<&str>,
        fields: &str,
    ) -> Result<Vec<IssueRow>, PortError> {
        let label = state.to_string();
        let mut args = vec!["issue", "list"];
        if let Some(kind) = kind {
            args.extend(["--label", kind]);
        }
        args.extend([
            "--label", &label, "--state", "open", "--limit", LIST_LIMIT, "--json", fields,
        ]);
        gh.json(&args)
    }

    fn change(
        &self,
        gh: &GhCommand,
        issue: u64,
        labels: &[NameRow],
        add: Option<TicketState>,
        remove: &[TicketState],
    ) -> Result<(), PortError> {
        let present = label_states(labels);
        let added: Vec<String> = add
            .filter(|state| !present.contains(state))
            .map(|state| state.to_string())
            .into_iter()
            .collect();
        let removed: Vec<String> = remove
            .iter()
            .filter(|state| present.contains(state) && Some(**state) != add)
            .map(ToString::to_string)
            .collect();
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }
        let number = issue.to_string();
        let mut args = vec!["issue", "edit", number.as_str()];
        let added = added.join(",");
        let removed = removed.join(",");
        if !added.is_empty() {
            args.extend(["--add-label", added.as_str()]);
        }
        if !removed.is_empty() {
            args.extend(["--remove-label", removed.as_str()]);
        }
        gh.text(&args).map(drop)
    }
}

/// Which Project holds the states, and in which field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRef {
    /// The user or organization that owns the Project.
    pub owner: String,
    /// The Project's number in its owner's projects.
    pub number: u64,
    /// The single-select field holding each task's state.
    pub field: String,
}

/// The ids a write needs: the Project's, the field's, and one option per state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldIds {
    project: String,
    field: String,
    options: BTreeMap<TicketState, String>,
}

/// State as a single-select field of a GitHub Project: the field's option named for the
/// state, matched without regard to case. The field is created, with one option per state,
/// on the first write to a Project that lacks it.
#[derive(Debug)]
pub struct ProjectState {
    project: ProjectRef,
    ids: OnceLock<FieldIds>,
}

impl ProjectState {
    /// The store for `project`.
    pub fn new(project: ProjectRef) -> Self {
        Self {
            project,
            ids: OnceLock::new(),
        }
    }
}

impl PartialEq for ProjectState {
    fn eq(&self, other: &Self) -> bool {
        self.project == other.project
    }
}

impl Eq for ProjectState {}

impl Clone for ProjectState {
    fn clone(&self) -> Self {
        Self::new(self.project.clone())
    }
}

/// The state a field's option names, matched without regard to case.
fn state_named(name: &str) -> Option<TicketState> {
    name.to_lowercase().parse().ok()
}

const PROJECT_FIELDS: &str = "query($owner: String!, $number: Int!) {
  repositoryOwner(login: $owner) { ... on ProjectV2Owner { projectV2(number: $number) {
    id
    fields(first: 50) { nodes { ... on ProjectV2SingleSelectField { id name options { id name } } } }
  } } }
}";

const CREATE_FIELD: &str = "mutation($project: ID!, $name: String!) {
  createProjectV2Field(input: { projectId: $project, dataType: SINGLE_SELECT, name: $name,
    singleSelectOptions: [
      { name: \"Ready\", color: GREEN, description: \"Open for a claim\" },
      { name: \"Claimed\", color: YELLOW, description: \"Held by an instance under a lease\" },
      { name: \"Blocked\", color: GRAY, description: \"Waiting on another task\" },
      { name: \"Question\", color: ORANGE, description: \"Waiting on the Engineer\" },
      { name: \"Done\", color: PURPLE, description: \"Merged\" }
    ] }) {
    projectV2Field { ... on ProjectV2SingleSelectField { id name options { id name } } }
  }
}";

const ISSUE_ITEMS: &str = "query($owner: String!, $name: String!, $issue: Int!, $field: String!) {
  repository(owner: $owner, name: $name) { issue(number: $issue) {
    id
    projectItems(first: 50) { nodes {
      id
      project { number owner { ... on User { login } ... on Organization { login } } }
      fieldValueByName(name: $field) { ... on ProjectV2ItemFieldSingleSelectValue { name } }
    } }
  } }
}";

const PROJECT_ITEMS: &str =
    "query($owner: String!, $number: Int!, $field: String!, $page: Int!, $after: String) {
  repositoryOwner(login: $owner) { ... on ProjectV2Owner { projectV2(number: $number) {
    items(first: $page, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        fieldValueByName(name: $field) { ... on ProjectV2ItemFieldSingleSelectValue { name } }
        content { ... on Issue {
          number title body state
          labels(first: 100) { nodes { name } }
          parent { number }
          repository { nameWithOwner }
        } }
      }
    }
  } } }
}";

const ADD_ITEM: &str = "mutation($project: ID!, $content: ID!) {
  addProjectV2ItemById(input: { projectId: $project, contentId: $content }) { item { id } }
}";

const SET_VALUE: &str = "mutation($project: ID!, $item: ID!, $field: ID!, $option: String!) {
  updateProjectV2ItemFieldValue(input: { projectId: $project, itemId: $item, fieldId: $field,
    value: { singleSelectOptionId: $option } }) { projectV2Item { id } }
}";

const CLEAR_VALUE: &str = "mutation($project: ID!, $item: ID!, $field: ID!) {
  clearProjectV2ItemFieldValue(input: { projectId: $project, itemId: $item, fieldId: $field }) {
    projectV2Item { id }
  }
}";

#[derive(Deserialize)]
struct OwnerData<T> {
    #[serde(rename = "repositoryOwner")]
    owner: Option<OwnerProject<T>>,
}

#[derive(Deserialize)]
struct OwnerProject<T> {
    #[serde(rename = "projectV2")]
    project: Option<T>,
}

#[derive(Deserialize)]
struct FieldsProject {
    id: String,
    fields: Nodes<SelectField>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

/// A field as the fields query reads it; a field that is not single-select reads empty.
#[derive(Deserialize)]
struct SelectField {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    options: Vec<SelectOption>,
}

#[derive(Deserialize)]
struct SelectOption {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct CreatedField {
    #[serde(rename = "createProjectV2Field")]
    created: CreatedFieldPayload,
}

#[derive(Deserialize)]
struct CreatedFieldPayload {
    #[serde(rename = "projectV2Field")]
    field: SelectField,
}

#[derive(Deserialize)]
struct RepositoryData {
    repository: Option<RepositoryIssue>,
}

#[derive(Deserialize)]
struct RepositoryIssue {
    issue: Option<IssueItems>,
}

#[derive(Deserialize)]
struct IssueItems {
    id: String,
    #[serde(rename = "projectItems")]
    items: Nodes<IssueItem>,
}

#[derive(Deserialize)]
struct IssueItem {
    id: String,
    project: ItemProject,
    #[serde(rename = "fieldValueByName")]
    value: Option<OptionValue>,
}

#[derive(Deserialize)]
struct ItemProject {
    number: u64,
    #[serde(default)]
    owner: Option<Login>,
}

#[derive(Deserialize)]
struct Login {
    #[serde(default)]
    login: Option<String>,
}

#[derive(Deserialize)]
struct OptionValue {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ItemsProject {
    items: ItemsPage,
}

#[derive(Deserialize)]
struct ItemsPage {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<ProjectItem>,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct ProjectItem {
    #[serde(rename = "fieldValueByName")]
    value: Option<OptionValue>,
    /// Empty for a draft or a pull request: only issues are read.
    #[serde(default)]
    content: Option<ItemIssue>,
}

#[derive(Deserialize)]
struct ItemIssue {
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    labels: Option<Nodes<NameRow>>,
    #[serde(default)]
    parent: Option<NumberRow>,
    #[serde(default)]
    repository: Option<NameWithOwner>,
}

#[derive(Deserialize)]
struct NameWithOwner {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Deserialize)]
struct AddedItem {
    #[serde(rename = "addProjectV2ItemById")]
    added: AddedItemPayload,
}

#[derive(Deserialize)]
struct AddedItemPayload {
    item: ItemId,
}

#[derive(Deserialize)]
struct ItemId {
    id: String,
}

/// An issue as the issue-items query reads it: its node id, and its item on the board's
/// Project with the item's state, if it is on it.
struct Placement {
    content: String,
    item: Option<PlacedItem>,
}

/// An issue's item on the board's Project.
struct PlacedItem {
    id: String,
    state: Option<TicketState>,
}

/// An issue on the Project as one items page reads it.
struct OnProject {
    state: Option<TicketState>,
    row: IssueRow,
}

impl ProjectState {
    fn owner_vars(&self) -> Vec<(&'static str, GraphqlValue)> {
        vec![
            ("owner", GraphqlValue::Text(self.project.owner.clone())),
            ("number", GraphqlValue::Number(self.project.number)),
        ]
    }

    fn missing_project(&self) -> PortError {
        PortError::NotFound(format!(
            "no Project {} owned by {} that gh can read",
            self.project.number, self.project.owner
        ))
    }

    /// The Project's id and its field by the board's name, if it has one.
    fn read_field(&self, gh: &GhCommand) -> Result<(String, Option<SelectField>), PortError> {
        let data: OwnerData<FieldsProject> = gh.graphql(PROJECT_FIELDS, &self.owner_vars())?;
        let project = data
            .owner
            .and_then(|owner| owner.project)
            .ok_or_else(|| self.missing_project())?;
        let named = project
            .fields
            .nodes
            .into_iter()
            .find(|field| field.name.as_deref() == Some(self.project.field.as_str()));
        Ok((project.id, named))
    }

    /// The ids a write needs, read once per process; the field is created when the Project
    /// has none by its name. A creation that fails because another writer created the field
    /// first reads the field again and goes on with it.
    fn ids(&self, gh: &GhCommand) -> Result<&FieldIds, PortError> {
        if let Some(ids) = self.ids.get() {
            return Ok(ids);
        }
        let (project, named) = self.read_field(gh)?;
        let field = match named {
            Some(field) => field,
            None => {
                let created: Result<CreatedField, PortError> = gh.graphql(
                    CREATE_FIELD,
                    &[
                        ("project", GraphqlValue::Text(project.clone())),
                        ("name", GraphqlValue::Text(self.project.field.clone())),
                    ],
                );
                match created {
                    Ok(created) => created.created.field,
                    Err(error) => self.read_field(gh)?.1.ok_or(error)?,
                }
            }
        };
        let field_id = field.id.ok_or_else(|| {
            PortError::Rejected(format!(
                "the Project's `{}` field is not a single-select field",
                self.project.field
            ))
        })?;
        let options: BTreeMap<TicketState, String> = field
            .options
            .into_iter()
            .filter_map(|option| state_named(&option.name).map(|state| (state, option.id)))
            .collect();
        Ok(self.ids.get_or_init(|| FieldIds {
            project,
            field: field_id,
            options,
        }))
    }

    /// The issue's node id and its item on this Project, with the item's state, if it is on it.
    fn item_of(&self, gh: &GhCommand, issue: u64) -> Result<Placement, PortError> {
        let repo = gh.repo().as_ref();
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| PortError::Backend(format!("`{repo}` is not owner/name")))?;
        let data: RepositoryData = gh.graphql(
            ISSUE_ITEMS,
            &[
                ("owner", GraphqlValue::Text(owner.to_owned())),
                ("name", GraphqlValue::Text(name.to_owned())),
                ("issue", GraphqlValue::Number(issue)),
                ("field", GraphqlValue::Text(self.project.field.clone())),
            ],
        )?;
        let found = data
            .repository
            .and_then(|repository| repository.issue)
            .ok_or_else(|| PortError::NotFound(format!("no issue #{issue} in {repo}")))?;
        let item = found.items.nodes.into_iter().find(|item| {
            item.project.number == self.project.number
                && item
                    .project
                    .owner
                    .as_ref()
                    .and_then(|owner| owner.login.as_deref())
                    .is_some_and(|login| login.eq_ignore_ascii_case(&self.project.owner))
        });
        let item = item.map(|item| PlacedItem {
            state: item
                .value
                .and_then(|value| value.name)
                .and_then(|name| state_named(&name)),
            id: item.id,
        });
        Ok(Placement {
            content: found.id,
            item,
        })
    }

    /// Every issue of the board's repository on the Project, with its state, page by page.
    fn items(&self, gh: &GhCommand) -> Result<Vec<OnProject>, PortError> {
        let repo = gh.repo().as_ref().to_owned();
        let mut found = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut vars = self.owner_vars();
            vars.push(("field", GraphqlValue::Text(self.project.field.clone())));
            vars.push(("page", GraphqlValue::Number(PAGE)));
            if let Some(cursor) = &after {
                vars.push(("after", GraphqlValue::Text(cursor.clone())));
            }
            let data: OwnerData<ItemsProject> = gh.graphql(PROJECT_ITEMS, &vars)?;
            let page = data
                .owner
                .and_then(|owner| owner.project)
                .ok_or_else(|| self.missing_project())?
                .items;
            for item in page.nodes {
                let Some(issue) = item.content else {
                    continue;
                };
                let ours = issue.repository.as_ref().is_some_and(|repository| {
                    repository.name_with_owner.eq_ignore_ascii_case(&repo)
                });
                let Some(number) = issue.number.filter(|_| ours) else {
                    continue;
                };
                let state = item
                    .value
                    .and_then(|value| value.name)
                    .and_then(|name| state_named(&name));
                let row = IssueRow {
                    number,
                    title: issue.title,
                    body: issue.body,
                    labels: issue.labels.map(|labels| labels.nodes).unwrap_or_default(),
                    parent: issue.parent,
                    state: issue.state,
                    state_reason: None,
                    comments: Vec::new(),
                };
                found.push(OnProject { state, row });
            }
            if !page.page_info.has_next_page {
                break;
            }
            after = page.page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }
        found.sort_by_key(|item| item.row.number);
        Ok(found)
    }
}

/// The state gh prints for an open issue.
const OPEN: &str = "OPEN";

impl StateStore for ProjectState {
    fn states(
        &self,
        gh: &GhCommand,
        issue: u64,
        _: &[NameRow],
    ) -> Result<Vec<TicketState>, PortError> {
        let placement = self.item_of(gh, issue)?;
        Ok(placement
            .item
            .and_then(|item| item.state)
            .into_iter()
            .collect())
    }

    fn states_of(
        &self,
        gh: &GhCommand,
        issues: &[(u64, &[NameRow])],
    ) -> Result<BTreeMap<u64, TicketState>, PortError> {
        if issues.is_empty() {
            return Ok(BTreeMap::new());
        }
        let wanted: Vec<u64> = issues.iter().map(|(number, _)| *number).collect();
        Ok(self
            .items(gh)?
            .into_iter()
            .filter(|item| wanted.contains(&item.row.number))
            .filter_map(|item| item.state.map(|state| (item.row.number, state)))
            .collect())
    }

    fn in_state(
        &self,
        gh: &GhCommand,
        state: TicketState,
        kind: Option<&str>,
        _: &str,
    ) -> Result<Vec<IssueRow>, PortError> {
        Ok(self
            .items(gh)?
            .into_iter()
            .filter(|item| item.state == Some(state) && item.row.state == OPEN)
            .filter(|item| {
                kind.is_none_or(|kind| item.row.labels.iter().any(|label| label.name == kind))
            })
            .map(|item| item.row)
            .collect())
    }

    fn change(
        &self,
        gh: &GhCommand,
        issue: u64,
        _: &[NameRow],
        add: Option<TicketState>,
        remove: &[TicketState],
    ) -> Result<(), PortError> {
        let Placement { content, item } = self.item_of(gh, issue)?;
        let current = item.as_ref().and_then(|item| item.state);
        let wanted = add.or(current.filter(|state| !remove.contains(state)));
        if wanted == current {
            return Ok(());
        }
        let ids = self.ids(gh)?;
        let item = match item {
            Some(item) => item.id,
            None => {
                let added: AddedItem = gh.graphql(
                    ADD_ITEM,
                    &[
                        ("project", GraphqlValue::Text(ids.project.clone())),
                        ("content", GraphqlValue::Text(content)),
                    ],
                )?;
                added.added.item.id
            }
        };
        let mut vars = vec![
            ("project", GraphqlValue::Text(ids.project.clone())),
            ("item", GraphqlValue::Text(item)),
            ("field", GraphqlValue::Text(ids.field.clone())),
        ];
        match wanted {
            Some(state) => {
                let option = ids.options.get(&state).ok_or_else(|| {
                    PortError::Rejected(format!(
                        "the Project's `{}` field has no `{state}` option; add one",
                        self.project.field
                    ))
                })?;
                vars.push(("option", GraphqlValue::Text(option.clone())));
                gh.graphql::<serde_json::Value>(SET_VALUE, &vars).map(drop)
            }
            None => gh
                .graphql::<serde_json::Value>(CLEAR_VALUE, &vars)
                .map(drop),
        }
    }
}
