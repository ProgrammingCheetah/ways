//! The ways repository's `base-up` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote, `base-up <branch> [<base>]` brings the branch up to
//! the base as the remote has them and pushes it, and the working copy never moves: a
//! fast-forward when the branch has nothing of its own, else a merge commit. Up to date is quiet
//! and pushes nothing. A conflicting merge, a local branch that differs from the remote's, a
//! branch the remote lacks and a name it cannot use exit 1 with nothing moved.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const BASE: &str = "main";
const BRANCH: &str = "feature/v1";

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
        .join(".zaf/actions/base-up");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base and the branch pushed to a bare remote,
/// both at the first commit, and `base-up` in its actions folder.
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
    let remote = remote_dir(&project);
    jj(
        &root,
        &["git", "remote", "add", "origin", &remote.to_string_lossy()],
    );
    // The action is the project's own file, so it sits in the base.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\ntwo\nthree\n");
    let actions = root.join(".zaf/actions");
    write(&actions, "base-up", &action_text());
    fs::set_permissions(actions.join("base-up"), fs::Permissions::from_mode(0o750)).expect("mode");
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run,
    // and before the first commit so the `.zaf/.gitignore` it writes is in every commit: a
    // working copy moved onto a commit without it would lose it, and the next run would write
    // it into the working copy again.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["bookmark", "create", BRANCH, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);
    jj(&root, &["git", "push", "--bookmark", BRANCH]);
    project
}

/// `zaf do --more base-up <args>`: the exit code, the whole log (stdout) and everything printed.
fn base_up_with(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "base-up"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    let text = format!("{stdout}{stderr}");
    (code, stdout, text)
}

fn base_up(project: &Project) -> (i32, String, String) {
    base_up_with(project, &[BRANCH])
}

/// The last non-empty line of a log: what the action handed back.
fn last_line(log: &str) -> &str {
    log.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
}

fn remote_dir(project: &Project) -> PathBuf {
    project.temp.path().join("remote.git")
}

/// Runs git on the bare remote.
fn remote_git(project: &Project, args: &[&str]) -> String {
    let git_dir = remote_dir(project);
    let mut full_args = vec!["--git-dir", git_dir.to_str().expect("a UTF-8 path")];
    full_args.extend_from_slice(args);
    run(project.temp.path(), "git", &full_args)
}

/// The commit the remote holds for a branch, if it has the branch.
fn remote_ref(project: &Project, branch: &str) -> Option<String> {
    let reference = format!("refs/heads/{branch}");
    let found = remote_git(
        project,
        &["for-each-ref", "--format=%(objectname)", &reference],
    );
    let found = found.trim();
    (!found.is_empty()).then(|| found.to_owned())
}

/// The parents of the commit the remote holds for a branch, in order.
fn remote_parents(project: &Project, branch: &str) -> Vec<String> {
    let reference = format!("refs/heads/{branch}");
    remote_git(project, &["log", "-1", "--format=%P", &reference])
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The first line of the message of the commit the remote holds for a branch.
fn remote_subject(project: &Project, branch: &str) -> String {
    let reference = format!("refs/heads/{branch}");
    remote_git(project, &["log", "-1", "--format=%s", &reference])
        .trim()
        .to_owned()
}

/// The commits a revset names in the root repository, one per line.
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

/// The commit the root's working copy is and the one it sits on, after a snapshot so a file on
/// disk is in them.
fn working_copy(project: &Project) -> (String, String) {
    jj(&project.root, &["status"]);
    (commit(&project.root, "@"), commit(&project.root, "@-"))
}

/// A second clone of the remote, made on first use and fetched after.
fn elsewhere(project: &Project) -> PathBuf {
    let temp = project.temp.path();
    let other = temp.join("other");
    if other.exists() {
        run(&other, "git", &["fetch", "-q", "origin"]);
    } else {
        let remote = remote_dir(project);
        run(
            temp,
            "git",
            &["clone", "-q", &remote.to_string_lossy(), "other"],
        );
    }
    other
}

/// Commits a file on `branch` from the second clone and pushes it, as a merged pull request would,
/// so the root's bookmarks lag until something fetches. A branch the remote lacks starts at the
/// remote's base.
fn push_elsewhere(project: &Project, branch: &str, path: &str, content: &str) {
    let start = if remote_ref(project, branch).is_some() {
        format!("origin/{branch}")
    } else {
        format!("origin/{BASE}")
    };
    let other = elsewhere(project);
    run(&other, "git", &["checkout", "-q", "-B", branch, &start]);
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
            &format!("Work on {branch} elsewhere"),
        ],
    );
    run(&other, "git", &["push", "-q", "origin", branch]);
}

/// Commits a file on the branch from the root and pushes it: the branch's own work. The working
/// copy is left on the new commit.
fn push_on_branch(project: &Project, path: &str, content: &str) {
    let root = &project.root;
    jj(root, &["new", &format!("bookmarks(exact:\"{BRANCH}\")")]);
    write(root, path, content);
    jj(root, &["commit", "-m", "Work on the branch"]);
    jj(root, &["bookmark", "set", BRANCH, "-r", "@-"]);
    jj(root, &["git", "push", "--bookmark", BRANCH]);
}

/// From here on, every ref update the remote receives is recorded: a pre-receive hook appends
/// its `<old> <new> <ref>` lines to a file beside the remote.
fn watch_pushes(project: &Project) {
    let record = project.temp.path().join("pushes");
    let hook = remote_dir(project).join("hooks/pre-receive");
    write(
        hook.parent().expect("the hooks folder"),
        "pre-receive",
        &format!("#!/bin/sh\ncat >> '{}'\n", record.display()),
    );
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// The ref updates the remote received since `watch_pushes`.
fn pushes(project: &Project) -> Vec<String> {
    fs::read_to_string(project.temp.path().join("pushes"))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Nothing in the log says a commit was made, a bookmark moved or a push was prepared.
fn assert_quiet(text: &str) {
    for said in ["Created new commit", "Moved ", "Changes to push"] {
        assert!(!text.contains(said), "{text}");
    }
}

/// The only push was the branch's, from `from` to `to`.
fn assert_pushed_once(project: &Project, from: &str, to: &str, text: &str) {
    assert_eq!(
        pushes(project),
        [format!("{from} {to} refs/heads/{BRANCH}")],
        "{text}"
    );
}

#[test]
fn base_up_fast_forwards_a_branch_with_nothing_of_its_own() {
    let project = project();
    let before = working_copy(&project);
    let own = remote_ref(&project, BRANCH).expect("the branch");
    push_elsewhere(&project, BASE, "merged.txt", "merged\n");
    let base = remote_ref(&project, BASE).expect("the base");
    assert_ne!(base, own);
    watch_pushes(&project);

    let (code, log, text) = base_up(&project);

    assert_eq!(code, 0, "{text}");
    // The branch's tip is now the base's, on the remote and locally, and is handed back.
    assert_eq!(
        remote_ref(&project, BRANCH).as_deref(),
        Some(&*base),
        "{text}"
    );
    assert_eq!(bookmark_at(&project, BRANCH), base);
    assert_eq!(last_line(&log), base, "{text}");
    assert_pushed_once(&project, &own, &base, &text);
    // No merge commit was made: the base's commit is the branch's.
    assert_eq!(remote_parents(&project, BRANCH).len(), 1);
    // The working copy is where it was, the base's new file not on disk.
    assert_eq!(working_copy(&project), before);
    assert!(!project.root.join("merged.txt").exists());
}

#[test]
fn base_up_merges_the_base_into_a_branch_that_moved_too() {
    let project = project();
    push_on_branch(&project, "branch.txt", "branch\n");
    let before = working_copy(&project);
    let own = remote_ref(&project, BRANCH).expect("the branch");
    push_elsewhere(&project, BASE, "merged.txt", "merged\n");
    let base = remote_ref(&project, BASE).expect("the base");
    watch_pushes(&project);

    let (code, log, text) = base_up(&project);

    assert_eq!(code, 0, "{text}");
    // The remote's branch is a merge commit of the branch and then the base.
    let merged = remote_ref(&project, BRANCH).expect("the branch");
    assert_ne!(merged, own);
    assert_ne!(merged, base);
    assert_eq!(remote_parents(&project, BRANCH), [own.clone(), base]);
    assert_eq!(
        remote_subject(&project, BRANCH),
        format!("Merge {BASE} into {BRANCH}")
    );
    assert_eq!(bookmark_at(&project, BRANCH), merged);
    assert_eq!(last_line(&log), merged, "{text}");
    assert_pushed_once(&project, &own, &merged, &text);
    // The working copy is where it was, on the branch's own commit, the base's file not on disk.
    assert_eq!(working_copy(&project), before);
    assert!(project.root.join("branch.txt").is_file());
    assert!(!project.root.join("merged.txt").exists());

    // The branch now has the base: a second run is up to date, hands back the merge and pushes
    // nothing.
    let (code, log, text) = base_up(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), merged, "{text}");
    assert_quiet(&text);
    assert_eq!(pushes(&project).len(), 1, "{text}");
    assert_eq!(remote_ref(&project, BRANCH).as_deref(), Some(&*merged));
}

#[test]
fn base_up_is_quiet_and_pushes_nothing_when_the_branch_has_the_base() {
    let project = project();
    watch_pushes(&project);

    // The branch is at the base.
    let at = remote_ref(&project, BRANCH).expect("the branch");
    let (code, log, text) = base_up(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), at, "{text}");
    assert_quiet(&text);
    assert_eq!(pushes(&project), Vec::<String>::new(), "{text}");
    assert_eq!(remote_ref(&project, BRANCH).as_deref(), Some(&*at));

    // The branch is ahead of the base: still nothing to bring in.
    push_on_branch(&project, "branch.txt", "branch\n");
    let own = remote_ref(&project, BRANCH).expect("the branch");
    let before = working_copy(&project);
    let pushed = pushes(&project);
    let (code, log, text) = base_up(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), own, "{text}");
    assert_quiet(&text);
    assert_eq!(pushes(&project), pushed, "{text}");
    assert_eq!(remote_ref(&project, BRANCH).as_deref(), Some(&*own));
    assert_eq!(bookmark_at(&project, BRANCH), own);
    assert_eq!(working_copy(&project), before);
}

#[test]
fn base_up_conflict_exits_1_and_moves_nothing() {
    let project = project();
    push_on_branch(&project, "notes.txt", "one\nbranch\nthree\n");
    push_elsewhere(&project, BASE, "notes.txt", "one\nbase\nthree\n");
    let before = working_copy(&project);
    let own = remote_ref(&project, BRANCH).expect("the branch");
    let base = remote_ref(&project, BASE).expect("the base");
    watch_pushes(&project);

    let (code, _, text) = base_up(&project);

    assert_eq!(code, 1, "{text}");
    // The conflicted path is in the log.
    assert!(text.contains("conflicts in:"), "{text}");
    assert!(text.contains("notes.txt"), "{text}");
    // Nothing moved: the remote, the local branch and the working copy are as they were.
    assert_eq!(remote_ref(&project, BRANCH).as_deref(), Some(&*own));
    assert_eq!(remote_ref(&project, BASE).as_deref(), Some(&*base));
    assert_eq!(bookmark_at(&project, BRANCH), own);
    assert_eq!(pushes(&project), Vec::<String>::new(), "{text}");
    assert_eq!(working_copy(&project), before);
    assert_eq!(
        fs::read_to_string(project.root.join("notes.txt")).expect("notes"),
        "one\nbranch\nthree\n"
    );
    // No merge commit was left behind.
    let merges = commit(&project.root, "description(substring:\"Merge \")");
    assert_eq!(merges, "", "a merge commit was left behind");
    let children = commit(
        &project.root,
        &format!("children({own}) & children({base})"),
    );
    assert_eq!(children, "", "a merge commit was left behind");

    // A second run conflicts the same way.
    let (code, _, text) = base_up(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("notes.txt"), "{text}");
    assert_eq!(bookmark_at(&project, BRANCH), own);
    assert_eq!(pushes(&project), Vec::<String>::new(), "{text}");
}

#[test]
fn base_up_refuses_a_local_branch_that_differs_from_the_remote() {
    let project = project();
    let root = &project.root;
    // The branch moves on locally, unpushed.
    jj(root, &["new", &format!("bookmarks(exact:\"{BRANCH}\")")]);
    write(root, "local.txt", "local\n");
    jj(root, &["commit", "-m", "Local only"]);
    jj(root, &["bookmark", "set", BRANCH, "-r", "@-"]);
    let local = bookmark_at(&project, BRANCH);
    let remote = remote_ref(&project, BRANCH).expect("the branch");
    assert_ne!(local, remote);
    push_elsewhere(&project, BASE, "merged.txt", "merged\n");
    let before = working_copy(&project);
    watch_pushes(&project);

    let (code, _, text) = base_up(&project);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("push or reset it first"), "{text}");
    assert_eq!(bookmark_at(&project, BRANCH), local);
    assert_eq!(remote_ref(&project, BRANCH).as_deref(), Some(&*remote));
    assert_eq!(pushes(&project), Vec::<String>::new(), "{text}");
    assert_eq!(working_copy(&project), before);
}

#[test]
fn base_up_refuses_a_branch_or_base_the_remote_lacks() {
    let project = project();
    // A branch known only locally, never pushed.
    jj(
        &project.root,
        &["bookmark", "create", "feature/local", "-r", BASE],
    );
    watch_pushes(&project);

    for (args, missing) in [
        (&["feature/nowhere"][..], "feature/nowhere is not on origin"),
        (&["feature/local"][..], "feature/local is not on origin"),
        (&[BRANCH, "develop"][..], "develop is not on origin"),
    ] {
        let (code, _, text) = base_up_with(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
        assert!(text.contains(missing), "{args:?}: {text}");
    }
    assert_eq!(pushes(&project), Vec::<String>::new());
    assert_eq!(remote_ref(&project, "feature/nowhere"), None);
    assert_eq!(remote_ref(&project, "feature/local"), None);
    assert_eq!(remote_ref(&project, "develop"), None);
}

#[test]
fn base_up_tracks_a_branch_known_only_on_the_remote() {
    let project = project();
    let branch = "feature/v2";
    // The branch is opened elsewhere with work of its own, and the base moves on: the root has
    // fetched neither.
    push_elsewhere(&project, branch, "v2.txt", "v2\n");
    push_elsewhere(&project, BASE, "merged.txt", "merged\n");
    let own = remote_ref(&project, branch).expect("the branch");
    let base = remote_ref(&project, BASE).expect("the base");
    let before = working_copy(&project);

    let (code, log, text) = base_up_with(&project, &[branch]);

    assert_eq!(code, 0, "{text}");
    // The push moved the remote's branch: the root tracks it now.
    let merged = remote_ref(&project, branch).expect("the branch");
    assert_eq!(remote_parents(&project, branch), [own, base]);
    assert_eq!(last_line(&log), merged, "{text}");
    assert_eq!(bookmark_at(&project, branch), merged);
    let tracked = jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "bookmark",
            "list",
            "--tracked",
            "-T",
            "remote ++ \"\\n\"",
            &format!("exact:\"{branch}\""),
        ],
    );
    assert!(tracked.lines().any(|line| line == "origin"), "{tracked}");
    assert_eq!(working_copy(&project), before);
}

#[test]
fn base_up_refuses_with_1_what_it_cannot_use() {
    let project = project();
    let branch = remote_ref(&project, BRANCH);
    watch_pushes(&project);

    for args in [
        &[][..],
        &[BRANCH, BASE, "extra"][..],
        &[""][..],
        &["-x"][..],
        &["feature/\"quoted"][..],
        &["feature\\slashed"][..],
        &[BRANCH, "-x"][..],
        &[BRANCH, "ma\"in"][..],
        // A branch cannot be brought up to itself, the default base included.
        &[BASE][..],
        &[BRANCH, BRANCH][..],
    ] {
        let (code, _, text) = base_up_with(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    assert_eq!(pushes(&project), Vec::<String>::new());
    assert_eq!(remote_ref(&project, BRANCH), branch);
}
