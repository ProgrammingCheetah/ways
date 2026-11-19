//! The teams pack's `papers-commit` action (design 0.5.2, M22 and I10): run through `zaf do` on a
//! throwaway jj repository with a bare remote and the integration branch opened by `base-open`,
//! it puts the release folder, as the working copy holds it, in one commit on the integration
//! branch and pushes it. Nothing new is quiet; a file outside the folder is never committed; the
//! working copy never moves.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, fixtures, jj, outcome, path_with_zaf, run, write};

const BASE: &str = "main";
const BRANCH: &str = "feature/0.5.2";
const FOLDER: &str = ".teams/releases/0.5.2";

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

/// An action as the teams pack ships it, copied into the project's actions folder.
fn install(root: &Path, name: &str) {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join("plugins/teams/actions")
        .join(name);
    let text =
        fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    let actions = root.join(".zaf/actions");
    write(&actions, name, &text);
    fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
}

/// A project whose root is a jj repository with the default branch pushed to a bare remote, the
/// integration branch opened with `base-open`, a release folder of two files (and a subfolder) in
/// the working copy, and both actions in its actions folder.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project
        .env("ZAF_VERSION", "0.5.2")
        .env("ZAF_BASE", BRANCH)
        .env("ZAF_RELEASE_DIR", FOLDER)
        .env("PATH", &path_with_zaf(&[&fixtures()]));
    project.on("api *", &format!("{BASE}\n"));
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
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\n");
    install(&root, "base-open");
    install(&root, "papers-commit");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// The project with the integration branch opened and the release folder written.
fn opened() -> Project {
    let project = project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "base-open"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    write_folder(&project);
    project
}

fn write_folder(project: &Project) {
    let folder = project.root.join(FOLDER);
    write(&folder, "objectives.md", "# Objectives\n");
    write(&folder, "design.md", "# Design\n");
    write(&folder.join("mocks"), "screen.sh", "echo screen\n");
}

/// `zaf do --more papers-commit`: the exit code, the action's whole log (what it printed) and
/// zaf's own stderr.
fn papers_commit(project: &Project) -> (i32, String, String) {
    outcome(&project.zaf(&["do", "--more", "papers-commit"]))
}

fn remote_dir(project: &Project) -> PathBuf {
    project.temp.path().join("remote.git")
}

/// The commit a ref names in the bare remote, none when it holds no such ref.
fn remote_ref(project: &Project, name: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(remote_dir(project))
        .args(["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")])
        .output()
        .expect("git");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The files a commit of the bare remote holds, as `git ls-tree` lists them.
fn remote_files(project: &Project, commit: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(remote_dir(project))
        .args(["ls-tree", "-r", "--name-only", commit])
        .output()
        .expect("git");
    assert!(output.status.success(), "ls-tree {commit}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A file of a commit of the bare remote, its text.
fn remote_file(project: &Project, commit: &str, path: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(remote_dir(project))
        .args(["show", &format!("{commit}:{path}")])
        .output()
        .expect("git");
    assert!(output.status.success(), "show {commit}:{path}");
    String::from_utf8_lossy(&output.stdout).into_owned()
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

/// The working copy's commit once its files are recorded in it.
fn settled_working_copy(project: &Project) -> String {
    jj(&project.root, &["status"]);
    commit(&project.root, "@")
}

#[test]
fn papers_commit_commits_and_pushes_the_folder() {
    let project = opened();
    let opened_at = remote_ref(&project, BRANCH).expect("base-open pushed the branch");
    let main_head = remote_ref(&project, BASE).expect("the default branch");

    let (code, stdout, stderr) = papers_commit(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    let head = remote_ref(&project, BRANCH).expect("the branch stays on the remote");
    assert_eq!(stdout.trim(), format!("committed {head}"), "{stderr}");
    assert_ne!(head, opened_at);
    let files = remote_files(&project, &head);
    for file in [
        format!("{FOLDER}/objectives.md"),
        format!("{FOLDER}/design.md"),
        format!("{FOLDER}/mocks/screen.sh"),
    ] {
        assert!(files.contains(&file), "{file} is missing from {files:?}");
    }
    assert_eq!(
        remote_file(&project, &head, &format!("{FOLDER}/design.md")),
        "# Design\n"
    );
    // One commit on the branch as it was opened; nothing else moved.
    let parent = std::process::Command::new("git")
        .args(["-C"])
        .arg(remote_dir(&project))
        .args(["rev-parse", &format!("{head}^")])
        .output()
        .expect("git");
    assert_eq!(
        String::from_utf8_lossy(&parent.stdout).trim(),
        opened_at,
        "one commit on the opened branch"
    );
    assert_eq!(remote_ref(&project, BASE), Some(main_head));

    // A file edited in the folder, and one removed from it, are the next commit.
    write(
        &project.root.join(FOLDER),
        "design.md",
        "# Design, second\n",
    );
    fs::remove_file(project.root.join(FOLDER).join("objectives.md")).expect("remove");
    let (code, stdout, stderr) = papers_commit(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let second = remote_ref(&project, BRANCH).expect("branch");
    assert_eq!(stdout.trim(), format!("committed {second}"), "{stderr}");
    assert_eq!(
        remote_file(&project, &second, &format!("{FOLDER}/design.md")),
        "# Design, second\n"
    );
    assert!(!remote_files(&project, &second).contains(&format!("{FOLDER}/objectives.md")));
}

#[test]
fn papers_commit_takes_an_absolute_folder() {
    let mut project = opened();
    let folder = project.root.join(FOLDER);
    project.env("ZAF_RELEASE_DIR", &folder.to_string_lossy());

    let (code, stdout, stderr) = papers_commit(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    let head = remote_ref(&project, BRANCH).expect("branch");
    assert_eq!(stdout.trim(), format!("committed {head}"), "{stderr}");
    assert!(remote_files(&project, &head).contains(&format!("{FOLDER}/design.md")));
}

#[test]
fn papers_commit_is_quiet_with_nothing_new() {
    let project = opened();
    let (code, stdout, stderr) = papers_commit(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let pushed = remote_ref(&project, BRANCH).expect("branch");

    let (code, stdout, stderr) = papers_commit(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "", "nothing new prints nothing");
    assert_eq!(remote_ref(&project, BRANCH), Some(pushed));
}

#[test]
fn papers_commit_leaves_files_outside_the_folder() {
    let project = opened();
    write(&project.root, "notes.txt", "two\n");
    write(&project.root, "elsewhere.txt", "e\n");

    let (code, stdout, stderr) = papers_commit(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    let head = remote_ref(&project, BRANCH).expect("branch");
    let files = remote_files(&project, &head);
    assert!(!files.contains(&"elsewhere.txt".to_owned()), "{files:?}");
    assert_eq!(
        remote_file(&project, &head, "notes.txt"),
        "one\n",
        "a file outside the folder is as the branch had it"
    );
    // The working copy keeps both edits.
    assert_eq!(
        fs::read_to_string(project.root.join("notes.txt")).expect("notes"),
        "two\n"
    );
    assert_eq!(
        fs::read_to_string(project.root.join("elsewhere.txt")).expect("elsewhere"),
        "e\n"
    );
    // With only files outside the folder changed, a run commits nothing.
    let (code, stdout, stderr) = papers_commit(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "");
    assert_eq!(remote_ref(&project, BRANCH), Some(head));
}

#[test]
fn papers_commit_refuses_without_the_branch() {
    let project = project();
    write_folder(&project);

    let (code, stdout, stderr) = papers_commit(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains(BRANCH),
        "the refusal names the branch: {stdout}{stderr}"
    );
    assert_eq!(remote_ref(&project, BRANCH), None, "a branch was pushed");
    assert!(remote_ref(&project, BASE).is_some());
}

#[test]
fn papers_commit_leaves_the_working_copy() {
    let project = opened();
    write(&project.root, "wip.txt", "wip\n");
    let before = settled_working_copy(&project);

    let (code, stdout, stderr) = papers_commit(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(commit(&project.root, "@"), before, "committed");

    let (code, stdout, stderr) = papers_commit(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(commit(&project.root, "@"), before, "quiet");

    assert_eq!(
        fs::read_to_string(project.root.join("wip.txt")).expect("the file stays"),
        "wip\n"
    );
    assert_eq!(
        fs::read_to_string(project.root.join(FOLDER).join("design.md")).expect("the folder stays"),
        "# Design\n"
    );

    // A refused run leaves it too.
    let remote = remote_dir(&project);
    run(
        project.temp.path(),
        "git",
        &[
            "-C",
            &remote.to_string_lossy(),
            "update-ref",
            "-d",
            &format!("refs/heads/{BRANCH}"),
        ],
    );
    let (code, _, _) = papers_commit(&project);
    assert_eq!(code, 1);
    assert_eq!(commit(&project.root, "@"), before, "refused");
}
