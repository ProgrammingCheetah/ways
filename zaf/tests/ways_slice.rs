//! The ways repository's `slice-open`, `slice-pr` and `slice-drop` actions (docs/zaf-actions.md
//! §3, §7; rulings 26, 27 and 39): run through `zaf do` on a throwaway jj repository with a bare
//! remote and a fake `gh` that knows task 2 of epic 1, each acts on the task's slice by the
//! values zaf hands it, hands back what zaf keeps as the task's record, and runs only in the
//! phases it names.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"

[vcs]
kind = "jj"
"#;

/// The actions the three are built from, and the three.
const ACTIONS: [&str; 11] = [
    "workspace-open",
    "workspace-drop",
    "commit",
    "push",
    "pr-open",
    "merge-base",
    "pr-merge",
    "slice-open",
    "slice-pr",
    "slice-merge",
    "slice-drop",
];

/// Task 2 of epic 1, claimed by a team instance, as `gh issue view` prints it.
const CLAIMED: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[{"body":"claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}]}"#;

/// Task 2 of epic 1, not yet claimed.
const READY: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote, the actions in
/// its actions folder, the built `zaf` on `PATH` so they can call it, and a fake `gh` where task 2
/// holds `ticket`.
fn project_with(ticket: &str) -> Project {
    let mut project = Project::new(MANIFEST);
    let temp = project.temp.path().to_owned();
    let root = project.root.clone();
    run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
    jj(&temp, &["git", "init", "repo"]);
    jj(
        &root,
        &["config", "set", "--repo", "user.name", "Test Author"],
    );
    jj(
        &root,
        &[
            "config",
            "set",
            "--repo",
            "user.email",
            "author@example.com",
        ],
    );
    let remote = temp.join("remote.git");
    jj(
        &root,
        &["git", "remote", "add", "origin", &remote.to_string_lossy()],
    );
    write(&root, ".gitignore", ".zaf/*\n");
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // The board and the forge: the claim is the label and a comment, a pull request is number 7.
    project
        .on("issue list --label claimed *", "[{\"number\":2}]")
        .on("issue view 2 *", ticket)
        .on(&format!("pr list --head {SLICE} *"), "")
        .on(
            "pr create *",
            &format!("https://github.com/{REPO}/pull/7\n"),
        )
        .on("pr view 7 *", "abc\n")
        .env("ZAF_VERSION", "v1");

    // The built `zaf` first on PATH, then the fake gh, so a body can say `zaf do`.
    let bin = Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder")
        .to_owned();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [bin, fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    project.env("PATH", &search_path.to_string_lossy());

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    for name in ACTIONS {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }
    project
}

/// A project whose task 2 is claimed: it stands in `claim`.
fn project() -> Project {
    project_with(CLAIMED)
}

/// `zaf do --more <args>` with its whole log: the exit code and the lines.
fn action(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// The slice's workspace, where `workspace-open` puts it.
fn workspace(project: &Project) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(SLICE)
}

/// What zaf kept of what an action handed back: the record `name` under `.zaf/slices`.
fn record(project: &Project, name: &str) -> Option<String> {
    fs::read_to_string(project.root.join(".zaf/slices").join(name)).ok()
}

/// Opens the slice and returns its workspace.
fn open(project: &Project) -> PathBuf {
    let (code, text) = action(project, &["slice-open", "2"]);
    assert_eq!(code, 0, "{text}");
    workspace(project)
}

/// The commit a revset names in the root repository.
fn commit_at(root: &Path, revset: &str) -> String {
    jj(
        root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            "commit_id",
        ],
    )
}

fn bookmark_at(root: &Path, name: &str) -> String {
    commit_at(root, &format!("bookmarks(exact:\"{name}\")"))
}

/// A field of a commit in a workspace.
fn commit_field(dir: &Path, revset: &str, template: &str) -> String {
    jj(dir, &["log", "--no-graph", "-r", revset, "-T", template])
}

/// Stores a task check's record as `zaf check` would: the last one, with `verdict`.
fn record_check(project: &Project, verdict: &str) {
    write(
        &project.root,
        ".zaf/runs/check-task-2.json",
        &format!(r#"{{"target":"task #2","verdict":"{verdict}","lines":[]}}"#),
    );
}

/// Task 2 of epic 1, merged and marked done.
const DONE: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"done"}],"parent":{"number":1},"state":"CLOSED","comments":[{"body":"claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}]}"#;

/// The forge's view of pull request 7 at `head`: open until `gh pr merge` is called, which the
/// fake accepts. Rules go in front of the earlier ones, so a later call moves the head.
fn forge_at(project: &mut Project, head: &str) {
    project
        .on_first("pr view 7 --json state *", r#"{"state":"MERGED"}"#)
        .on_first("pr view 7 --json state --jq *", "OPEN\n")
        .on_first("pr view 7 --json headRefOid*", &format!("{head}\n"))
        .on_first("pr merge *", "")
        .on_first("issue edit *", "");
}

/// Stores a `check pr` record for pull request 7: `verdict` for the commit `head`, begun long ago.
fn pr_check(project: &Project, verdict: &str, head: &str) {
    write(
        &project.root,
        ".zaf/runs/check-pr-7.json",
        &format!(
            r#"{{"target":"pr #7","verdict":"{verdict}","lines":[],"head":"{head}","at":"2020-01-01T00:00:00Z"}}"#
        ),
    );
}

/// A slice with `content` in `path`, published as pull request 7 whose `check pr` is clean: the
/// task is in `land`. The project, the workspace and the head.
fn landing_with(path: &str, content: &str) -> (Project, PathBuf, String) {
    let mut project = project();
    let dir = open(&project);
    write(&dir, path, content);
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    let head = commit_field(&dir, "@-", "commit_id");
    forge_at(&mut project, &head);
    pr_check(&project, "clean", &head);
    (project, dir, head)
}

fn landing() -> (Project, PathBuf, String) {
    landing_with("writer.csv", "a,b\n")
}

/// Pushes a commit that sets `path` to `content` onto the base from a second clone, as another
/// merged pull request would.
fn advance_base_elsewhere(project: &Project, path: &str, content: &str) {
    let temp = project.temp.path();
    let remote = temp.join("remote.git");
    run(
        temp,
        "git",
        &["clone", "-q", &remote.to_string_lossy(), "other"],
    );
    let other = temp.join("other");
    run(&other, "git", &["checkout", "-q", BASE]);
    write(&other, path, content);
    run(&other, "git", &["add", path]);
    run(
        &other,
        "git",
        &[
            "-c",
            "user.name=Other",
            "-c",
            "user.email=other@example.com",
            "commit",
            "-qm",
            "Another pull request",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", BASE]);
}

/// What the remote holds under the slice's branch.
fn remote_slice(project: &Project) -> String {
    run(
        &project.temp.path().join("remote.git"),
        "git",
        &["rev-parse", SLICE],
    )
    .trim()
    .to_owned()
}

/// Whether any `gh` call so far began with `prefix`.
fn called(project: &Project, prefix: &str) -> bool {
    project
        .gh_calls()
        .iter()
        .any(|call| call.starts_with(prefix))
}

/// Asserts `slice-merge` is refused in `phase`: not run, with the §3 message, and so no step.
fn assert_not_run_in(project: &Project, phase: &str) -> String {
    let merges = |project: &Project| {
        project
            .gh_calls()
            .iter()
            .filter(|call| call.starts_with("pr merge"))
            .count()
    };
    let merged = merges(project);
    let (code, text) = action(project, &["slice-merge", "2"]);
    assert_eq!(merges(project), merged, "{text}");
    assert_eq!(code, 125, "{text}");
    assert!(text.contains("NOT_RUN slice-merge: "), "{text}");
    assert!(
        text.contains(&format!("2 is in {phase}, not land")),
        "{text}"
    );
    assert!(!text.contains("zaf do "), "a step ran: {text}");
    text
}

#[test]
fn slices_have_valid_metadata() {
    // The group of each: team and release, and the phase roles of the actions that call it (the
    // ruling on #518): implement and fix open and publish a slice, merge lands one.
    for (name, group) in [
        ("slice-open", "# zaf group: team, release, implement, fix"),
        ("slice-pr", "# zaf group: team, release, implement, fix"),
        ("slice-merge", "# zaf group: team, release, merge"),
        ("slice-drop", "# zaf group: team, release"),
    ] {
        let text = action_text(name);
        // The header zaf reads: a description, the group, a task target.
        for line in ["# zaf mode: 750", group, "# zaf target: task"] {
            assert!(text.lines().any(|found| found == line), "{name}: {line}");
        }
        assert!(
            text.lines()
                .any(|line| line.starts_with("# zaf description: ")),
            "{name} has no description"
        );
    }
    // The phases each runs in: open before the slice exists; the other two from `build`, which a
    // failed task check keeps (ruling 26), and drop also once done. `slice-pr` also runs in
    // `publish`, where the `fix` phase finds a task whose CI failed before any `check pr` ran.
    for (name, phases) in [
        ("slice-open", "# zaf phases: claim"),
        ("slice-pr", "# zaf phases: build, check, publish, review"),
        ("slice-merge", "# zaf phases: land"),
        ("slice-drop", "# zaf phases: build, check, done"),
    ] {
        let text = action_text(name);
        assert!(text.lines().any(|line| line == phases), "{name}: {phases}");
    }
    // 125 is zaf's alone: no action declares it.
    for name in ACTIONS {
        assert!(
            !action_text(name)
                .lines()
                .any(|line| line.starts_with("# zaf 125")),
            "{name} declares 125"
        );
    }
}

#[test]
fn slice_merge_runs_the_steps_in_order() {
    let (project, dir, head) = landing();

    let (code, text) = action(&project, &["slice-merge", "2"]);

    assert_eq!(code, 0, "{text}");
    // The base is merged in first, then the pull request, then the board, then the workspace goes;
    // CI is the gate, so no light gate runs.
    let steps: Vec<usize> = [
        "zaf do merge-base",
        "zaf do pr-merge",
        "zaf do workspace-drop",
    ]
    .iter()
    .map(|step| text.find(step).unwrap_or_else(|| panic!("{step}: {text}")))
    .collect();
    assert!(steps.is_sorted(), "{text}");
    assert!(!text.contains("gate-light"), "{text}");
    assert!(!text.contains("zaf do push"), "nothing moved: {text}");
    let calls = project.gh_calls();
    let merge = calls
        .iter()
        .position(|call| {
            call == &format!("pr merge 7 --merge --match-head-commit {head} --repo {REPO}")
        })
        .unwrap_or_else(|| panic!("{calls:?}"));
    let done = calls
        .iter()
        .position(|call| call.starts_with("issue edit 2 ") && call.contains("done"))
        .unwrap_or_else(|| panic!("{calls:?}"));
    assert!(merge < done, "{calls:?}");
    // The workspace is gone and zaf forgot the task's records.
    assert!(!dir.exists(), "the workspace is still there");
    assert_eq!(record(&project, "2"), None);
    assert_eq!(record(&project, "2.pr"), None);
    // The branch with the merged work stays.
    assert_eq!(bookmark_at(&project.root, SLICE), head);
}

#[test]
fn slice_merge_pins_the_checked_head() {
    let (project, _dir, head) = landing();

    // The merge names the head the slice stands on, the commit CI passed.
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(project.gh_calls().contains(&format!(
        "pr merge 7 --merge --match-head-commit {head} --repo {REPO}"
    )));

    // A push after CI moves the pull request's head: the forge refuses the pinned merge, so the
    // run ends 1, the task is not marked done and the workspace stays.
    let (mut other, dir_other, _) = landing();
    other.fail_first(
        "pr merge *",
        "X Pull request #7 Head branch was modified. Review and try the merge again.",
    );
    let (code, text) = action(&other, &["slice-merge", "2"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("Head branch was modified"), "{text}");
    assert!(!called(&other, "issue edit"), "{text}");
    assert!(!text.contains("zaf do workspace-drop"), "{text}");
    assert!(dir_other.is_dir());
    assert!(record(&other, "2").is_some());
    // Nor is it retried: only a moved base is waited for.
    assert_eq!(
        other
            .gh_calls()
            .iter()
            .filter(|call| call.starts_with("pr merge"))
            .count(),
        1
    );
}

#[test]
fn slice_merge_conflict_exits_1() {
    let (project, dir, head) = landing_with("notes.txt", "one\nslice\n");
    advance_base_elsewhere(&project, "notes.txt", "one\nbase\n");

    let (code, text) = action(&project, &["slice-merge", "2"]);

    assert_eq!(code, 1, "{text}");
    // The paths are in the log, and nothing was pushed, merged or marked.
    assert!(text.contains("conflicts in"), "{text}");
    assert!(text.contains("notes.txt"), "{text}");
    assert!(!text.contains("zaf do push"), "{text}");
    assert!(!text.contains("gate-light"), "{text}");
    assert!(!called(&project, "pr merge"), "{text}");
    assert!(!called(&project, "issue edit"), "{text}");
    assert_eq!(remote_slice(&project), head);
    assert!(dir.is_dir(), "the workspace went");
    assert_eq!(commit_field(&dir, "@-", "commit_id"), head);
    assert!(record(&project, "2").is_some());
}

#[test]
fn slice_merge_maps_a_nested_not_run_to_two() {
    let (project, dir, head) = landing();
    advance_base_elsewhere(&project, "merged.txt", "merged\n");
    // `merge-base` has no description: zaf does not run it, which is exit 125 and not a conflict.
    // `slice-merge` declares 1 and 2, never 125, so it ends with 2.
    let actions = project.root.join(".zaf/actions");
    write(&actions, "merge-base", "#!/bin/sh\nexit 1\n");
    fs::set_permissions(
        actions.join("merge-base"),
        fs::Permissions::from_mode(0o750),
    )
    .expect("mode");

    let (code, text) = action(&project, &["slice-merge", "2"]);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("NOT_RUN merge-base"), "{text}");
    // Nothing was pushed or merged, and the slice is as it was.
    assert!(!text.contains("zaf do push"), "{text}");
    assert!(!called(&project, "pr merge"), "{text}");
    assert_eq!(remote_slice(&project), head);
    assert!(dir.is_dir());
    assert_eq!(commit_field(&dir, "@-", "commit_id"), head);
}

#[test]
fn slice_merge_pushes_a_moved_head_and_asks_for_a_check() {
    let (project, dir, head) = landing();
    advance_base_elsewhere(&project, "merged.txt", "merged\n");

    let (code, text) = action(&project, &["slice-merge", "2"]);

    // The base moved the head: it is pushed and the run ends with 1, so the team checks again.
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("zaf do push"), "{text}");
    assert!(text.contains("run zaf check pr again"), "{text}");
    assert!(!text.contains("zaf do pr-merge"), "{text}");
    assert!(!text.contains("gate-light"), "{text}");
    let moved = commit_field(&dir, "@-", "commit_id");
    assert_ne!(moved, head);
    assert_eq!(remote_slice(&project), moved);
    assert!(
        dir.join("merged.txt").is_file(),
        "the base is in the workspace"
    );
    assert!(!called(&project, "pr merge"), "{text}");
    assert!(!called(&project, "issue edit"), "{text}");
    assert!(record(&project, "2").is_some());
}

#[test]
fn slice_merge_moved_head_still_marks_done() {
    let (mut project, dir, head) = landing();
    advance_base_elsewhere(&project, "merged.txt", "merged\n");
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 1, "{text}");
    let moved = commit_field(&dir, "@-", "commit_id");
    assert_ne!(moved, head);

    // The forge now shows the pushed head, and `check pr` is clean on it: the task is in `land`.
    forge_at(&mut project, &moved);
    pr_check(&project, "clean", &moved);
    let (code, text) = action(&project, &["slice-merge", "2"]);

    // The base is already in: nothing moves, the pull request merges and the board marks it done.
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("zaf do push"), "{text}");
    assert!(called(&project, "pr merge 7 --merge"), "{text}");
    assert!(called(&project, "issue edit 2 "), "{text}");
    assert!(!dir.exists(), "the workspace is still there");
    assert_eq!(record(&project, "2"), None);
    assert_eq!(bookmark_at(&project.root, SLICE), moved);
}

#[test]
fn failed_pr_merge_needs_a_new_check() {
    let (mut project, dir, head) = landing();
    advance_base_elsewhere(&project, "merged.txt", "merged\n");
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 1, "{text}");
    let moved = commit_field(&dir, "@-", "commit_id");
    assert_ne!(moved, head);

    // The merge was pushed, so the forge's head moved on and the last `check pr` is for the old
    // one: the task is out of `land`, and a merge is not even attempted.
    forge_at(&mut project, &moved);
    let before = project.gh_calls().len();
    let text = assert_not_run_in(&project, "publish");
    assert!(text.contains("NOT_RUN"), "{text}");
    assert!(
        project.gh_calls()[before..]
            .iter()
            .all(|call| !call.starts_with("pr merge") && !call.starts_with("issue edit"))
    );
    assert!(dir.is_dir());

    // A new `check pr` that is clean on the pushed head puts it back in `land`; if the forge then
    // refuses the merge, the run stops there: nothing is marked done and the workspace stays.
    pr_check(&project, "clean", &moved);
    project.fail_first("pr merge *", "Pull request #7 has failing required checks");
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("failing required checks"), "{text}");
    assert!(!text.contains("zaf do workspace-drop"), "{text}");
    assert!(!called(&project, "issue edit"), "{text}");
    assert!(dir.is_dir());
    assert!(record(&project, "2").is_some());
}

#[test]
fn slice_merge_outside_land_refuses() {
    // Claimed, no slice: `claim`.
    let project = project();
    assert_not_run_in(&project, "claim");

    // A slice open and nothing published: `build`.
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    assert_not_run_in(&project, "build");
    assert!(dir.is_dir());
}

#[test]
fn slice_merge_ahead_of_the_head_refuses() {
    let (project, dir, head) = landing();
    // A commit past the pull request's head: the clean result is for an older commit.
    write(&dir, "more.csv", "c,d\n");
    jj(&dir, &["commit", "-m", "One more thing"]);
    assert_ne!(commit_field(&dir, "@-", "commit_id"), head);

    assert_not_run_in(&project, "build");

    assert!(dir.is_dir());
    assert_eq!(remote_slice(&project), head);
}

#[test]
fn slice_merge_on_a_done_task_refuses() {
    let (mut project, dir, _) = landing();
    // The board has the task done already.
    project.on_first("issue view 2 *", DONE);

    assert_not_run_in(&project, "done");

    assert!(dir.is_dir());
    assert!(!called(&project, "issue edit"));
}

#[test]
fn claim_to_done_walks_the_phases() {
    let mut project = project();
    // `slice-merge` runs only in `land`, so its refusal names the phase the task stands in.
    assert_not_run_in(&project, "claim");

    let dir = open(&project);
    assert_not_run_in(&project, "build");

    // A task check has passed on the work: `check`.
    write(&dir, "writer.csv", "a,b\n");
    record_check(&project, "clean");
    assert_not_run_in(&project, "check");

    // Published with the pull request recorded, no step run on it: `publish`.
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    let head = commit_field(&dir, "@-", "commit_id");
    forge_at(&mut project, &head);
    assert_not_run_in(&project, "publish");

    // `check pr` ran and a step failed: `review`.
    pr_check(&project, "step-failed", &head);
    assert_not_run_in(&project, "review");

    // `check pr` is clean for the head the workspace is on: `land`, and the run lands it.
    pr_check(&project, "clean", &head);
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(called(&project, "pr merge 7 --merge"), "{text}");
    assert!(!dir.exists());

    // The board shows `done` now.
    project.on_first("issue view 2 *", DONE);
    assert_not_run_in(&project, "done");
}

#[test]
fn must_fix_round_returns_to_build() {
    let mut project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    let head = commit_field(&dir, "@-", "commit_id");
    forge_at(&mut project, &head);

    // `check pr` found something to fix on the head: back to `build`.
    pr_check(&project, "must-fix", &head);
    assert_not_run_in(&project, "build");

    // The fix is published as a new head, which no `check pr` has seen: `publish`.
    write(&dir, "writer.csv", "a,b\n1,2\n");
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    let fixed = commit_field(&dir, "@-", "commit_id");
    assert_ne!(fixed, head);
    forge_at(&mut project, &fixed);
    assert_not_run_in(&project, "publish");

    // `check` is only passed through when a task check ran, later than the pull request.
    write(
        &project.root,
        ".zaf/runs/check-task-2.json",
        r#"{"target":"task #2","verdict":"clean","lines":[],"at":"2099-01-01T00:00:00Z"}"#,
    );
    assert_not_run_in(&project, "check");
    fs::remove_file(project.root.join(".zaf/runs/check-task-2.json")).expect("remove");
    assert_not_run_in(&project, "publish");

    // A clean `check pr` on the fixed head: `land`.
    pr_check(&project, "clean", &fixed);
    let (code, text) = action(&project, &["slice-merge", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(called(&project, "pr merge 7 --merge"), "{text}");
}

#[test]
fn slice_open_hands_back_the_path() {
    let project = project();

    let (code, text) = action(&project, &["slice-open", "2"]);

    assert_eq!(code, 0, "{text}");
    // The last line is the workspace, an absolute path of the directory that now exists.
    let path = handed_back(&text);
    let dir = workspace(&project);
    assert_eq!(
        Path::new(path).canonicalize().expect("the path exists"),
        dir,
        "{text}"
    );
    assert!(
        dir.join("notes.txt").is_file(),
        "the workspace holds the base"
    );
    // zaf kept it as the task's workspace record, and the slice's bookmark sits on the base.
    assert_eq!(record(&project, "2").as_deref(), Some(path), "{text}");
    assert_eq!(
        bookmark_at(&project.root, SLICE),
        bookmark_at(&project.root, BASE)
    );
}

#[test]
fn slice_open_past_claim_refuses() {
    let project = project();
    let dir = open(&project);
    let opened = record(&project, "2");

    // The slice is open: the task is in `build`, past `claim`.
    let (code, text) = action(&project, &["slice-open", "2"]);

    assert_eq!(code, 125, "{text}");
    assert!(text.contains("NOT_RUN slice-open: "), "{text}");
    assert!(text.contains("2 is in build, not claim"), "{text}");
    assert_eq!(record(&project, "2"), opened);
    assert!(dir.is_dir());

    // A task nobody has claimed is before `claim`, and no slice is made for it.
    let unclaimed = project_with(READY);
    let (code, text) = action(&unclaimed, &["slice-open", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(text.contains("2 is in unclaimed, not claim"), "{text}");
    assert!(!workspace(&unclaimed).exists());
    assert_eq!(record(&unclaimed, "2"), None);
}

#[test]
fn slice_pr_skips_an_empty_commit() {
    let project = project();
    let dir = open(&project);
    // The work is already committed: the working copy holds nothing more.
    write(&dir, "writer.csv", "a,b\n");
    jj(&dir, &["commit", "-m", "Add the CSV writer"]);
    let committed = commit_field(&dir, "@-", "commit_id");

    let (code, text) = action(&project, &["slice-pr", "2"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    // No commit step ran and none was made: the work is as it was, pushed under the bookmark.
    assert!(!text.contains("zaf do commit"), "{text}");
    assert!(text.contains("zaf do push"), "{text}");
    assert!(text.contains("zaf do pr-open"), "{text}");
    assert_eq!(commit_field(&dir, "@-", "commit_id"), committed);
    assert_eq!(
        commit_field(&dir, "@-", "description").trim(),
        "Add the CSV writer"
    );
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
    assert_eq!(record(&project, "2.pr").as_deref(), Some("7"));
}

#[test]
fn slice_pr_titles_the_request() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");

    let (code, text) = action(&project, &["slice-pr", "2"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    // What was left is committed under the title, which is the task's with its number.
    assert!(text.contains("zaf do commit"), "{text}");
    assert_eq!(
        commit_field(&dir, "@-", "description").trim(),
        "Export the list (#2)"
    );
    assert_eq!(
        commit_field(&dir, "@", "empty"),
        "true",
        "the work is all committed"
    );
    assert_eq!(
        bookmark_at(&project.root, SLICE),
        commit_field(&dir, "@-", "commit_id")
    );
    // The request names the slice, the base, the title and the task it refers to.
    let create = project
        .gh_calls()
        .into_iter()
        .find(|call| call.starts_with("pr create"))
        .expect("a pr create call");
    assert!(
        create.contains(&format!(
            "--head {SLICE} --base {BASE} --title Export the list (#2) --body Refs #2."
        )),
        "{create}"
    );
    assert_eq!(record(&project, "2.pr").as_deref(), Some("7"));
}

#[test]
fn slice_drop_keeps_the_branch() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["commit", SLICE, "Add the CSV writer"]);
    assert_eq!(code, 0, "{text}");
    let committed = bookmark_at(&project.root, SLICE);
    assert_ne!(committed, bookmark_at(&project.root, BASE));

    let (code, text) = action(&project, &["slice-drop", "2"]);

    assert_eq!(code, 0, "{text}");
    // The workspace and the task's records go; the branch with its work stays.
    assert!(!dir.exists(), "the workspace is still there");
    assert_eq!(record(&project, "2"), None);
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
    // Dropped is back in `claim`: another drop has no slice to act on.
    let (code, text) = action(&project, &["slice-drop", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(text.contains("2 is in claim"), "{text}");
}

#[test]
fn drop_in_check_then_reopen() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["commit", SLICE, "Add the CSV writer"]);
    assert_eq!(code, 0, "{text}");
    let committed = bookmark_at(&project.root, SLICE);
    // The task's checks have passed: it is in `check`, where a drop is allowed (ruling 27).
    record_check(&project, "clean");

    let (code, text) = action(&project, &["slice-drop", "2"]);

    assert_eq!(code, 0, "{text}");
    // Everything zaf kept of the slice is forgotten, so the task is back in `claim`.
    assert!(!dir.exists());
    assert_eq!(record(&project, "2"), None);
    assert!(!project.root.join(".zaf/runs/check-task-2.json").exists());

    // And it opens again: a fresh workspace on the branch, with the work the drop kept.
    let (code, text) = action(&project, &["slice-open", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(dir.is_dir(), "{text}");
    assert_eq!(
        Path::new(handed_back(&text))
            .canonicalize()
            .expect("the path exists"),
        dir
    );
    assert!(record(&project, "2").is_some());
    assert_eq!(
        fs::read_to_string(dir.join("writer.csv")).expect("the kept work"),
        "a,b\n"
    );
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
}

#[test]
fn slice_pr_runs_from_build() {
    // Before the slice is open the task is in `claim`: nothing to publish.
    let project = project();
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(
        text.contains("2 is in claim, not build, check, publish, review"),
        "{text}"
    );
    assert!(
        project
            .gh_calls()
            .iter()
            .all(|call| !call.starts_with("pr "))
    );

    // From `build` it publishes, though no check has run: CI is the only gate (ruling 39).
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");

    // A failed task check keeps the task in `build` (ruling 26), and it publishes then too.
    let failed = project_with(CLAIMED);
    let dir = open(&failed);
    write(&dir, "writer.csv", "a,b\n");
    record_check(&failed, "must-fix");
    let (code, text) = action(&failed, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    assert_eq!(record(&failed, "2.pr").as_deref(), Some("7"));
}
