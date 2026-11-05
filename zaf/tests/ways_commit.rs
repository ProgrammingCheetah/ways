//! The ways repository's `commit` action (docs/zaf-actions.md §3, §4, §7): run through `zaf do`
//! on a throwaway jj repository with a bare remote, it commits a slice's work in its workspace,
//! moves the slice's bookmark onto the commit and hands the commit id back as the last line of
//! its log. With nothing to commit it exits 1 and changes nothing.

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

/// A project whose root is a jj repository with one base bookmark pushed to a bare remote, the
/// `workspace-open` and `commit` actions in its actions folder, and the slice open.
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
    for name in ["workspace-open", "commit"] {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }

    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "workspace-open", SLICE, BASE]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// The slice's workspace.
fn workspace(project: &Project) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(SLICE)
}

/// `zaf do commit <slice> <message>` with its whole log: the exit code and the lines.
fn commit_action(project: &Project, slice: &str, message: &str) -> (i32, String) {
    let output = project.zaf(&["do", "--more", "commit", slice, message]);
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

#[test]
fn commit_hands_back_the_id() {
    let project = project();
    let root = &project.root;
    let dir = workspace(&project);
    let base = bookmark_at(root, BASE);
    write(&dir, "writer.csv", "a,b\n");

    let (code, text) = commit_action(&project, SLICE, "Add the CSV writer");

    assert_eq!(code, 0, "{text}");
    let id = handed_back(&text);
    // The id is the commit holding the work, which the bookmark now names and the workspace's
    // empty working copy sits on.
    assert_eq!(id.len(), 40, "{text}");
    assert_eq!(bookmark_at(root, SLICE), id, "{text}");
    assert_eq!(
        jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]),
        id
    );
    assert_eq!(
        jj(
            &dir,
            &["log", "--no-graph", "-r", "@-", "-T", "description"]
        )
        .trim(),
        "Add the CSV writer"
    );
    assert_eq!(
        jj(
            &dir,
            &[
                "log",
                "--no-graph",
                "-r",
                "@-",
                "-T",
                "parents.map(|c| c.commit_id())"
            ]
        ),
        base
    );
    // The base bookmark stays where it was, and nothing was pushed.
    assert_eq!(bookmark_at(root, BASE), base);
    assert_eq!(
        jj(
            root,
            &[
                "--ignore-working-copy",
                "log",
                "--no-graph",
                "-r",
                &format!("remote_bookmarks(exact:\"{SLICE}\")"),
                "-T",
                "commit_id",
            ]
        ),
        "",
        "commit pushed"
    );

    // A second commit of more work moves the bookmark on and hands back the new id.
    write(&dir, "writer.csv", "a,b\nc,d\n");
    let (code, text) = commit_action(&project, SLICE, "Write a second row");
    assert_eq!(code, 0, "{text}");
    assert_ne!(handed_back(&text), id);
    assert_eq!(bookmark_at(root, SLICE), handed_back(&text), "{text}");
}

#[test]
fn nothing_to_commit_refuses() {
    let project = project();
    let root = &project.root;
    let base = bookmark_at(root, BASE);

    let (code, text) = commit_action(&project, SLICE, "Add nothing");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("nothing to commit"), "{text}");
    // The bookmark did not move and no commit was made.
    assert_eq!(bookmark_at(root, SLICE), base, "{text}");
    assert_eq!(
        jj(
            &workspace(&project),
            &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]
        ),
        base
    );
}

#[test]
fn commit_refuses_a_slice_that_is_not_open() {
    let project = project();

    let (code, text) = commit_action(&project, "feature/v1-9-9", "Add a file");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("not open"), "{text}");
    assert!(!project.root.join(".zaf/workspaces/feature/v1-9-9").exists());
}

#[test]
fn commit_refuses_a_slice_name_that_leaves_the_workspaces_directory() {
    let project = project();

    let (code, text) = commit_action(&project, "../escape", "Add a file");

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("not a relative path"), "{text}");
}
