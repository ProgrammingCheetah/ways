//! `zaf board …` through the built binary and the fake `gh`: the terse and JSON
//! forms, and the exit codes for success, a refusal and a backend failure.

mod common;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const INSTANCE: &str = "team instance 1 on #1";

fn issue(number: u64, label: &str, comments: &str) -> String {
    format!(
        r#"{{"number":{number},"title":"Task {number}","body":"","labels":[{{"name":"task"}},{{"name":"{label}"}}],"parent":{{"number":1}},"state":"OPEN","comments":[{comments}]}}"#
    )
}

fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project
        .on(
            "issue list --label task --label ready *",
            r#"[{"number":3,"title":"Import the list","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1}},{"number":2,"title":"Export the list","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1}}]"#,
        )
        .on("issue list --label claimed *", "[]")
        .on("issue list --label question *", "[]")
        .on("issue view 2 *", &issue(2, "ready", ""))
        .on("issue view 4 *", &issue(4, "blocked", ""))
        .fail_on("issue view 5 *", "HTTP 502: Bad Gateway")
        .on("issue comment *", "https://github.com/example/tracker/issues/2#issuecomment-1\n")
        .on("issue edit *", "")
        .on("issue create *", "https://github.com/example/tracker/issues/9\n")
        .on(
            "issue view 7 *",
            r#"{"number":7,"title":"Which columns?","labels":[{"name":"question"}],"state":"OPEN","comments":[]}"#,
        )
        .on("issue close 7 *", "")
        .on("issue view 6 *", &issue(6, "question", ""))
        .on(
            "issue view 8 *",
            r#"{"number":8,"title":"Which rows?","body":"Raised on #6.","labels":[{"name":"question"}],"state":"OPEN","comments":[]}"#,
        )
        .on("issue close 8 *", "");
    project
}

#[test]
fn ready_lists_tasks_terse_and_as_json() {
    let project = project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["board", "ready"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "#2 Export the list\n#3 Import the list\n");
    let (code, stdout, _) = outcome(&project.zaf(&["--json", "board", "ready"]));
    assert_eq!(code, 0);
    let tickets: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(tickets[0]["number"], 2);
    assert_eq!(tickets[0]["epic"], 1);
}

#[test]
fn claim_exits_by_outcome() {
    let mut project = project();
    let (code, _, stderr) = outcome(&project.zaf(&["board", "claim", "2"]));
    assert_eq!(code, 2, "no instance is an environment failure");
    assert!(stderr.contains("ZAF_INSTANCE"), "{stderr}");

    project.env("ZAF_INSTANCE", INSTANCE);
    let (code, stdout, stderr) = outcome(&project.zaf(&["board", "claim", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "#2 claimed\n");
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call
                .starts_with(&format!("issue comment 2 --body claimed by {INSTANCE} at"))),
        "{calls:?}"
    );

    let (code, _, stderr) = outcome(&project.zaf(&["board", "claim", "4"]));
    assert_eq!(code, 1, "a task not ready is refused: {stderr}");
    let (code, _, stderr) = outcome(&project.zaf(&["board", "claim", "5"]));
    assert_eq!(code, 2, "a failing board is a backend failure: {stderr}");
}

#[test]
fn question_ruling_and_report_go_through() {
    let mut project = project();
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "question", "2", "Which columns?", "--door"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "door question #9 on #2\n");
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.contains("--label question --label door"))
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue edit 2 --add-label question --remove-label ready"))
    );

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "ruling", "7", "Title and author."]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "#7 ruled\n");
    assert!(
        project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("issue close 7"))
    );

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "ruling", "8", "Header row only."]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "#8 ruled; #6 ready\n");
    assert!(
        project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("issue edit 6 --add-label ready --remove-label question"))
    );

    project.env("ZAF_INSTANCE", INSTANCE);
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["--json", "board", "report", "2", "working"]));
    assert_eq!(code, 0, "{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(report["heartbeat"], false, "task 2 is not claimed");
    let marker = format!("issue comment 2 --body <!-- zaf:report {INSTANCE} -->");
    assert!(
        project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with(&marker))
    );
}

/// A task body in the template, as a planning instance files it.
const TASK_BODY: &str = "\
Goal: Members can export their reading list as a CSV file.
Owned paths: src/export/**
Interfaces: ExportSink (defined here)
Design pages: 1001
Architecture: export, ExportSink
Acceptance:
  - an empty list exports a header row only
Tests: export_empty_list_writes_header
Security trigger: no
Model: sonnet
Size: S
Blocked by: none
";

/// The project with epic #1 open and the issue types `Task` and `Bug` defined.
fn filing_project() -> Project {
    let mut project = project();
    project
        .on(
            "issue list --label epic *",
            r#"[{"number":1,"title":"Exports","body":"","labels":[{"name":"epic"}],"comments":[]}]"#,
        )
        .on("issue list --label task --state all *", "[]")
        .on(
            "api graphql *issueTypes*",
            r#"{"data":{"repository":{"issueTypes":{"nodes":[{"name":"Task"},{"name":"Bug"}]}}}}"#,
        )
        .on("api repos/*", "4242\n");
    project
}

/// The `issue create` call a filing made, if it made one.
fn created(project: &Project) -> Option<String> {
    project
        .gh_calls()
        .into_iter()
        .find(|call| call.starts_with("issue create"))
}

#[test]
fn a_filed_task_is_a_task_by_default() {
    let project = filing_project();
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "file", "1", "export as CSV", "--body", TASK_BODY]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "#9 filed under #1\n");
    let call = created(&project).expect("an issue is opened");
    assert!(call.contains("--type Task"), "{call}");
}

#[test]
fn a_filed_task_takes_the_named_type() {
    let project = filing_project();
    let (code, _, stderr) = outcome(&project.zaf(&[
        "board",
        "file",
        "1",
        "export crash",
        "--body",
        TASK_BODY,
        "--type",
        "Bug",
    ]));
    assert_eq!(code, 0, "{stderr}");
    let call = created(&project).expect("an issue is opened");
    assert!(call.contains("--type Bug"), "{call}");
}

#[test]
fn an_unknown_type_is_refused_before_filing() {
    let project = filing_project();
    let (code, stdout, stderr) = outcome(&project.zaf(&[
        "board",
        "file",
        "1",
        "export as CSV",
        "--body",
        TASK_BODY,
        "--type",
        "Nope",
    ]));
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(stdout, "");
    assert!(
        stderr.lines().any(|line| line
            == "zaf board file: example/tracker has no issue type Nope; it has Task, Bug"),
        "{stderr}"
    );
    assert_eq!(created(&project), None, "no issue is opened");
}
