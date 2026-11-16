//! The core's placement beside the old plan, through the binary (docs/zaf-actions.md §5.2):
//! `zaf spawn --once --dry-run --json` carries `placement`, where the scheduling core places
//! each open ticket with a phase list, deciding nothing; the plain-text dry run is as it was and
//! says nothing of it. Both read the open tickets for the labels the tick would set, and write
//! none.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [phases]\nTask = [\"merge\"]\n";

/// A task body of size S naming `blocked_by` as its blockers.
fn task_body(blocked_by: &str) -> String {
    format!(
        "Goal: Members export their reading list as CSV.\n\
         Owned paths: src/export/**\n\
         Interfaces: none\n\
         Design pages: none\n\
         Architecture: app over ports\n\
         Acceptance:\n  - an empty list exports the header row only\n\
         Tests: beside the use case\n\
         Security trigger: no\n\
         Model: sonnet\n\
         Size: S\n\
         Blocked by: {blocked_by}\n"
    )
}

/// A project with current profiles and a `zaf`-kind `merge` as its one phase: task 7 (of epic 1)
/// is claimed, task 8 is blocked by it. Every other `gh` call answers an empty list.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    let ticket = |number: u64, state: &str, blocked_by: &str| {
        serde_json::json!({
            "number": number,
            "title": format!("Task {number}"),
            "body": task_body(blocked_by),
            "labels": [{"name": "task"}, {"name": state}],
            "parent": {"number": 1},
            "state": "OPEN",
            "comments": [{"body": "claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}],
            "issueType": {"name": "Task"},
        })
    };
    let (seven, eight) = (ticket(7, "claimed", "none"), ticket(8, "blocked", "#7"));
    project
        .on(
            "issue list --label task --state open *",
            &serde_json::json!([seven, eight]).to_string(),
        )
        .on("issue list --label claimed *", r#"[{"number":7}]"#)
        .on("issue view 7 *", &seven.to_string())
        .on("issue view 8 *", &eight.to_string())
        .on("*", "[]");
    project.provide_slice_actions();
    let file = project.root.join(".zaf/actions/merge");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
         # zaf kind: zaf\n# zaf ends: merged\ntrue\n",
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

#[test]
fn the_json_dry_run_carries_the_placement_and_the_plain_one_is_unchanged() {
    let project = project();

    // Plain text first: it says nothing of the placement. It reads the open tickets for the
    // labels the tick would set (none here: #8 carries `blocked` and waits on #7), and writes none.
    let (code, plain, stderr) =
        outcome(&project.zaf(&["spawn", "--once", "--dry-run", "--version", "0.1"]));
    assert_eq!(code, 0, "{plain}{stderr}");
    assert!(!plain.contains("placement"), "{plain}");
    assert!(
        !plain.contains("labelled") && !plain.contains("unblocked"),
        "{plain}"
    );
    let calls = project.gh_calls();
    assert!(
        calls.iter().all(|call| !call.starts_with("issue edit")),
        "{calls:?}"
    );

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["--json", "spawn", "--once", "--dry-run", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    // The plan's keys are all still there.
    for key in ["dry_run", "held_tasks", "unblocked"] {
        assert!(json.get(key).is_some(), "{key} is gone: {json}");
    }
    let placement = &json["placement"];
    assert_eq!(
        placement["unreadable"],
        serde_json::json!([]),
        "{placement}"
    );
    let tickets = placement["tickets"].as_array().expect("tickets");
    assert_eq!(tickets.len(), 2, "{placement}");
    // Task 7 may run its merge; the time it is ready since is the dry run's own.
    assert_eq!(tickets[0]["ticket"], 7, "{placement}");
    assert_eq!(tickets[0]["place"], "lane", "{placement}");
    assert_eq!(tickets[0]["next"], "merge", "{placement}");
    assert!(tickets[0]["since"].is_string(), "{placement}");
    // Task 8 waits on task 7, which is not done.
    assert_eq!(
        tickets[1],
        serde_json::json!({"ticket": 8, "place": "waiting", "on": [{"ticket": 7}]}),
        "{placement}"
    );
}
