//! The teams pack's `base-open` action (design 0.5.2, M21 and I10): run through `zaf do` on a
//! throwaway jj repository with a bare remote, it opens the version's integration branch on the
//! remote at the default branch's head, hands back one that descends from the default branch, and
//! refuses one that shares no history with it or that exists only locally. The working copy never
//! moves.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, fixtures, jj, outcome, path_with_zaf, run, write};

const BASE: &str = "main";
const BRANCH: &str = "feature/0.5.2";

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

/// The action as the teams pack ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join("plugins/teams/actions/base-open");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the default branch pushed to a bare remote, and
/// the action in its actions folder. `ZAF_VERSION` and `ZAF_BASE` are set as the build start sets
/// them.
fn project() -> Project {
    project_with_forge_default(BASE)
}

/// The same project, its fake forge naming `default` as the repository's default branch.
fn project_with_forge_default(default: &str) -> Project {
    let mut project = Project::new(MANIFEST);
    project
        .env("ZAF_VERSION", "0.5.2")
        .env("ZAF_BASE", BRANCH)
        .env("PATH", &path_with_zaf(&[&fixtures()]));
    project.on("api *", &format!("{default}\n"));
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
    let actions = root.join(".zaf/actions");
    write(&actions, "base-open", &action_text());
    fs::set_permissions(actions.join("base-open"), fs::Permissions::from_mode(0o750))
        .expect("mode");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// `zaf do --more base-open`: the exit code, the action's whole log (what it printed) and zaf's
/// own stderr.
fn base_open(project: &Project) -> (i32, String, String) {
    outcome(&project.zaf(&["do", "--more", "base-open"]))
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

fn working_copy(project: &Project) -> String {
    commit(&project.root, "@")
}

/// Commits to the default branch from a second clone, so the integration branch no longer sits at
/// its head.
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
    write(&other, "elsewhere.txt", "e\n");
    run(&other, "git", &["add", "elsewhere.txt"]);
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

/// Pushes the integration branch from a commit with no history of the default branch's.
fn push_unrelated_branch(project: &Project) {
    let temp = project.temp.path();
    let other = temp.join("unrelated");
    run(temp, "git", &["init", "-q", "unrelated"]);
    write(&other, "alone.txt", "a\n");
    run(&other, "git", &["add", "alone.txt"]);
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
            "A commit that lacks the default branch",
        ],
    );
    let remote = remote_dir(project);
    run(
        &other,
        "git",
        &[
            "push",
            "-q",
            &remote.to_string_lossy(),
            &format!("HEAD:refs/heads/{BRANCH}"),
        ],
    );
}

#[test]
fn base_open_makes_and_pushes_the_branch() {
    let project = project();
    let main_head = remote_ref(&project, BASE).expect("the remote holds the default branch");
    assert_eq!(remote_ref(&project, BRANCH), None);

    let (code, stdout, stderr) = base_open(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), format!("made {BRANCH}"));
    assert_eq!(remote_ref(&project, BRANCH), Some(main_head));
    // Nothing else moved: the default branch is where it was and no tag appeared.
    assert_eq!(
        remote_ref(&project, BASE),
        remote_ref(&project, BRANCH),
        "{stderr}"
    );
}

#[test]
fn base_open_cuts_from_the_forges_default_branch() {
    let project = project_with_forge_default("release-main");
    // jj's trunk() is `main`; the forge's default branch is `release-main`, one commit further.
    write(&project.root, "release.txt", "r\n");
    jj(&project.root, &["commit", "-m", "The release line"]);
    jj(
        &project.root,
        &["bookmark", "create", "release-main", "-r", "@-"],
    );
    jj(
        &project.root,
        &["git", "push", "--bookmark", "release-main"],
    );
    let main_head = remote_ref(&project, BASE).expect("main");
    let release_head = remote_ref(&project, "release-main").expect("release-main");
    assert_ne!(main_head, release_head);

    let (code, stdout, stderr) = base_open(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), format!("made {BRANCH}"));
    assert_eq!(remote_ref(&project, BRANCH), Some(release_head));
}

#[test]
fn base_open_refuses_a_missing_default_branch() {
    let project = project_with_forge_default("release-main");

    let (code, stdout, stderr) = base_open(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    let said = format!("{stdout}{stderr}");
    assert!(said.contains("release-main"), "names the branch: {said}");
    // Besides zaf's own `FAIL` line, the action said one sentence.
    let own = said.lines().filter(|line| !line.starts_with("FAIL "));
    assert_eq!(own.count(), 1, "one sentence: {said}");
    assert_eq!(remote_ref(&project, BRANCH), None, "a branch was pushed");
    let bookmarks = jj(
        &project.root,
        &["bookmark", "list", &format!("exact:{BRANCH}")],
    );
    assert_eq!(bookmarks.trim(), "", "a bookmark was made");
}

#[test]
fn base_open_hands_back_an_existing_branch() {
    let project = project();
    let (code, stdout, stderr) = base_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let made = remote_ref(&project, BRANCH).expect("made");

    let (code, stdout, stderr) = base_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "", "a branch handed back prints nothing");
    assert_eq!(remote_ref(&project, BRANCH), Some(made.clone()));

    // A commit landing on the default branch later does not unmake the branch (#288).
    advance_base_elsewhere(&project);
    let (code, stdout, stderr) = base_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "");
    assert_eq!(remote_ref(&project, BRANCH), Some(made));
}

#[test]
fn base_open_refuses_unrelated_history() {
    let project = project();
    push_unrelated_branch(&project);
    let before = remote_ref(&project, BRANCH);

    let (code, stdout, stderr) = base_open(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains(BRANCH),
        "the refusal names the branch: {stdout}{stderr}"
    );
    assert_eq!(
        remote_ref(&project, BRANCH),
        before,
        "the remote's branch moved"
    );
}

#[test]
fn base_open_refuses_a_local_only_branch() {
    let project = project();
    jj(&project.root, &["bookmark", "create", BRANCH, "-r", BASE]);

    let (code, stdout, stderr) = base_open(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains(BRANCH),
        "the refusal names the branch: {stdout}{stderr}"
    );
    assert_eq!(
        remote_ref(&project, BRANCH),
        None,
        "a local branch was pushed"
    );
}

#[test]
fn base_open_leaves_the_working_copy() {
    let project = project();
    write(&project.root, "wip.txt", "wip\n");
    // Snapshot the file into the working copy's commit once, so the commit id is settled.
    jj(&project.root, &["status"]);
    let before = working_copy(&project);

    let (code, stdout, stderr) = base_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(working_copy(&project), before, "made");

    let (code, stdout, stderr) = base_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(working_copy(&project), before, "handed back");

    push_unrelated_branch_over(&project);
    let (code, _, _) = base_open(&project);
    assert_eq!(code, 1);
    assert_eq!(working_copy(&project), before, "refused");

    assert_eq!(
        fs::read_to_string(project.root.join("wip.txt")).expect("the file stays"),
        "wip\n"
    );
}

/// Replaces the remote's integration branch with one unrelated to the default branch.
fn push_unrelated_branch_over(project: &Project) {
    let remote = remote_dir(project);
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
    push_unrelated_branch(project);
}
