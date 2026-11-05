//! The ways repository's `push` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote, it moves a slice's bookmark forward to the
//! workspace's latest commit and pushes it plainly. Uncommitted work, a backward move and a remote
//! with work the slice lacks exit 1 and overwrite nothing; nothing new exits 0.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// A project whose root is a jj repository with a base bookmark pushed to a bare remote, the
/// `workspace-open` and `push` actions in its actions folder, and the slice open.
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
    for name in ["workspace-open", "push"] {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }

    let (code, text) = action(&project, "workspace-open", &[SLICE, BASE]);
    assert_eq!(code, 0, "{text}");
    project
}

/// `zaf do --more <name> <args>` with its whole log: the exit code and the lines.
fn action(project: &Project, name: &str, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", name];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn push(project: &Project, slice: &str) -> (i32, String) {
    action(project, "push", &[slice])
}

fn workspace(project: &Project) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(SLICE)
}

/// Commits a file in the slice's workspace by hand, which leaves the bookmark behind.
fn commit_by_hand(project: &Project, file: &str) -> String {
    let dir = workspace(project);
    write(&dir, file, "by hand\n");
    jj(&dir, &["commit", "-m", &format!("Add {file}")]);
    head(&dir)
}

/// The workspace's latest commit.
fn head(dir: &Path) -> String {
    jj(dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"])
}

/// The commit the slice's local bookmark names in the root repository.
fn local_target(project: &Project) -> String {
    jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            &format!("bookmarks(exact:\"{SLICE}\")"),
            "-T",
            "commit_id",
        ],
    )
}

/// The commit the bare remote's branch names, or `None` while the remote has no such branch.
fn remote_target(project: &Project, branch: &str) -> Option<String> {
    let remote = project.temp.path().join("remote.git");
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&remote)
        .args(["rev-parse", "--verify", "-q"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .expect("spawn");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[test]
fn push_moves_the_bookmark_to_a_commit_made_by_hand_and_pushes_it() {
    let project = project();
    let made = commit_by_hand(&project, "first.txt");
    // The hand-made commit left the bookmark on the base.
    assert_eq!(local_target(&project), jj_base(&project));
    assert_eq!(remote_target(&project, SLICE), None);

    let (code, text) = push(&project, SLICE);

    assert_eq!(code, 0, "{text}");
    assert_eq!(local_target(&project), made);
    assert_eq!(remote_target(&project, SLICE), Some(made.clone()));
    // The hand-back is the pushed commit.
    assert_eq!(text.lines().last(), Some(made.as_str()), "{text}");

    // A second commit goes the same way, forward.
    let second = commit_by_hand(&project, "second.txt");
    let (code, text) = push(&project, SLICE);
    assert_eq!(code, 0, "{text}");
    assert_eq!(remote_target(&project, SLICE), Some(second));
}

fn jj_base(project: &Project) -> String {
    jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            &format!("bookmarks(exact:\"{BASE}\")"),
            "-T",
            "commit_id",
        ],
    )
}

#[test]
fn push_with_nothing_new_exits_zero_and_changes_nothing() {
    let project = project();
    let made = commit_by_hand(&project, "first.txt");
    let (code, text) = push(&project, SLICE);
    assert_eq!(code, 0, "{text}");

    let (code, text) = push(&project, SLICE);

    assert_eq!(code, 0, "{text}");
    assert_eq!(remote_target(&project, SLICE), Some(made.clone()));
    assert_eq!(local_target(&project), made);
    assert_eq!(text.lines().last(), Some(made.as_str()), "{text}");
}

#[test]
fn push_refuses_uncommitted_work_and_pushes_nothing() {
    let project = project();
    write(&workspace(&project), "wip.txt", "wip\n");

    let (code, text) = push(&project, SLICE);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("uncommitted work"), "{text}");
    assert_eq!(remote_target(&project, SLICE), None);
}

#[test]
fn push_refuses_to_move_the_bookmark_backwards() {
    let project = project();
    let dir = workspace(&project);
    commit_by_hand(&project, "a.txt");
    let (code, text) = push(&project, SLICE);
    assert_eq!(code, 0, "{text}");
    let pushed = local_target(&project);

    // The workspace now sits on a sibling of the slice's tip: the bookmark would go sideways.
    jj(&dir, &["new", &format!("bookmarks(exact:\"{BASE}\")")]);
    let (code, text) = push(&project, SLICE);

    assert_eq!(code, 1, "{text}");
    assert_eq!(local_target(&project), pushed);
    assert_eq!(remote_target(&project, SLICE), Some(pushed));
}

#[test]
fn push_refuses_a_moved_remote() {
    let project = project();
    commit_by_hand(&project, "a.txt");
    let (code, text) = push(&project, SLICE);
    assert_eq!(code, 0, "{text}");

    // Someone else adds work to the slice's branch on the remote.
    let temp = project.temp.path().to_owned();
    let remote_text = temp.join("remote.git").to_string_lossy().into_owned();
    run(&temp, "git", &["clone", "-q", &remote_text, "other"]);
    let other = temp.join("other");
    run(&other, "git", &["checkout", "-q", SLICE]);
    write(&other, "b.txt", "b\n");
    run(&other, "git", &["add", "b.txt"]);
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
            "Add b elsewhere",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", SLICE]);
    let remote_work = remote_target(&project, SLICE).expect("the remote branch");

    commit_by_hand(&project, "c.txt");
    let (code, text) = push(&project, SLICE);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("the remote has work"), "{text}");
    assert_eq!(remote_target(&project, SLICE), Some(remote_work));
}

#[test]
fn push_refuses_a_slice_without_a_workspace_or_a_name_that_leaves_the_directory() {
    let project = project();
    commit_by_hand(&project, "a.txt");

    for name in [
        "feature/v1-9-9",
        "../outside",
        "feature/../../outside",
        "/tmp/zaf-outside",
        "feature//twice",
        "quote\"d",
        "",
    ] {
        let (code, text) = push(&project, name);
        assert_eq!(code, 1, "{name:?}: {text}");
    }
    assert_eq!(remote_target(&project, SLICE), None);
}
