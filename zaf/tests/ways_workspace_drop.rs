//! The ways repository's `workspace-drop` action (docs/zaf-actions.md §4, §7): run through `zaf
//! do` on a throwaway jj repository with a bare remote, it forgets a slice's workspace, removes
//! its directory and any empty parents, and one already gone is quiet.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";
const SIBLING: &str = "feature/v1-1-3";

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

/// A project whose root is a jj repository with a base bookmark pushed to a bare remote, and the
/// `workspace-open` and `workspace-drop` actions in its actions folder.
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
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    for name in ["workspace-open", "workspace-drop"] {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }
    project
}

/// `zaf do --more <name> <args>` with its whole log: the exit code and the lines.
fn action(project: &Project, name: &str, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", name];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn open(project: &Project, slice: &str) {
    let (code, text) = action(project, "workspace-open", &[slice, BASE]);
    assert_eq!(code, 0, "{text}");
}

fn drop_slice(project: &Project, slice: &str) -> (i32, String) {
    action(project, "workspace-drop", &[slice])
}

fn workspaces(root: &Path) -> String {
    jj(root, &["--ignore-working-copy", "workspace", "list"])
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

/// The directory a slice's workspace sits in, under the workspaces directory.
fn workspace_dir(project: &Project, slice: &str) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(slice)
}

#[test]
fn workspace_drop_forgets_removes_and_clears_empty_parents() {
    let project = project();
    let root = &project.root;
    open(&project, SLICE);
    let dir = workspace_dir(&project, SLICE);
    assert!(dir.join(".jj").is_dir());
    assert!(workspaces(root).contains(SLICE));
    // A file the slice made leaves with the directory.
    write(&dir, "work.txt", "in progress\n");

    let (code, text) = drop_slice(&project, SLICE);

    assert_eq!(code, 0, "{text}");
    assert!(!dir.exists(), "{} is still there", dir.display());
    assert!(!workspaces(root).contains(SLICE), "{}", workspaces(root));
    // `feature/` held only this slice, so it is gone with it; the workspaces directory stays.
    assert!(
        !dir.parent().expect("a parent").exists(),
        "an empty parent was left"
    );
    assert!(workspace_dir(&project, "").is_dir());
    // The bookmark is the branch, and it stays.
    assert_eq!(bookmark_at(root, SLICE), bookmark_at(root, BASE));
}

#[test]
fn workspace_drop_twice_is_quiet() {
    let project = project();
    let root = &project.root;
    open(&project, SLICE);
    let dir = workspace_dir(&project, SLICE);

    let (code, text) = drop_slice(&project, SLICE);
    assert_eq!(code, 0, "{text}");

    // Gone already: exit 0, and a plain run prints nothing, as a pass does.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "workspace-drop", SLICE]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(format!("{stdout}{stderr}"), "");
    assert!(!dir.exists());
    assert!(!workspaces(root).contains(SLICE));

    // A name that never was a slice is the same.
    let (code, text) = drop_slice(&project, "feature/v1-9-9");
    assert_eq!(code, 0, "{text}");
}

#[test]
fn workspace_drop_after_the_directory_was_removed_by_hand_forgets_the_name() {
    let project = project();
    let root = &project.root;
    open(&project, SLICE);
    let dir = workspace_dir(&project, SLICE);
    fs::remove_dir_all(&dir).expect("remove by hand");
    assert!(workspaces(root).contains(SLICE), "jj forgot it already");

    let (code, text) = drop_slice(&project, SLICE);

    assert_eq!(code, 0, "{text}");
    assert!(!workspaces(root).contains(SLICE));
}

#[test]
fn workspace_drop_keeps_a_parent_another_slice_lives_in() {
    let project = project();
    let root = &project.root;
    open(&project, SLICE);
    open(&project, SIBLING);

    let (code, text) = drop_slice(&project, SLICE);

    assert_eq!(code, 0, "{text}");
    assert!(!workspace_dir(&project, SLICE).exists());
    let sibling = workspace_dir(&project, SIBLING);
    assert!(sibling.join(".jj").is_dir(), "the sibling was removed");
    assert!(workspaces(root).contains(SIBLING));
}

#[test]
fn workspace_drop_refuses_a_name_that_leaves_the_workspaces_directory() {
    let project = project();
    let outside = project.temp.path().join("repo/.zaf/outside");
    fs::create_dir_all(&outside).expect("outside directory");
    write(&outside, "keep.txt", "not a slice\n");

    for name in [
        "../outside",
        "feature/../../outside",
        "/tmp/zaf-outside",
        "./inside",
        "feature//twice",
        "..",
        "",
    ] {
        let (code, text) = drop_slice(&project, name);
        assert_eq!(code, 1, "{name:?}: {text}");
    }

    assert!(
        outside.join("keep.txt").is_file(),
        "a refused drop removed it"
    );
}
