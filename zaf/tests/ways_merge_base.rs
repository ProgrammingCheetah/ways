//! The ways repository's `merge-base` action (docs/zaf-actions.md §3, §4, §7): run through `zaf
//! do` on a throwaway jj repository with a bare remote, it fetches and merges the base as it
//! stands on the remote into a slice. Up to date and merged exit 0, a conflict exits 1 with the
//! paths in the log, and uncommitted work or a failed fetch exits 2; the last two leave the
//! slice as it was.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[vcs]
kind = "jj"
"#;

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote, the
/// `workspace-open` and `merge-base` actions in its actions folder and the slice open on the base.
fn project() -> Project {
    let project = Project::new(MANIFEST);
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
    // The actions are the project's own files, so they sit in the base: the root's working copy
    // moves between commits when the base advances, and must not lose them.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\ntwo\nthree\n");
    let actions = root.join(".zaf/actions");
    for name in ["workspace-open", "merge-base"] {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "workspace-open", SLICE, BASE]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// `zaf do merge-base <args>` with its whole log: the exit code and the lines.
fn merge_with(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "merge-base"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn merge(project: &Project) -> (i32, String) {
    merge_with(project, &[SLICE, BASE])
}

fn slice_dir(project: &Project) -> PathBuf {
    project.root.join(".zaf/workspaces").join(SLICE)
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

/// The commit the remote holds for the base.
fn remote_base(project: &Project) -> String {
    let git_dir = remote_dir(project);
    let reference = format!("refs/heads/{BASE}");
    let stdout = run(
        &project.root,
        "git",
        &[
            "--git-dir",
            &git_dir.to_string_lossy(),
            "rev-parse",
            &reference,
        ],
    );
    stdout.trim().to_owned()
}

/// Commits a file on the base from the root's own working copy and pushes it.
fn advance_base(project: &Project, path: &str, content: &str) {
    let root = &project.root;
    jj(root, &["new", &format!("bookmarks(exact:\"{BASE}\")")]);
    write(root, path, content);
    jj(root, &["commit", "-m", "Advance the base"]);
    jj(root, &["bookmark", "set", BASE, "-r", "@-"]);
    jj(root, &["git", "push", "--bookmark", BASE]);
}

/// Pushes a commit to the base from a second clone, so the root's local and remote-tracking
/// base both lag until something fetches.
fn advance_base_elsewhere(project: &Project, path: &str, content: &str) {
    let temp = project.temp.path();
    let remote = remote_dir(project);
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
            "Advance the base elsewhere",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", BASE]);
}

/// Commits a file of the slice's own in its workspace and moves its bookmark onto the commit.
fn commit_in_slice(project: &Project, path: &str, content: &str) {
    let dir = slice_dir(project);
    write(&dir, path, content);
    jj(&dir, &["commit", "-m", "Slice work"]);
    jj(&dir, &["bookmark", "set", SLICE, "-r", "@-"]);
}

/// What a slice shows from outside: its bookmark, the commit its working copy sits on, whether
/// that working copy is empty, and the notes file.
fn state(project: &Project) -> (String, String, String, String) {
    let dir = slice_dir(project);
    (
        bookmark_at(project, SLICE),
        jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]),
        jj(&dir, &["log", "--no-graph", "-r", "@", "-T", "empty"]),
        fs::read_to_string(dir.join("notes.txt")).expect("notes"),
    )
}

#[test]
fn merge_base_merges_a_moved_base_and_is_then_up_to_date() {
    let project = project();

    // The slice is at the base: nothing to merge.
    let (code, text) = merge(&project);
    assert_eq!(code, 0, "{text}");

    advance_base(&project, "base-1.txt", "1\n");
    let (code, text) = merge(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(&project, SLICE), bookmark_at(&project, BASE));
    assert!(slice_dir(&project).join("base-1.txt").is_file());

    let before = state(&project);
    let (code, text) = merge(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(state(&project), before);
}

#[test]
fn merge_base_merges_into_a_slice_with_work_of_its_own() {
    let project = project();
    commit_in_slice(&project, "slice.txt", "s\n");
    let own = bookmark_at(&project, SLICE);
    advance_base(&project, "base-1.txt", "1\n");

    let (code, text) = merge(&project);

    assert_eq!(code, 0, "{text}");
    let merged = bookmark_at(&project, SLICE);
    assert_ne!(merged, own);
    let parents = jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            &format!("bookmarks(exact:\"{SLICE}\")-"),
            "-T",
            r#"commit_id ++ "\n""#,
        ],
    );
    assert_eq!(parents.lines().count(), 2, "{parents}");
    assert!(slice_dir(&project).join("slice.txt").is_file());
    assert!(slice_dir(&project).join("base-1.txt").is_file());
    // The working copy is clean on the merge, so a second run changes nothing.
    let (code, text) = merge(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(&project, SLICE), merged);
}

#[test]
fn merge_base_merges_the_base_as_the_remote_has_it_and_moves_a_lagging_local_base_up() {
    let project = project();
    let before = bookmark_at(&project, BASE);
    advance_base_elsewhere(&project, "elsewhere.txt", "e\n");
    let pushed = remote_base(&project);
    assert_ne!(pushed, before);
    assert_eq!(bookmark_at(&project, BASE), before);

    let (code, text) = merge(&project);

    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(&project, BASE), pushed);
    assert_eq!(bookmark_at(&project, SLICE), pushed);
    assert!(slice_dir(&project).join("elsewhere.txt").is_file());
}

#[test]
fn merge_base_never_moves_a_local_base_that_is_ahead_of_the_remote() {
    let project = project();
    let root = &project.root;
    // The base moves on locally, unpushed: the remote's base is behind it.
    jj(root, &["new", &format!("bookmarks(exact:\"{BASE}\")")]);
    write(root, "local.txt", "l\n");
    jj(root, &["commit", "-m", "Local only"]);
    jj(root, &["bookmark", "set", BASE, "-r", "@-"]);
    let local = bookmark_at(&project, BASE);
    let remote = remote_base(&project);
    assert_ne!(local, remote);
    let before = state(&project);

    let (code, text) = merge(&project);

    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(&project, BASE), local, "the local base moved");
    assert_eq!(remote_base(&project), remote);
    assert_eq!(state(&project), before);
    assert!(!slice_dir(&project).join("local.txt").exists());
}

#[test]
fn merge_base_conflict_exits_1_unchanged() {
    let project = project();
    commit_in_slice(&project, "notes.txt", "one\nslice\nthree\n");
    let before = state(&project);
    advance_base(&project, "notes.txt", "one\nbase\nthree\n");

    let (code, text) = merge(&project);

    assert_eq!(code, 1, "{text}");
    // The paths are in the log.
    assert!(text.contains("notes.txt"), "{text}");
    assert!(text.contains("conflict"), "{text}");
    // The slice is as it was: the same bookmark and parent, a clean working copy, the same file.
    assert_eq!(state(&project), before);
    assert_eq!(before.2.trim(), "true");

    // The base was still fetched, and a second run conflicts the same way.
    let (code, text) = merge(&project);
    assert_eq!(code, 1, "{text}");
    assert_eq!(state(&project), before);
}

#[test]
fn merge_base_failed_fetch_exits_2() {
    let project = project();
    commit_in_slice(&project, "slice.txt", "s\n");
    let before = state(&project);
    fs::remove_dir_all(remote_dir(&project)).expect("remove the remote");

    let (code, text) = merge(&project);

    assert_eq!(code, 2, "{text}");
    assert_eq!(state(&project), before);
    assert!(slice_dir(&project).join("slice.txt").is_file());
}

#[test]
fn merge_base_uncommitted_work_exits_2() {
    let project = project();
    write(&slice_dir(&project), "wip.txt", "wip\n");
    advance_base(&project, "base-1.txt", "1\n");
    let before = bookmark_at(&project, SLICE);

    let (code, text) = merge(&project);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("uncommitted"), "{text}");
    assert_eq!(bookmark_at(&project, SLICE), before);
    assert!(slice_dir(&project).join("wip.txt").is_file());
    assert!(!slice_dir(&project).join("base-1.txt").exists());
}

#[test]
fn merge_base_refuses_with_2_what_it_cannot_use() {
    let project = project();

    for args in [
        &[][..],
        &[SLICE][..],
        &[SLICE, BASE, "extra"][..],
        &["", BASE][..],
        &["../outside", BASE][..],
        &["-x", BASE][..],
        &[SLICE, ""][..],
        &[SLICE, "-x"][..],
        // A slice that was never opened, and one open on another base.
        &["feature/v1-9-9", BASE][..],
        &[SLICE, "feature/v2"][..],
    ] {
        let (code, text) = merge_with(&project, args);
        assert_eq!(code, 2, "{args:?}: {text}");
    }
}
