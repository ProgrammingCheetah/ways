//! The ways repository's `sync` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote, it fetches and starts a new change on the base as
//! it stands there. A failed fetch exits 2 and leaves the working copy where it was.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const BASE: &str = "main";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[vcs]
kind = "jj"
"#;

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/sync");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote and the `sync`
/// action in its actions folder.
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
    // The action is the project's own file, so it sits in the base: the working copy moves
    // between commits when the base advances, and must not lose it.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\n");
    let actions = root.join(".zaf/actions");
    write(&actions, "sync", &action_text());
    fs::set_permissions(actions.join("sync"), fs::Permissions::from_mode(0o750)).expect("mode");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// `zaf do sync <args>` with its whole log: the exit code and the lines.
fn sync_with(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "sync"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn sync(project: &Project) -> (i32, String) {
    sync_with(project, &[BASE])
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

/// The commit the root's working copy sits on.
fn working_copy(project: &Project) -> String {
    commit(&project.root, "@")
}

fn bookmark_at(project: &Project, name: &str) -> String {
    commit(&project.root, &format!("bookmarks(exact:\"{name}\")"))
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

/// The commit the working copy's parent is, and whether the working copy is empty.
fn parent_and_empty(project: &Project) -> (String, String) {
    (
        commit(&project.root, "@-"),
        jj(
            &project.root,
            &[
                "--ignore-working-copy",
                "log",
                "--no-graph",
                "-r",
                "@",
                "-T",
                "empty",
            ],
        ),
    )
}

#[test]
fn sync_fetches_and_starts_a_new_change_on_the_moved_base() {
    let project = project();
    let before = bookmark_at(&project, BASE);
    advance_base_elsewhere(&project, "elsewhere.txt", "e\n");

    let (code, text) = sync(&project);

    assert_eq!(code, 0, "{text}");
    let moved = bookmark_at(&project, BASE);
    assert_ne!(moved, before, "the fetch did not move the base");
    let (parent, empty) = parent_and_empty(&project);
    assert_eq!(parent, moved);
    assert_eq!(empty.trim(), "true");
    assert!(project.root.join("elsewhere.txt").is_file());

    // Nothing new on the remote: a second run starts another change on the same base.
    let (code, text) = sync(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(parent_and_empty(&project).0, moved);
}

#[test]
fn sync_leaves_work_in_the_old_change_alone() {
    let project = project();
    write(&project.root, "wip.txt", "wip\n");
    advance_base_elsewhere(&project, "elsewhere.txt", "e\n");

    let (code, text) = sync(&project);

    assert_eq!(code, 0, "{text}");
    // The work was snapshotted into its change, which is no longer the working copy.
    assert!(!project.root.join("wip.txt").exists());
    let holders = jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            "files(\"wip.txt\") ~ @",
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    );
    assert_eq!(holders.lines().count(), 1, "{holders}");
}

#[test]
fn sync_failed_fetch_stays() {
    let project = project();
    write(&project.root, "wip.txt", "wip\n");
    jj(&project.root, &["status"]);
    let at = working_copy(&project);
    let base = bookmark_at(&project, BASE);
    fs::remove_dir_all(remote_dir(&project)).expect("remove the remote");

    let (code, text) = sync(&project);

    assert_eq!(code, 2, "{text}");
    assert_eq!(working_copy(&project), at, "the working copy moved");
    assert_eq!(bookmark_at(&project, BASE), base);
    assert!(project.root.join("wip.txt").is_file());
}

#[test]
fn sync_refuses_with_1_what_it_cannot_use() {
    let project = project();
    // Snapshot first: any jj run does, so the commit is settled before the runs below.
    jj(&project.root, &["status"]);
    let at = working_copy(&project);

    for args in [
        &[][..],
        &[BASE, "extra"][..],
        &[""][..],
        &["-x"][..],
        &["a\"b"][..],
        // A base the repository has never heard of.
        &["feature/unknown"][..],
    ] {
        let (code, text) = sync_with(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    assert_eq!(working_copy(&project), at);
}
