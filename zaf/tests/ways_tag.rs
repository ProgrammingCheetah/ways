//! The ways repository's `tag` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote, `tag <name> [rev]` creates the tag on `rev`
//! (default: the parent of the working copy) and pushes it. A name already on another commit
//! exits 1 and moves nothing; a name already on the same commit exits 0.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use common::{Project, jj, outcome, run, write};

const TAG: &str = "zaf-v0.2.0";

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

/// A project whose root is a jj repository with two commits, `first` then `second`, pushed to a
/// bare remote, the working copy a new empty change on `second`, and `tag` in its actions folder.
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
    write(&root, "notes.txt", "one\n");
    jj(&root, &["commit", "-m", "First"]);
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Second"]);
    jj(&root, &["bookmark", "create", "main", "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", "main"]);

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    write(&actions, "tag", &action_text("tag"));
    fs::set_permissions(actions.join("tag"), fs::Permissions::from_mode(0o750)).expect("mode");
    project
}

/// `zaf do --more tag <args>` with its whole log: the exit code and the lines.
fn tag(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "tag"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

/// The commit a revision names in the root repository.
fn commit(project: &Project, revision: &str) -> String {
    jj(
        &project.root,
        &["log", "--no-graph", "-r", revision, "-T", "commit_id"],
    )
}

/// The commit the local tag names, or `None` while there is no such tag.
fn local_tag(project: &Project, name: &str) -> Option<String> {
    let ids = jj(
        &project.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            &format!("tags(exact:\"{name}\")"),
            "-T",
            "commit_id",
        ],
    );
    (!ids.is_empty()).then_some(ids)
}

/// The commit the bare remote's tag names, or `None` while the remote has no such tag.
fn remote_tag(project: &Project, name: &str) -> Option<String> {
    let remote = project.temp.path().join("remote.git");
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&remote)
        .args(["rev-parse", "--verify", "-q"])
        .arg(format!("refs/tags/{name}^{{commit}}"))
        .output()
        .expect("spawn");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[test]
fn tag_defaults_to_the_parent_of_the_working_copy_and_pushes_it() {
    let project = project();
    let second = commit(&project, "@-");

    let (code, text) = tag(&project, &[TAG]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(local_tag(&project, TAG), Some(second.clone()));
    assert_eq!(remote_tag(&project, TAG), Some(second.clone()));
    // The hand-back is the tagged commit.
    assert_eq!(text.lines().last(), Some(second.as_str()), "{text}");
}

#[test]
fn tag_takes_the_commit_a_revision_names() {
    let project = project();
    let first = commit(&project, "@--");

    let (code, text) = tag(&project, &[TAG, "@--"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(local_tag(&project, TAG), Some(first.clone()));
    assert_eq!(remote_tag(&project, TAG), Some(first.clone()));
    assert_eq!(text.lines().last(), Some(first.as_str()), "{text}");
}

#[test]
fn tag_on_another_commit_refuses() {
    let project = project();
    let first = commit(&project, "@--");
    let (code, text) = tag(&project, &[TAG, &first]);
    assert_eq!(code, 0, "{text}");

    // The same name on the parent of the working copy is another commit.
    let (code, text) = tag(&project, &[TAG]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("already on another commit"), "{text}");
    assert_eq!(local_tag(&project, TAG), Some(first.clone()));
    assert_eq!(remote_tag(&project, TAG), Some(first));
}

#[test]
fn tag_on_the_same_commit_exits_zero_and_changes_nothing() {
    let project = project();
    let second = commit(&project, "@-");
    let (code, text) = tag(&project, &[TAG]);
    assert_eq!(code, 0, "{text}");

    let (code, text) = tag(&project, &[TAG, &second]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(local_tag(&project, TAG), Some(second.clone()));
    assert_eq!(remote_tag(&project, TAG), Some(second.clone()));
    assert_eq!(text.lines().last(), Some(second.as_str()), "{text}");
}

#[test]
fn tag_on_the_same_commit_pushes_a_tag_the_remote_lacks() {
    let project = project();
    let second = commit(&project, "@-");
    // A tag made by hand is local only: the action finishes the job.
    jj(&project.root, &["tag", "set", TAG, "-r", &second]);
    assert_eq!(remote_tag(&project, TAG), None);

    let (code, text) = tag(&project, &[TAG]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(remote_tag(&project, TAG), Some(second));
}

#[test]
fn tag_refuses_a_name_the_remote_holds_on_another_commit() {
    let project = project();
    let first = commit(&project, "@--");
    // Someone else pushed the name to the remote; this repository has not fetched it.
    let temp = project.temp.path().to_owned();
    let remote_text = temp.join("remote.git").to_string_lossy().into_owned();
    run(&temp, "git", &["clone", "-q", &remote_text, "other"]);
    let other = temp.join("other");
    run(&other, "git", &["tag", TAG, &first]);
    run(&other, "git", &["push", "-q", "origin", TAG]);
    assert_eq!(local_tag(&project, TAG), None);

    let (code, text) = tag(&project, &[TAG]);

    assert_eq!(code, 1, "{text}");
    assert_eq!(remote_tag(&project, TAG), Some(first));
}

#[test]
fn tag_refuses_a_malformed_ask_and_pushes_nothing() {
    let project = project();

    let asks: [&[&str]; 8] = [
        &[],
        &["", "@-"],
        &["-v1"],
        &["quote\"d"],
        &["back\\slash"],
        &["two words"],
        &[TAG, "@-", "extra"],
        &[TAG, "no-such-revision"],
    ];
    for ask in asks {
        let (code, text) = tag(&project, ask);
        assert_eq!(code, 1, "{ask:?}: {text}");
    }
    // A revision that names more than one commit has no one commit to tag.
    let (code, text) = tag(&project, &[TAG, "@- | @--"]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(local_tag(&project, TAG), None);
    assert_eq!(remote_tag(&project, TAG), None);
}
