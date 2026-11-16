//! The ways repository's `workspace-open` action (docs/zaf-actions.md §4, §7): run through `zaf
//! do` on a throwaway jj repository with a bare remote, it makes a slice's workspace and a
//! same-named bookmark at the base and hands the path back as the last line of its log. A new
//! slice is cut from a fetched base: a local base behind the remote's is moved up first, and a
//! repository with no remote opens on the local base.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const OTHER_BASE: &str = "feature/v2";
const SLICE: &str = "feature/v1-1-2";

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
        .join(".zaf/actions/workspace-open");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with two base bookmarks pushed to a bare remote and
/// the `workspace-open` action in its actions folder.
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
    write(&root, ".gitignore", ".zaf/*\n");
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    write(&root, "later.txt", "later\n");
    jj(&root, &["commit", "-m", "Go on"]);
    jj(&root, &["bookmark", "create", OTHER_BASE, "-r", "@-"]);
    jj(
        &root,
        &["git", "push", "--bookmark", BASE, "--bookmark", OTHER_BASE],
    );

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    write(&actions, "workspace-open", &action_text());
    fs::set_permissions(
        actions.join("workspace-open"),
        fs::Permissions::from_mode(0o750),
    )
    .expect("mode");
    project
}

/// `zaf do workspace-open <slice> <base>` with its whole log: the exit code and the lines.
fn open(project: &Project, slice: &str, base: &str) -> (i32, String) {
    let output = project.zaf(&["do", "--more", "workspace-open", slice, base]);
    let (code, stdout, stderr) = outcome(&output);
    (code, format!("{stdout}{stderr}"))
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
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

fn bookmark_at(root: &Path, name: &str) -> String {
    commit(root, &format!("bookmarks(exact:\"{name}\")"))
}

/// The repository's operation ids only. The default `op log` text carries a relative age
/// ("1 second ago" or "now") that changes across a second boundary, so it cannot be compared.
fn operation_ids(root: &Path) -> String {
    jj(
        root,
        &[
            "--ignore-working-copy",
            "op",
            "log",
            "--no-graph",
            "-T",
            "id.short() ++ \"\\n\"",
        ],
    )
}

fn workspaces(root: &Path) -> String {
    jj(root, &["--ignore-working-copy", "workspace", "list"])
}

/// Pushes a commit to the base from a second clone, so the root's local and remote-tracking
/// base both lag until something fetches. The commit the remote then holds for the base.
fn advance_base_elsewhere(project: &Project, path: &str, content: &str) -> String {
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
            "Advance the base elsewhere",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", BASE]);
    run(&remote, "git", &["rev-parse", BASE]).trim().to_owned()
}

fn workspace_dir(project: &Project, slice: &str) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(slice)
}

#[test]
fn workspace_open_hands_back_the_path() {
    let project = project();
    let root = &project.root;
    let dir = workspace_dir(&project, SLICE);

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), dir.to_string_lossy(), "{text}");
    assert!(
        dir.join(".jj").is_dir(),
        "no workspace at {}",
        dir.display()
    );
    assert!(
        dir.join("notes.txt").is_file(),
        "the base is not checked out"
    );
    assert!(workspaces(root).contains(SLICE), "{}", workspaces(root));
    assert_eq!(bookmark_at(root, SLICE), bookmark_at(root, BASE));
    // The workspace's change sits on the base.
    let parent = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    assert_eq!(parent, bookmark_at(root, BASE));

    // On the same base again it changes nothing and exits 0, handing the path back as before.
    let operations = operation_ids(root);
    let (code, text) = open(&project, SLICE, BASE);
    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), dir.to_string_lossy(), "{text}");
    assert_eq!(
        operation_ids(root),
        operations,
        "a second open changed the repository"
    );
    assert_eq!(bookmark_at(root, SLICE), bookmark_at(root, BASE));
}

#[test]
fn workspace_open_keeps_an_existing_bookmark_of_the_slice() {
    let project = project();
    let root = &project.root;
    jj(
        root,
        &[
            "bookmark",
            "create",
            SLICE,
            "-r",
            &bookmark_at(root, OTHER_BASE),
        ],
    );

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(root, SLICE), bookmark_at(root, OTHER_BASE));
}

#[test]
fn workspace_open_on_another_base_refuses() {
    let project = project();
    let root = &project.root;
    let (code, text) = open(&project, SLICE, BASE);
    assert_eq!(code, 0, "{text}");
    let operations = operation_ids(root);

    let (code, text) = open(&project, SLICE, OTHER_BASE);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains(BASE), "{text}");
    assert_eq!(
        operation_ids(root),
        operations,
        "a refused open changed the repository"
    );
    assert_eq!(bookmark_at(root, SLICE), bookmark_at(root, BASE));
}

#[test]
fn workspace_open_refuses_a_directory_that_is_not_a_slice() {
    let project = project();
    let dir = workspace_dir(&project, SLICE);
    write(&dir, "stray.txt", "not a workspace\n");

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("not a slice workspace"), "{text}");
    assert!(
        !workspaces(&project.root).contains(SLICE),
        "{}",
        workspaces(&project.root)
    );
    assert_eq!(
        commit(&project.root, &format!("bookmarks(exact:\"{SLICE}\")")),
        "",
        "a bookmark was made for a refused slice"
    );
}

#[test]
fn workspace_open_refuses_an_unknown_base() {
    let project = project();

    let (code, text) = open(&project, SLICE, "feature/nowhere");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("feature/nowhere"), "{text}");
    assert!(!workspace_dir(&project, SLICE).exists());
    assert_eq!(bookmark_at(&project.root, SLICE), "");
}

#[test]
fn workspace_open_refuses_a_name_that_leaves_the_workspaces_directory() {
    let project = project();
    let escaped = project.temp.path().join("repo/.zaf/outside");

    for name in [
        "../outside",
        "feature/../../outside",
        "/tmp/zaf-outside",
        "./inside",
        "feature//twice",
        "..",
        "",
    ] {
        let (code, text) = open(&project, name, BASE);
        assert_eq!(code, 1, "{name:?}: {text}");
    }

    assert!(!escaped.exists());
    assert!(!project.temp.path().join("outside").exists());
    assert!(!project.root.join(".zaf/workspaces").exists());
}

#[test]
fn a_slice_opened_after_the_remote_base_moved_carries_the_remotes_commits() {
    let project = project();
    let root = &project.root;
    let before = bookmark_at(root, BASE);
    let pushed = advance_base_elsewhere(&project, "elsewhere.txt", "e\n");
    assert_ne!(pushed, before);
    assert_eq!(bookmark_at(root, BASE), before, "nothing fetched yet");

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 0, "{text}");
    let dir = workspace_dir(&project, SLICE);
    assert_eq!(handed_back(&text), dir.to_string_lossy(), "{text}");
    // The local base moved up to the remote's, and the slice starts there.
    assert_eq!(bookmark_at(root, BASE), pushed);
    assert_eq!(bookmark_at(root, SLICE), pushed);
    let parent = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    assert_eq!(parent, pushed);
    assert!(dir.join("elsewhere.txt").is_file());
}

#[test]
fn a_slice_opens_on_a_local_base_that_is_ahead_of_the_remote_and_leaves_it_there() {
    let project = project();
    let root = &project.root;
    // The base moves on locally, unpushed: the remote's base is behind it.
    let ahead = bookmark_at(root, OTHER_BASE);
    jj(root, &["bookmark", "set", BASE, "-r", &ahead]);
    let local = bookmark_at(root, BASE);

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 0, "{text}");
    assert_eq!(bookmark_at(root, BASE), local, "the local base moved");
    assert_eq!(bookmark_at(root, SLICE), local);
    assert_eq!(local, ahead);
    assert!(workspace_dir(&project, SLICE).join("later.txt").is_file());
}

#[test]
fn a_repository_with_no_remote_still_opens_a_slice() {
    let project = project();
    let root = &project.root;
    jj(root, &["git", "remote", "remove", "origin"]);
    let base = bookmark_at(root, BASE);

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 0, "{text}");
    let dir = workspace_dir(&project, SLICE);
    assert_eq!(handed_back(&text), dir.to_string_lossy(), "{text}");
    assert_eq!(bookmark_at(root, BASE), base);
    assert_eq!(bookmark_at(root, SLICE), base);
    assert!(dir.join("notes.txt").is_file());
}

#[test]
fn a_fetch_that_fails_opens_no_slice_and_exits_2() {
    let project = project();
    fs::remove_dir_all(project.temp.path().join("remote.git")).expect("remove the remote");

    let (code, text) = open(&project, SLICE, BASE);

    assert_eq!(code, 2, "{text}");
    assert!(!workspace_dir(&project, SLICE).exists());
    assert_eq!(bookmark_at(&project.root, SLICE), "");
}
