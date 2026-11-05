//! The ways repository's `land` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote and a fake `gh`, `land <pr>` runs `zaf do pr-merge
//! <pr>` and then `zaf do sync main`, each with its own log. A refused merge exits 1 and does not
//! sync.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "main";

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

/// The actions `land` is built from, and `land` itself.
const ACTIONS: [&str; 3] = ["pr-merge", "sync", "land"];

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote, the three
/// actions in its actions folder, and the built `zaf` on `PATH` so `land` can call it.
fn project() -> Project {
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
    // The actions are the project's own files, so they sit in the base: the working copy moves
    // between commits when the base advances, and must not lose them.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\n");
    let actions = root.join(".zaf/actions");
    for name in ACTIONS {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

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
    project
}

/// `zaf do land <args>` with its whole log: the exit code and the lines.
fn land(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "land"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn remote_dir(project: &Project) -> PathBuf {
    project.temp.path().join("remote.git")
}

/// The commit a revset names in the root repository.
fn commit(root: &Path, revset: &str) -> String {
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

fn bookmark_at(project: &Project, name: &str) -> String {
    commit(&project.root, &format!("bookmarks(exact:\"{name}\")"))
}

/// Pushes a commit to the base from a second clone, as a merged pull request would, so the
/// root's local and remote-tracking base both lag until something fetches.
fn advance_base_elsewhere(project: &Project) {
    let temp = project.temp.path();
    let remote = remote_dir(project);
    run(
        temp,
        "git",
        &["clone", "-q", &remote.to_string_lossy(), "other"],
    );
    let other = temp.join("other");
    run(&other, "git", &["checkout", "-q", BASE]);
    write(&other, "merged.txt", "merged\n");
    run(&other, "git", &["add", "merged.txt"]);
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
            "The merged pull request",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", BASE]);
}

/// The names of the logs the runs wrote, `<DATETIME>_` dropped, sorted.
fn logs(project: &Project) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(project.root.join(".zaf/logs"))
        .map(|entries| {
            entries
                .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
                .map(|name: String| {
                    name.split_once('_')
                        .map_or(name.clone(), |(_, rest)| rest.to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn view_call(pr: &str) -> String {
    format!("pr view {pr} --json state --jq .state --repo {REPO}")
}

fn merge_call(pr: &str) -> String {
    format!("pr merge {pr} --merge --repo {REPO}")
}

#[test]
fn land_merges_then_syncs() {
    let mut project = project();
    project.on("pr view *", "OPEN\n").on("pr merge *", "");
    let before = bookmark_at(&project, BASE);
    advance_base_elsewhere(&project);

    let (code, text) = land(&project, &["7"]);

    assert_eq!(code, 0, "{text}");
    // The merge was asked for, then the sync fetched the base the merge moved.
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7")],
        "{text}"
    );
    let moved = bookmark_at(&project, BASE);
    assert_ne!(moved, before, "the sync did not fetch the base");
    assert_eq!(commit(&project.root, "@-"), moved);
    assert!(project.root.join("merged.txt").is_file());

    // Each step has its own log, beside the log of `land`; the outer log names both.
    assert_eq!(
        logs(&project),
        ["land.log", "pr-merge.log", "sync.log"],
        "{text}"
    );
    assert!(text.contains("zaf do pr-merge: log "), "{text}");
    assert!(text.contains("zaf do sync: log "), "{text}");
    assert!(
        text.find("zaf do pr-merge").expect("merge") < text.find("zaf do sync").expect("sync"),
        "the merge did not come first: {text}"
    );
}

#[test]
fn land_stops_on_a_refused_merge() {
    let mut project = project();
    project
        .on("pr view *", "OPEN\n")
        .fail_on("pr merge *", "Pull request #7 has failing required checks");
    advance_base_elsewhere(&project);
    // Snapshot first: any jj run does, so the commit is settled before the run below.
    jj(&project.root, &["status"]);
    let at = commit(&project.root, "@");
    let base = bookmark_at(&project, BASE);

    let (code, text) = land(&project, &["7"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("failing required checks"), "{text}");
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7")],
        "{text}"
    );
    // No sync ran: no log, no fetch, the working copy and the base where they were.
    assert_eq!(logs(&project), ["land.log", "pr-merge.log"], "{text}");
    assert_eq!(commit(&project.root, "@"), at);
    assert_eq!(bookmark_at(&project, BASE), base);
    assert!(!project.root.join("merged.txt").exists());
}

/// Replaces an action with one that has no description, which zaf does not run: `NOT_RUN`, 125.
fn break_action(project: &Project, name: &str) {
    let actions = project.root.join(".zaf/actions");
    write(&actions, name, "#!/bin/sh\ntrue\n");
    fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
}

#[test]
fn land_maps_a_nested_not_run_to_two() {
    // A merge that did not run: land declares 1 and 2, never 125, so it is a failed backend, and
    // no sync follows.
    let project = project();
    break_action(&project, "pr-merge");
    let (code, text) = land(&project, &["7"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("NOT_RUN pr-merge"), "{text}");
    assert!(
        !logs(&project).iter().any(|name| name == "sync.log"),
        "{text}"
    );
    assert_eq!(project.gh_calls(), Vec::<String>::new());

    // The same for a sync that did not run, after the merge was made.
    let mut project = self::project();
    project.on("pr view *", "OPEN\n").on("pr merge *", "");
    break_action(&project, "sync");
    let (code, text) = land(&project, &["7"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("NOT_RUN sync"), "{text}");
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7")],
        "{text}"
    );
}

#[test]
fn land_refuses_with_1_what_it_cannot_use() {
    let project = project();

    for args in [&[][..], &["7", "8"][..], &["seven"][..], &["-7"][..]] {
        let (code, text) = land(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    // Refused before any step: gh was never called, and no sync ran.
    assert_eq!(project.gh_calls(), Vec::<String>::new());
    assert!(!logs(&project).iter().any(|name| name == "sync.log"));
}
