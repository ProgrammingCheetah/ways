use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use super::*;
use crate::domain::DocumentSignature;

const VERSION: &str = "0.5.2";
const BRANCH: &str = "feature/0.5.2";
const FOLDER: &str = ".teams/releases/0.5.2";
const MANIFEST: &str = "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n[vcs]\nkind = \"jj\"\n";

struct Project {
    temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn jj(dir: &Path, args: &[&str]) -> String {
    let mut full = vec!["--no-pager", "--color=never"];
    full.extend_from_slice(args);
    run(dir, "jj", &full)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

fn executable(path: &Path, text: &str) {
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o750)).expect("mode");
}

/// The teams pack's action `name`, copied into the project's actions folder as shipped.
fn install(project: &Project, name: &str) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .join("plugins/teams/actions")
        .join(name);
    let text = std::fs::read_to_string(&file).expect("the shipped action");
    executable(&project.root.join(".zaf/actions").join(name), &text);
}

/// A jj project with the default branch pushed to a bare remote, signed objectives and design in
/// its release folder, and the two shipped actions.
fn project(remote: bool) -> Project {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("repo");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    jj(temp.path(), &["git", "init", "repo"]);
    jj(
        &root,
        &["config", "set", "--repo", "user.name", "Test Author"],
    );
    jj(
        &root,
        &["config", "set", "--repo", "user.email", "a@example.com"],
    );
    // `base-open` asks `zaf forge default-branch`: a `zaf` that says `main`, first on the actions'
    // path, stands for the forge.
    let bin = temp.path().join("bin");
    executable(&bin.join("zaf"), "#!/bin/sh\necho main\n");
    let search = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("search path");
    write(
        &root.join(".zaf.toml"),
        &format!(
            "{MANIFEST}\n[env]\nPATH = \"{}\"\n",
            search.to_string_lossy()
        ),
    );
    write(&root.join(".gitignore"), ".zaf/*\n!.zaf/actions/\n");
    write(&root.join("notes.txt"), "one\n");
    let project = Project { temp, root, home };
    install(&project, "base-open");
    install(&project, "papers-commit");
    if remote {
        let bare = project.temp.path().join("remote.git");
        run(
            project.temp.path(),
            "git",
            &["init", "-q", "--bare", "remote.git"],
        );
        jj(
            &project.root,
            &["git", "remote", "add", "origin", &bare.to_string_lossy()],
        );
    }
    jj(&project.root, &["commit", "-m", "Start the project"]);
    jj(&project.root, &["bookmark", "create", "main", "-r", "@-"]);
    if remote {
        jj(&project.root, &["git", "push", "--bookmark", "main"]);
    }
    sign(&project, "objectives.md", "# Objectives\n");
    sign(&project, "design.md", "# Design\n");
    project
}

fn sign(project: &Project, name: &str, body: &str) {
    write(
        &project.root.join(FOLDER).join(name),
        &DocumentSignature::sign(body, UNIX_EPOCH),
    );
}

fn start_of(project: &Project) -> Result<Started, BuildError> {
    let composition =
        Composition::load_with_home(&project.root, Some(&project.home)).expect("the manifest");
    start_with_home(&composition, VERSION, &project.home)
}

/// The commit `refs/heads/<name>` of the bare remote, none without it.
fn remote_ref(project: &Project, name: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project.temp.path().join("remote.git"))
        .args(["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")])
        .output()
        .expect("git");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// An action of the project that replaces the shipped one: it prints `text` and exits `code`, and
/// leaves a line in `ran` for each run.
fn stub(project: &Project, name: &str, code: u8, text: &str) {
    executable(
        &project.root.join(".zaf/actions").join(name),
        &format!(
            "#!/bin/sh\n# zaf description: a stub\n# zaf 1: refused\n# zaf 2: failed\n# zaf mode: 750\necho {name} >> \"$ZAF_ROOT/ran\"\necho '{text}'\nexit {code}\n"
        ),
    );
}

fn ran(project: &Project) -> String {
    std::fs::read_to_string(project.root.join("ran")).unwrap_or_default()
}

#[test]
fn build_start_refuses_an_unsigned_design() {
    let project = project(true);
    write(&project.root.join(FOLDER).join("design.md"), "# Design\n");

    let error = start_of(&project).expect_err("refused");

    assert!(
        matches!(&error, BuildError::Unsigned { document } if document == "design.md"),
        "{error:?}"
    );
    assert!(
        error
            .sentence(VERSION)
            .contains("zaf begin --version 0.5.2")
    );
    assert_eq!(remote_ref(&project, BRANCH), None);
}

#[test]
fn build_start_refuses_edited_objectives() {
    let project = project(true);
    let path = project.root.join(FOLDER).join("objectives.md");
    let text = std::fs::read_to_string(&path).expect("read");
    write(&path, &format!("{text}An edit after signing.\n"));

    let error = start_of(&project).expect_err("refused");

    assert!(
        matches!(&error, BuildError::Unsigned { document } if document == "objectives.md"),
        "{error:?}"
    );
    assert_eq!(remote_ref(&project, BRANCH), None);
}

#[test]
fn build_start_refuses_without_a_remote() {
    let project = project(false);
    stub(&project, "base-open", 0, "made nothing");
    stub(&project, "papers-commit", 0, "committed nothing");

    assert_eq!(start_of(&project), Err(BuildError::NoRemote));
    assert_eq!(ran(&project), "", "no action ran");
}

#[test]
fn build_start_opens_and_commits() {
    let project = project(true);

    let started = start_of(&project).expect("started");

    assert_eq!(started.branch, BRANCH);
    assert!(started.made);
    let commit = started.commit.expect("a commit");
    assert_eq!(remote_ref(&project, BRANCH), Some(commit));
}

#[test]
fn build_start_again_changes_nothing() {
    let project = project(true);
    let first = start_of(&project).expect("started");
    let head = remote_ref(&project, BRANCH);

    let again = start_of(&project).expect("started again");

    assert_eq!(again.branch, BRANCH);
    assert!(!again.made, "the branch is handed back");
    assert_eq!(again.commit, None);
    assert_eq!(remote_ref(&project, BRANCH), head);
    assert_eq!(first.commit, head);
}

#[test]
fn build_start_maps_an_action_refusal() {
    let project = project(true);
    stub(
        &project,
        "base-open",
        1,
        "the repository has no default branch",
    );

    let error = start_of(&project).expect_err("refused");

    assert_eq!(
        error,
        BuildError::Refused("the repository has no default branch".to_owned())
    );
    assert_eq!(ran(&project), "base-open\n", "the second action never ran");
}

#[test]
fn build_start_maps_an_action_failure() {
    let project = project(true);
    stub(&project, "base-open", 2, "the fetch failed");

    let error = start_of(&project).expect_err("failed");

    assert!(
        matches!(&error, BuildError::Backend(text) if text.contains("exit 2")),
        "{error:?}"
    );
}

#[test]
fn build_start_missing_base_open_is_a_refusal() {
    let project = project(true);
    std::fs::remove_file(project.root.join(".zaf/actions/base-open")).expect("remove");
    stub(&project, "papers-commit", 0, "committed nothing");

    let error = start_of(&project).expect_err("refused");

    assert_eq!(
        error,
        BuildError::Refused(
            "base-open is missing: run `zaf update` again; it installs the new actions".to_owned()
        )
    );
    assert_eq!(ran(&project), "", "papers-commit is not run");
    assert_eq!(remote_ref(&project, BRANCH), None);
}

#[test]
fn build_start_missing_papers_commit_is_a_refusal() {
    let project = project(true);
    std::fs::remove_file(project.root.join(".zaf/actions/papers-commit")).expect("remove");

    let error = start_of(&project).expect_err("refused");

    assert_eq!(
        error,
        BuildError::Refused(
            "papers-commit is missing: run `zaf update` again; it installs the new actions"
                .to_owned()
        )
    );
}

#[test]
fn build_start_a_failed_action_is_a_backend_failure() {
    let project = project(true);
    stub(&project, "papers-commit", 2, "the commit failed");

    let error = start_of(&project).expect_err("failed");

    assert!(
        matches!(&error, BuildError::Backend(text) if text.contains("exit 2")),
        "{error:?}"
    );
}

fn open_of(project: &Project) -> Result<ReleasePullRequest, BuildError> {
    let composition =
        Composition::load_with_home(&project.root, Some(&project.home)).expect("the manifest");
    open_release_with_home(&composition, VERSION, &project.home)
}

#[test]
fn build_open_release_hands_back_number_and_link() {
    let project = project(true);
    stub(
        &project,
        "release-open",
        0,
        "41\nhttps://forge.example/pulls/41",
    );

    let opened = open_of(&project).expect("opened");

    assert_eq!(opened.number, "41");
    assert_eq!(
        opened.link.as_deref(),
        Some("https://forge.example/pulls/41")
    );
}

#[test]
fn build_open_release_refused_keeps_its_sentence() {
    let project = project(true);
    stub(
        &project,
        "release-open",
        1,
        "the integration branch is not on the remote",
    );

    assert_eq!(
        open_of(&project),
        Err(BuildError::Refused(
            "the integration branch is not on the remote".to_owned()
        ))
    );
}

#[test]
fn open_release_skips_trailing_blank_lines() {
    let project = project(true);
    stub(
        &project,
        "release-open",
        0,
        "Nothing changed.\n41\nhttps://forge.example/pulls/41\n\n  \n",
    );

    let opened = open_of(&project).expect("opened");

    assert_eq!(opened.number, "41");
    assert_eq!(
        opened.link.as_deref(),
        Some("https://forge.example/pulls/41")
    );
}

#[test]
fn open_release_without_a_number_is_a_backend_failure() {
    for printed in [
        "no number here",
        "Nothing changed.\nhttps://forge.example/pulls/41",
        "https://forge.example/pulls/41\n41",
        "41\nNothing changed.\nhttps://forge.example/pulls/41",
        "41",
    ] {
        let project = project(true);
        stub(&project, "release-open", 0, printed);

        let error = open_of(&project).expect_err(printed);

        assert!(
            matches!(&error, BuildError::Backend(text) if text.contains("gave no pull request number")),
            "{printed:?}: {error:?}"
        );
    }
}
