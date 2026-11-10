//! The Projects board: states in a single-select field of a GitHub Project, read and written
//! through `gh api graphql`.

use super::*;

/// The field query: the Project, its built-in `Status`, a field that is not single-select,
/// and the board's `zaf state` with two of its options.
const FIELDS: &str = r#"{"data":{"repositoryOwner":{"projectV2":{"id":"PVT_1","fields":{"nodes":[
  {"id":"F_status","name":"Status","options":[{"id":"o1","name":"Todo"}]},
  {},
  {"id":"F_state","name":"zaf state","options":[{"id":"OPT_READY","name":"Ready"},{"id":"OPT_BLOCKED","name":"blocked"}]}
]}}}}}"#;

/// The field query on a Project that has no `zaf state` yet.
const FIELDS_WITHOUT_STATE: &str = r#"{"data":{"repositoryOwner":{"projectV2":{"id":"PVT_1","fields":{"nodes":[
  {"id":"F_status","name":"Status","options":[{"id":"o1","name":"Todo"}]}
]}}}}}"#;

const CREATED: &str = r#"{"data":{"createProjectV2Field":{"projectV2Field":{"id":"F_new","name":"zaf state","options":[
  {"id":"N_READY","name":"Ready"},{"id":"N_CLAIMED","name":"Claimed"},{"id":"N_BLOCKED","name":"Blocked"},
  {"id":"N_QUESTION","name":"Question"},{"id":"N_DONE","name":"Done"}
]}}}}"#;

const VALUE_SET: &str =
    r#"{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"x"}}}}"#;

fn project_board(fake: &FakeGh) -> GithubProjectBoard {
    GithubProjectBoard::on_project(
        fake.gh(),
        ProjectRef {
            owner: "example".to_owned(),
            number: 1,
            field: "zaf state".to_owned(),
        },
    )
}

/// The issue-items query's answer for `issue`: on Project 2 of the same owner as `Ready`, and on
/// the board's Project 1 in `state` when one is given.
fn items_of(issue: u64, state: Option<&str>) -> String {
    let other = r#"{"id":"PVTI_other","project":{"number":2,"owner":{"login":"example"}},"fieldValueByName":{"name":"Ready"}}"#;
    let ours = state.map(|state| {
        format!(
            r#",{{"id":"PVTI_{issue}","project":{{"number":1,"owner":{{"login":"Example"}}}},"fieldValueByName":{{"name":"{state}"}}}}"#
        )
    });
    format!(
        r#"{{"data":{{"repository":{{"issue":{{"id":"I_{issue}","projectItems":{{"nodes":[{other}{}]}}}}}}}}}}"#,
        ours.unwrap_or_default()
    )
}

/// One item of the items query: an issue of `repo` in `state`, open or closed, with `labels`.
fn item(number: u64, state: &str, open: bool, labels: &[&str], repo: &str) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|label| format!(r#"{{"name":"{label}"}}"#))
        .collect();
    let issue_state = if open { "OPEN" } else { "CLOSED" };
    format!(
        r#"{{"fieldValueByName":{{"name":"{state}"}},"content":{{"number":{number},"title":"task {number}","body":"Goal: g","state":"{issue_state}","labels":{{"nodes":[{}]}},"parent":{{"number":1}},"repository":{{"nameWithOwner":"{repo}"}}}}}}"#,
        labels.join(",")
    )
}

fn page(items: &[String], next: Option<&str>) -> String {
    let info = match next {
        Some(cursor) => format!(r#"{{"hasNextPage":true,"endCursor":"{cursor}"}}"#),
        None => r#"{"hasNextPage":false,"endCursor":null}"#.to_owned(),
    };
    format!(
        r#"{{"data":{{"repositoryOwner":{{"projectV2":{{"items":{{"pageInfo":{info},"nodes":[{}]}}}}}}}}}}"#,
        items.join(",")
    )
}

/// The one call whose argv holds `mutation`, if exactly one does.
fn the_call(fake: &FakeGh, mutation: &str) -> Vec<String> {
    let calls: Vec<Vec<String>> = fake
        .calls()
        .into_iter()
        .filter(|call| call.iter().any(|word| word.contains(mutation)))
        .collect();
    assert_eq!(calls.len(), 1, "one {mutation} call, got {calls:?}");
    calls.into_iter().next().expect("one call")
}

fn holds(call: &[String], pair: &str) -> bool {
    call.iter().any(|word| word == pair)
}

#[test]
fn ready_reads_the_field_over_every_page_and_only_open_tasks_of_the_repository() {
    let fake = FakeGh::new();
    let second = page(&[item(9, "READY", true, &["task"], REPO)], None);
    let first = page(
        &[
            item(5, "Ready", true, &["task"], REPO),
            item(6, "Blocked", true, &["task"], REPO),
            item(7, "Ready", false, &["task"], REPO),
            item(8, "Ready", true, &["task"], "example/other"),
            item(10, "Ready", true, &["question"], REPO),
            r#"{"fieldValueByName":null,"content":{}}"#.to_owned(),
        ],
        Some("c1"),
    );
    fake.on("api graphql *items(first: $page*after=c1*", &second)
        .on("api graphql *items(first: $page*", &first);
    let ready = project_board(&fake).ready().expect("ready reads");
    let ids: Vec<u64> = ready.iter().map(|ticket| ticket.id.get()).collect();
    assert_eq!(ids, [5, 9]);
    assert_eq!(ready[0].state, Some(TicketState::Ready));
    assert_eq!(ready[0].epic, Some(TicketId::from(1)));
    assert_eq!(
        ready[0].url.as_str(),
        format!("https://github.com/{REPO}/issues/5")
    );
}

#[test]
fn a_ready_task_carries_its_issue_type_and_an_untyped_one_has_none() {
    let fake = FakeGh::new();
    let typed = item(5, "Ready", true, &["task"], REPO).replace(
        r#""state":"OPEN""#,
        r#""state":"OPEN","issueType":{"name":"Bug"}"#,
    );
    let untyped = item(6, "Ready", true, &["task"], REPO)
        .replace(r#""state":"OPEN""#, r#""state":"OPEN","issueType":null"#);
    let bare = item(7, "Ready", true, &["task"], REPO);
    fake.on(
        "api graphql *items(first: $page*",
        &page(&[typed, untyped, bare], None),
    );
    let ready = project_board(&fake).ready().expect("ready reads");
    let kinds: Vec<Option<&str>> = ready
        .iter()
        .map(|ticket| ticket.kind.as_ref().map(|kind| kind.as_str()))
        .collect();
    assert_eq!(kinds, [Some("Bug"), None, None]);
}

#[test]
fn a_new_state_sets_the_items_option_on_the_boards_project() {
    let fake = FakeGh::new();
    fake.on("issue view 7 *", &issue(7, &["task"], &[], "OPEN"))
        .on("api graphql *projectItems*", &items_of(7, Some("Ready")))
        .on("api graphql *fields(first: 50)*", FIELDS)
        .on("api graphql *updateProjectV2ItemFieldValue*", VALUE_SET);
    project_board(&fake)
        .set_state(TicketId::from(7), TicketState::Blocked)
        .expect("the state is set");
    let set = the_call(&fake, "updateProjectV2ItemFieldValue");
    for pair in [
        "project=PVT_1",
        "item=PVTI_7",
        "field=F_state",
        "option=OPT_BLOCKED",
    ] {
        assert!(holds(&set, pair), "{pair} in {set:?}");
    }
    let calls = fake.calls();
    assert!(
        !calls
            .iter()
            .flatten()
            .any(|word| word.contains("addProjectV2ItemById")),
        "an issue already on the Project is not added again"
    );
}

#[test]
fn an_issue_off_the_project_is_added_before_its_state_is_set() {
    let fake = FakeGh::new();
    fake.on("issue view 8 *", &issue(8, &["task"], &[], "OPEN"))
        .on("api graphql *projectItems*", &items_of(8, None))
        .on("api graphql *fields(first: 50)*", FIELDS)
        .on(
            "api graphql *addProjectV2ItemById*",
            r#"{"data":{"addProjectV2ItemById":{"item":{"id":"PVTI_8"}}}}"#,
        )
        .on("api graphql *updateProjectV2ItemFieldValue*", VALUE_SET);
    let board = project_board(&fake);
    board
        .set_state(TicketId::from(8), TicketState::Ready)
        .expect("the state is set");
    let added = the_call(&fake, "addProjectV2ItemById");
    assert!(holds(&added, "content=I_8"), "{added:?}");
    let set = the_call(&fake, "updateProjectV2ItemFieldValue");
    assert!(holds(&set, "item=PVTI_8") && holds(&set, "option=OPT_READY"));
}

#[test]
fn a_project_without_the_field_gets_one_with_an_option_per_state() {
    let fake = FakeGh::new();
    fake.on("issue view 7 *", &issue(7, &["task"], &[], "OPEN"))
        .on("api graphql *projectItems*", &items_of(7, Some("Ready")))
        .on("api graphql *createProjectV2Field*", CREATED)
        .on("api graphql *fields(first: 50)*", FIELDS_WITHOUT_STATE)
        .on("api graphql *updateProjectV2ItemFieldValue*", VALUE_SET);
    project_board(&fake)
        .set_state(TicketId::from(7), TicketState::Done)
        .expect("the state is set");
    let created = the_call(&fake, "createProjectV2Field");
    assert!(holds(&created, "project=PVT_1") && holds(&created, "name=zaf state"));
    let set = the_call(&fake, "updateProjectV2ItemFieldValue");
    assert!(holds(&set, "field=F_new") && holds(&set, "option=N_DONE"));
}

#[test]
fn a_field_without_the_states_option_is_refused_with_the_fix() {
    let fake = FakeGh::new();
    fake.on("issue view 7 *", &issue(7, &["task"], &[], "OPEN"))
        .on("api graphql *projectItems*", &items_of(7, Some("Ready")))
        .on("api graphql *fields(first: 50)*", FIELDS);
    let refused = project_board(&fake).set_state(TicketId::from(7), TicketState::Done);
    assert!(
        matches!(&refused, Err(PortError::Rejected(message)) if message.contains("no `done` option; add one")),
        "got {refused:?}"
    );
}

#[test]
fn a_transient_graphql_error_is_tried_again_three_times() {
    let fake = FakeGh::new();
    fake.fail_on(
        "api graphql *",
        "gh: Your attempt to move this item created a temporary conflict. Please try again.",
    );
    let failed = project_board(&fake).ready();
    assert!(
        matches!(failed, Err(PortError::Backend(_))),
        "got {failed:?}"
    );
    assert_eq!(fake.calls().len(), 4, "one call and three more tries");
}

#[test]
fn any_other_graphql_error_is_not_tried_again() {
    let fake = FakeGh::new();
    fake.fail_on(
        "api graphql *",
        "gh: Resource not accessible by integration",
    );
    let failed = project_board(&fake).ready();
    assert!(
        matches!(failed, Err(PortError::Backend(_))),
        "got {failed:?}"
    );
    assert_eq!(fake.calls().len(), 1);
}
