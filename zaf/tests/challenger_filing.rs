//! The filing verbs (`plan file`, `board file`, `epic file`) through the built binary and the
//! fake `gh`: a caller whose `ZAF_INSTANCE` names the challenger is refused with one line and
//! exit 1, and nothing reaches the board; a team instance is not refused by that check.

mod common;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const CHALLENGER: &str = "challenger instance 1 on #1";
const TEAM: &str = "team instance 1 on #1";

const WAY_ON: &str =
    "the challenger checks the graph and files nothing; raise a question with `board question`";

const TASK_BODY: &str = "Goal: export works
Scope: the export
Out of scope: none
Interfaces: none
Design pages: D1
Acceptance:
- it exports
Tests: it_exports
Security trigger: no
Model: sonnet
Size: S
Blocked by: none
";

/// A project whose board would take a filing, so only the refusal can keep it from one.
fn project(instance: &str) -> Project {
    let mut project = Project::new(MANIFEST);
    project
        .on(
            "issue list --label epic *",
            r#"[{"number":1,"title":"Exports","body":"","labels":[{"name":"epic"}],"comments":[]}]"#,
        )
        .on("issue list --label task --state all *", "[]")
        .on(
            "api graphql *issueTypes*",
            r#"{"data":{"repository":{"issueTypes":{"nodes":[{"name":"Task"}]}}}}"#,
        )
        .on("api repos/*", "4242\n")
        .on("issue create *", "https://github.com/example/tracker/issues/9\n")
        .on("*", "[]")
        .env("ZAF_INSTANCE", instance);
    project
}

/// Whether the run wrote to the board: any call but a read.
fn wrote(project: &Project) -> bool {
    project.gh_calls().iter().any(|call| {
        [
            "issue create",
            "issue edit",
            "issue comment",
            "issue close",
            "api -X",
            "api --method",
        ]
        .iter()
        .any(|write| call.starts_with(write))
    })
}

fn assert_refused(project: &Project, args: &[&str]) {
    let (code, stdout, stderr) = outcome(&project.zaf(args));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    let path = args[..2].join(" ");
    let want = format!("zaf {path}: refused: {WAY_ON}");
    assert_eq!(
        stderr.lines().filter(|line| *line == want).count(),
        1,
        "{stderr}"
    );
    assert!(!wrote(project), "{:?}", project.gh_calls());
}

#[test]
fn challenger_refused_plan_file_by_verb() {
    let project = project(CHALLENGER);
    assert_refused(&project, &["plan", "file", "1"]);
}

#[test]
fn challenger_refused_board_file_by_verb() {
    let project = project(CHALLENGER);
    assert_refused(
        &project,
        &["board", "file", "1", "export as CSV", "--body", TASK_BODY],
    );
}

#[test]
fn challenger_refused_epic_file_by_verb() {
    let project = project(CHALLENGER);
    assert_refused(
        &project,
        &[
            "epic",
            "file",
            "--version",
            "0.6.0",
            "--title",
            "A new epic",
        ],
    );
}

#[test]
fn team_instance_board_file_not_refused_as_challenger() {
    let project = project(TEAM);
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "file", "1", "export as CSV", "--body", TASK_BODY]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stderr.contains(WAY_ON), "{stderr}");
    assert!(
        project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("issue create")),
        "{:?}",
        project.gh_calls()
    );
}
