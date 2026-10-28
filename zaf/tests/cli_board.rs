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
        .on("issue close 7 *", "");
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
