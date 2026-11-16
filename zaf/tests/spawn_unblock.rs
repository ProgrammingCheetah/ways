//! The tick's unblocking, through the binary (docs/zaf-actions.md §5.1; ruling 48, R5): one case of
//! the label sync. `zaf spawn --once` sets each blocked task whose blockers are all done to
//! `ready`, prints `#26 unblocked: #25 is done` in its block and `unblocked` in `--json`, and
//! leaves a task whose blocker is still open as it is. The dry run prints the same line and sets
//! nothing. A board that fails while unblocking is named on stderr and exits 2, and the tick still
//! runs.

mod common;

use std::fs;

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n";

const SPAWN: [&str; 4] = ["spawn", "--once", "--version", "0.3.0"];

/// The blocked tasks as `gh issue list` prints them: #26 blocked by #25, #28 by #27.
const BLOCKED: &str = r#"[
  {"number":26,"title":"Write the file","body":"Goal: write it\nBlocked by: #25","labels":[{"name":"task"},{"name":"blocked"}],"parent":{"number":1}},
  {"number":28,"title":"Read the file","body":"Goal: read it\nBlocked by: #27","labels":[{"name":"task"},{"name":"blocked"}],"parent":{"number":1}}
]"#;

/// #25, closed as completed: done.
const DONE_25: &str = r#"{"number":25,"state":"CLOSED","stateReason":"COMPLETED","labels":[{"name":"task"}],"url":"https://github.com/example/tracker/issues/25"}"#;

/// #27, still open: not done.
const OPEN_27: &str = r#"{"number":27,"state":"OPEN","stateReason":null,"labels":[{"name":"task"},{"name":"claimed"}],"url":"https://github.com/example/tracker/issues/27"}"#;

/// #26 as `gh issue view` prints it, before its state is set.
const TASK_26: &str = r#"{"number":26,"title":"Write the file","body":"Goal: write it\nBlocked by: #25","labels":[{"name":"task"},{"name":"blocked"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// The call that sets #26 to `ready`.
const SET_READY: &str = "issue edit 26 --add-label ready --remove-label blocked";

/// A project with current profiles whose board holds the two blocked tasks; `blocker_25` is how
/// `gh issue view 25` answers, a failure when it is `None`.
fn project_with(blocker_25: Option<&str>) -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    // The tick labels every open task from its blockers; the two blocked ones are all there are.
    project.on("issue list --label task --state open *", BLOCKED);
    match blocker_25 {
        Some(answer) => project.on("issue view 25 *", answer),
        None => project.fail_on("issue view 25 *", "HTTP 502: the board is down"),
    };
    project
        .on("issue view 27 *", OPEN_27)
        .on("issue view 26 *", TASK_26)
        .on("issue edit 26 *", "")
        .on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

fn project() -> Project {
    project_with(Some(DONE_25))
}

fn set_ready(project: &Project) -> bool {
    project
        .gh_calls()
        .iter()
        .any(|call| call.starts_with(SET_READY))
}

#[test]
fn a_task_whose_blockers_are_done_is_set_ready_and_named() {
    let project = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| line == "#26 unblocked: #25 is done"),
        "{stdout}"
    );
    assert!(set_ready(&project), "{:?}", project.gh_calls());
    // #28 waits on the open #27: no line, no write.
    assert!(!stdout.contains("#28 unblocked"), "{stdout}");
    assert!(
        project
            .gh_calls()
            .iter()
            .all(|call| !call.starts_with("issue edit 28")),
        "{:?}",
        project.gh_calls()
    );
    assert!(!stdout.contains("nothing to do"), "{stdout}");
}

#[test]
fn the_json_names_what_was_unblocked() {
    let project = project();

    let mut args = vec!["--json"];
    args.extend(SPAWN);
    let (code, stdout, stderr) = outcome(&project.zaf(&args));

    assert_eq!(code, 0, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON document");
    assert_eq!(
        json["unblocked"],
        serde_json::json!([{ "task": 26, "blockers": [25] }])
    );
    assert!(set_ready(&project));
}

#[test]
fn a_dry_run_names_the_task_and_sets_nothing() {
    let project = project();

    let mut args = SPAWN.to_vec();
    args.push("--dry-run");
    let (code, stdout, stderr) = outcome(&project.zaf(&args));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| line == "#26 unblocked: #25 is done"),
        "{stdout}"
    );
    assert!(!set_ready(&project), "{:?}", project.gh_calls());
    assert!(
        project
            .gh_calls()
            .iter()
            .all(|call| !call.starts_with("issue edit")),
        "the dry run wrote: {:?}",
        project.gh_calls()
    );

    let mut json_args = vec!["--json"];
    json_args.extend(&args);
    let (code, stdout, stderr) = outcome(&project.zaf(&json_args));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON document");
    assert_eq!(
        json["unblocked"],
        serde_json::json!([{ "task": 26, "blockers": [25] }])
    );
    assert!(!set_ready(&project));
}

#[test]
fn a_board_that_fails_while_unblocking_fails_the_tick_and_the_tick_goes_on() {
    let project = project_with(None);

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stderr.contains("cannot unblock #26: "), "{stderr}");
    assert!(stderr.contains("the board is down"), "{stderr}");
    assert!(!set_ready(&project), "{:?}", project.gh_calls());
    // The rest of the tick ran: its block was printed, and the block read the questions again
    // after the plan's own read.
    assert!(stdout.starts_with("tick "), "{stdout}");
    assert!(stdout.contains("slots "), "{stdout}");
    let question_reads = project
        .gh_calls()
        .iter()
        .filter(|call| call.starts_with("issue list --label question"))
        .count();
    assert_eq!(question_reads, 2, "{:?}", project.gh_calls());
}
