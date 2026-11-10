//! The ways repository's `task-pr` action (docs/zaf-actions.md §3, §7): run through `zaf do`
//! against the fake `gh`, it hands back a task's pull request: the slice's record when there is
//! one, else of the pull requests titled `(#<task>)` or from a branch ending `-<task>` the open
//! one, else the latest merged, else the latest closed; none is exit 1.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use common::{Project, REPO, outcome, write};

const TASK: &str = "377";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/task-pr");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project with the `task-pr` action in its actions folder, its gh scene empty.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = project.root.join(".zaf/actions");
    write(&actions, "task-pr", &action_text());
    fs::set_permissions(actions.join("task-pr"), fs::Permissions::from_mode(0o755)).expect("mode");
    project
}

/// `zaf do --more task-pr <args>`: the exit code, the whole log (stdout) and what zaf said
/// about the run (stderr).
fn find(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "task-pr"];
    command.extend_from_slice(args);
    outcome(&project.zaf(&command))
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// One row of `gh pr list`, already through its `--jq`: number, state, branch and title.
fn row(number: u32, state: &str, branch: &str, title: &str) -> String {
    format!("{number}\t{state}\t{branch}\t{title}\n")
}

/// The `gh pr list` call for the task, as the fake records it.
fn list_call() -> String {
    format!(
        "pr list --state all --search {TASK} --limit 50 --json number,state,headRefName,title --jq .[] | [.number, .state, .headRefName, .title] | @tsv --repo {REPO}"
    )
}

#[test]
fn task_pr_hands_back_the_slice_record_without_asking_gh() {
    let project = project();
    // As zaf writes it: the number alone, no line ending.
    write(&project.root, &format!(".zaf/slices/{TASK}.pr"), "41");

    for task in [TASK, "#377"] {
        let (code, log, said) = find(&project, &[task]);

        assert_eq!(code, 0, "{task}: {log}{said}");
        assert_eq!(handed_back(&log), "41", "{task}: {log}");
        assert!(log.contains("#41 from the slice record"), "{log}");
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn task_pr_asks_gh_when_the_record_does_not_hold() {
    let mut project = project();
    write(&project.root, &format!(".zaf/slices/{TASK}.pr"), "none\n");
    project.on(
        "pr list *",
        &row(48, "OPEN", "feature/v1-2-377", "Add the CSV writer (#377)"),
    );

    let (code, log, said) = find(&project, &[TASK]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "48", "{log}");
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn task_pr_prefers_the_open_one() {
    let rows = [
        row(
            52,
            "MERGED",
            "feature/v1-2-377",
            "Add the CSV writer (#377)",
        ),
        row(
            48,
            "OPEN",
            "feature/v1-3-377",
            "Add the CSV writer, again (#377)",
        ),
        row(45, "CLOSED", "feature/v1-1-377", "Add a CSV writer (#377)"),
    ]
    .concat();
    for task in [TASK, "#377"] {
        let mut project = project();
        project.on("pr list *", &rows);

        let (code, log, said) = find(&project, &[task]);

        assert_eq!(code, 0, "{task}: {log}{said}");
        assert_eq!(handed_back(&log), "48", "{task}: {log}");
        // Each one is listed with its state and branch, newest first, as gh lists them.
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(
            lines,
            [
                "#52 MERGED feature/v1-2-377",
                "#48 OPEN feature/v1-3-377",
                "#45 CLOSED feature/v1-1-377",
                "48",
            ],
            "{task}"
        );
        // The search is by the number, without its `#`.
        assert_eq!(project.gh_calls(), [list_call()], "{task}");
    }
}

#[test]
fn task_pr_without_an_open_one_hands_back_the_latest_merged_else_closed() {
    let mut project = project();
    project.on(
        "pr list *",
        &[
            row(60, "CLOSED", "feature/v1-4-377", "Try the writer (#377)"),
            row(
                55,
                "MERGED",
                "feature/v1-3-377",
                "Add the CSV writer (#377)",
            ),
            row(50, "MERGED", "feature/v1-2-377", "Start the writer (#377)"),
        ]
        .concat(),
    );
    let (code, log, said) = find(&project, &[TASK]);
    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "55", "{log}");

    let mut project = self::project();
    project.on(
        "pr list *",
        &[
            row(60, "CLOSED", "feature/v1-4-377", "Try the writer (#377)"),
            row(
                58,
                "CLOSED",
                "feature/v1-3-377",
                "Try the writer once (#377)",
            ),
        ]
        .concat(),
    );
    let (code, log, said) = find(&project, &[TASK]);
    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "60", "{log}");
}

#[test]
fn task_pr_matches_a_branch_ending_in_the_task() {
    let mut project = project();
    // The search finds both by the number in the title; only #70 belongs to the task, by its
    // branch, since neither title ends `(#377)`.
    project.on(
        "pr list *",
        &[
            row(71, "OPEN", "docs-377-notes", "Notes on 377"),
            row(70, "OPEN", "feature/v1-2-377", "Task 377: the CSV writer"),
        ]
        .concat(),
    );

    let (code, log, said) = find(&project, &[TASK]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "70", "{log}");
    assert!(log.contains("#70 OPEN feature/v1-2-377"), "{log}");
    assert!(!log.contains("#71"), "{log}");
}

#[test]
fn task_pr_does_not_take_another_tasks_pull_request() {
    let others = [
        row(80, "OPEN", "feature/v1-3770", "Add the reader (#3770)"),
        row(81, "OPEN", "feature/v1-1377", "Fix the reader (#1377)"),
        row(
            82,
            "OPEN",
            "feature/v1-2-3770",
            "Task 377 follow-up (#3771)",
        ),
    ]
    .concat();

    let mut project = project();
    project.on(
        "pr list *",
        &format!(
            "{others}{}",
            row(
                79,
                "MERGED",
                "feature/v1-2-377",
                "Add the CSV writer (#377)"
            )
        ),
    );
    let (code, log, said) = find(&project, &[TASK]);
    // The open ones are another task's; the merged one is this task's.
    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "79", "{log}");
    for other in ["#80", "#81", "#82"] {
        assert!(!log.contains(other), "{other}: {log}");
    }

    let mut project = self::project();
    project.on("pr list *", &others);
    let (code, log, said) = find(&project, &[TASK]);
    assert_eq!(code, 1, "{log}{said}");
    assert!(
        log.contains("no pull request belongs to task #377"),
        "{log}"
    );
}

#[test]
fn task_pr_with_none_exits_1() {
    let mut project = project();
    project.on("pr list *", "");

    let (code, log, said) = find(&project, &[TASK]);

    assert_eq!(code, 1, "{log}{said}");
    assert_eq!(
        handed_back(&log),
        "no pull request belongs to task #377",
        "{log}"
    );
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn task_pr_refuses_arguments_it_cannot_use() {
    let project = project();

    for args in [
        &[][..],
        &["abc"][..],
        &[""][..],
        &["#"][..],
        &["-377"][..],
        &["37x"][..],
        &["3 7"][..],
        &["../377"][..],
        &[TASK, "378"][..],
    ] {
        let (code, log, said) = find(&project, args);
        assert_eq!(code, 1, "{args:?}: {log}{said}");
    }

    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn task_pr_fails_with_2_when_gh_fails() {
    let mut project = project();
    project.fail_on("pr list *", "HTTP 502");

    let (code, log, said) = find(&project, &[TASK]);

    assert_eq!(code, 2, "{log}{said}");
    assert!(log.contains("HTTP 502"), "{log}");
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn task_pr_without_a_manifest_repo_leaves_the_repository_to_gh() {
    let mut project = project();
    fs::write(
        project.root.join(".zaf.toml"),
        "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n",
    )
    .expect("manifest");
    project.on(
        "pr list *",
        &row(48, "OPEN", "feature/v1-2-377", "Add the CSV writer (#377)"),
    );

    let (code, log, said) = find(&project, &[TASK]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), "48", "{log}");
    assert_eq!(
        project.gh_calls(),
        [list_call()
            .trim_end_matches(&format!(" --repo {REPO}"))
            .to_owned()]
    );
}
