//! Integration tests against throwaway git repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test Author",
            "-c",
            "user.email=a@example.com",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one commit and the two tags `app-v1.0.0` and `app-v1.1.0`.
fn repository() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).expect("mkdir");
    git(&root, &["init", "--quiet"]);
    std::fs::write(root.join("notes.txt"), "one\n").expect("write");
    git(&root, &["add", "notes.txt"]);
    git(&root, &["commit", "--quiet", "-m", "Start the project"]);
    git(&root, &["tag", "app-v1.1.0"]);
    git(&root, &["tag", "app-v1.0.0"]);
    (temp, root)
}

#[test]
fn has_remote_reads_the_remote() {
    let (_temp, root) = repository();
    git(
        &root,
        &["remote", "add", "origin", "https://example.com/app.git"],
    );
    assert!(GitVcs::new().has_remote(&root).expect("remote"));
}

#[test]
fn no_remote_is_false() {
    let (_temp, root) = repository();
    assert!(!GitVcs::new().has_remote(&root).expect("remote"));
}

#[test]
fn tags_lists_every_tag() {
    let (_temp, root) = repository();
    assert_eq!(
        GitVcs::new().tags(&root).expect("tags"),
        ["app-v1.0.0", "app-v1.1.0"]
    );
}

#[test]
fn has_tag_true_and_false() {
    let (_temp, root) = repository();
    let vcs = GitVcs::new();
    assert!(vcs.has_tag(&root, "app-v1.0.0").expect("tag"));
    assert!(!vcs.has_tag(&root, "app-v2.0.0").expect("tag"));
    assert!(!vcs.has_tag(&root, "app-v1").expect("a prefix is no tag"));
}

#[test]
fn not_a_repository_is_not_found() {
    let temp = tempfile::tempdir().expect("temp dir");
    let vcs = GitVcs::new();
    assert!(matches!(
        vcs.has_remote(temp.path()),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(vcs.tags(temp.path()), Err(PortError::NotFound(_))));
    assert!(matches!(
        vcs.has_tag(temp.path(), "app-v1.0.0"),
        Err(PortError::NotFound(_))
    ));
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A clone of a repository whose `main` holds `notes.txt` and `kept.txt`, with a branch cut from
/// `origin/main` in a linked worktree of the clone.
fn slice() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let origin = temp.path().join("origin.git");
    let origin_path = origin.to_str().expect("utf8");
    git(
        temp.path(),
        &["init", "--quiet", "--bare", "-b", "main", origin_path],
    );
    let seed = temp.path().join("seed");
    git(temp.path(), &["clone", "--quiet", origin_path, "seed"]);
    git(&seed, &["checkout", "--quiet", "-b", "main"]);
    std::fs::write(seed.join("notes.txt"), "one\ntwo\n").expect("write");
    std::fs::write(seed.join("kept.txt"), "kept\n").expect("write");
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "--quiet", "-m", "Start the project"]);
    git(&seed, &["push", "--quiet", "origin", "main"]);
    let main = temp.path().join("main");
    git(temp.path(), &["clone", "--quiet", origin_path, "main"]);
    let work = temp.path().join("work");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature/task",
            work.to_str().expect("utf8"),
            "origin/main",
        ],
    );
    (temp, main, work)
}

fn base() -> BaseName {
    BaseName::from("main")
}

/// In the workspace: a committed change (`added.txt` and a `.gitignore`), a modified-but-unstaged
/// file (`notes.txt`), an untracked file (`fresh.txt`), and files that stay out (`skipped.log`
/// by the `.gitignore`, `.zaf/state` by zaf).
fn change(work: &Path) {
    std::fs::write(work.join("added.txt"), "a\nb\n").expect("write");
    std::fs::write(work.join(".gitignore"), "*.log\n").expect("write");
    git(work, &["add", "added.txt", ".gitignore"]);
    git(work, &["commit", "--quiet", "-m", "Add a file"]);
    std::fs::write(work.join("notes.txt"), "one\ntwo\nthree\n").expect("write");
    std::fs::write(work.join("fresh.txt"), "x\ny\nz\n").expect("write");
    std::fs::write(work.join("skipped.log"), "log\n").expect("write");
    std::fs::create_dir_all(work.join(".zaf")).expect("mkdir");
    std::fs::write(work.join(".zaf/state"), "s\n").expect("write");
}

#[test]
fn latest_commit_in_a_linked_worktree() {
    let (_temp, main, work) = slice();
    assert!(
        work.join(".git").is_file(),
        "a linked worktree's .git is a file"
    );
    std::fs::write(work.join("added.txt"), "a\n").expect("write");
    git(&work, &["add", "added.txt"]);
    git(&work, &["commit", "--quiet", "-m", "Add a file"]);
    let vcs = GitVcs::new();
    let latest = vcs.latest_commit(&work).expect("latest commit");
    assert_eq!(
        latest.as_str(),
        git_out(&work, &["rev-parse", "HEAD"]).trim()
    );
    assert_ne!(
        latest.as_str(),
        git_out(&main, &["rev-parse", "HEAD"]).trim()
    );
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        vcs.latest_commit(temp.path()),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn diff_stat_counts_committed_unstaged_and_untracked() {
    let (_temp, _main, work) = slice();
    change(&work);
    let stat = GitVcs::new().diff_stat(&work, &base()).expect("stat");
    let listed: Vec<(&str, u32, u32)> = stat
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.added, file.removed))
        .collect();
    assert_eq!(
        listed,
        [
            (".gitignore", 1, 0),
            ("added.txt", 2, 0),
            ("fresh.txt", 3, 0),
            ("notes.txt", 1, 0),
        ]
    );
}

#[test]
fn diff_leaves_the_index_alone() {
    let (_temp, _main, work) = slice();
    change(&work);
    let before = git_out(&work, &["status", "--porcelain"]);
    GitVcs::new().diff_stat(&work, &base()).expect("stat");
    GitVcs::new()
        .diff_hunks(&work, &base(), "fresh.txt")
        .expect("hunks");
    assert_eq!(git_out(&work, &["status", "--porcelain"]), before);
    assert!(before.contains("?? fresh.txt"));
}

#[test]
fn diff_hunks_of_an_untracked_file() {
    let (_temp, _main, work) = slice();
    change(&work);
    let hunks = GitVcs::new()
        .diff_hunks(&work, &base(), "fresh.txt")
        .expect("hunks");
    assert!(hunks.contains("diff --git a/fresh.txt b/fresh.txt"));
    assert!(hunks.contains("+x\n+y\n+z\n"));
}

#[test]
fn diff_hunks_unchanged_is_not_found() {
    let (_temp, _main, work) = slice();
    change(&work);
    let result = GitVcs::new().diff_hunks(&work, &base(), "kept.txt");
    assert!(matches!(result, Err(PortError::NotFound(_))), "{result:?}");
}

#[test]
fn user_config_does_not_change_the_diff() {
    let (_temp, _main, work) = slice();
    change(&work);
    let vcs = GitVcs::new();
    let plain_stat = vcs.diff_stat(&work, &base()).expect("stat");
    let plain = vcs.diff_hunks(&work, &base(), "fresh.txt").expect("hunks");
    // Set in the repository's own config, which the adapter still reads.
    git(&work, &["config", "diff.external", "false"]);
    git(&work, &["config", "color.ui", "always"]);
    git(&work, &["config", "diff.noprefix", "true"]);
    assert_eq!(vcs.diff_stat(&work, &base()).expect("stat"), plain_stat);
    let hunks = vcs.diff_hunks(&work, &base(), "fresh.txt").expect("hunks");
    assert_eq!(hunks, plain);
    assert!(!hunks.contains("\x1b"));
}

/// The commit `HEAD` names in `dir`.
fn head_of(dir: &Path) -> CommitId {
    CommitId::from(git_out(dir, &["rev-parse", "HEAD"]).trim())
}

#[test]
fn holds_an_empty_commit_on_top() {
    let (_temp, _main, work) = slice();
    change(&work);
    let latest = head_of(&work);
    let vcs = GitVcs::new();
    // The latest commit holds itself.
    assert_eq!(vcs.holds(&work, &latest), Ok(true));
    // An empty commit on top of it holds it, and so does one on top of that.
    git(
        &work,
        &["commit", "--quiet", "--allow-empty", "-m", "Empty"],
    );
    let empty = head_of(&work);
    git(
        &work,
        &["commit", "--quiet", "--allow-empty", "-m", "Emptier"],
    );
    let emptier = head_of(&work);
    assert_ne!(empty, latest);
    git(&work, &["reset", "--quiet", "--hard", latest.as_str()]);
    assert_eq!(vcs.holds(&work, &empty), Ok(true));
    assert_eq!(vcs.holds(&work, &emptier), Ok(true));
}

#[test]
fn holds_false_for_a_different_tree() {
    let (_temp, _main, work) = slice();
    change(&work);
    let latest = head_of(&work);
    std::fs::write(work.join("more.txt"), "more\n").expect("write");
    git(&work, &["add", "more.txt"]);
    git(&work, &["commit", "--quiet", "-m", "Change"]);
    let changed = head_of(&work);
    git(
        &work,
        &["commit", "--quiet", "--allow-empty", "-m", "Above"],
    );
    let above = head_of(&work);
    git(&work, &["reset", "--quiet", "--hard", latest.as_str()]);
    let vcs = GitVcs::new();
    assert_eq!(vcs.holds(&work, &changed), Ok(false));
    // An empty commit on top of the one that changed something is no better.
    assert_eq!(vcs.holds(&work, &above), Ok(false));
}

#[test]
fn holds_false_for_a_head_off_the_line() {
    let (_temp, _main, work) = slice();
    change(&work);
    let latest = head_of(&work);
    // A twin of the latest commit: the same tree and parent, and no descendant of it.
    git(
        &work,
        &[
            "commit",
            "--quiet",
            "--amend",
            "--allow-empty",
            "-m",
            "A twin",
        ],
    );
    let twin = head_of(&work);
    assert_ne!(twin, latest);
    git(&work, &["reset", "--quiet", "--hard", latest.as_str()]);
    let vcs = GitVcs::new();
    assert_eq!(vcs.holds(&work, &twin), Ok(false));
    // An ancestor, the base, is no head that holds the work.
    let base = CommitId::from(git_out(&work, &["rev-parse", "HEAD~1"]).trim());
    assert_eq!(vcs.holds(&work, &base), Ok(false));
}

#[test]
fn holds_false_for_an_unknown_head() {
    let (_temp, _main, work) = slice();
    change(&work);
    let vcs = GitVcs::new();
    // A well-formed id the repository does not have, and nothing that is a full commit id.
    let unseen = CommitId::from("0123456789abcdef0123456789abcdef01234567");
    assert_eq!(vcs.holds(&work, &unseen), Ok(false));
    for text in [
        "",
        "HEAD",
        "main",
        "--all",
        "origin/main",
        "0123456789abcdef",
    ] {
        assert_eq!(vcs.holds(&work, &CommitId::from(text)), Ok(false), "{text}");
    }
    // A tree is not a commit.
    let tree = CommitId::from(git_out(&work, &["rev-parse", "HEAD^{tree}"]).trim());
    assert_eq!(vcs.holds(&work, &tree), Ok(false));
    // A directory that is no workspace is not found.
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        vcs.holds(temp.path(), &unseen),
        Err(PortError::NotFound(_))
    ));
}

/// A repository with `notes.txt` and `src/lib.rs` committed, then `gone.txt` committed and
/// deleted; the second value is the commit before the deletion.
fn repository_with_a_deleted_file() -> (tempfile::TempDir, PathBuf, String) {
    let (temp, root) = repository();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), "x\n").expect("write");
    std::fs::write(root.join("gone.txt"), "x\n").expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "Add files"]);
    let before = git_out(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&root, &["rm", "--quiet", "gone.txt"]);
    git(&root, &["commit", "--quiet", "-m", "Delete gone.txt"]);
    (temp, root, before)
}

#[test]
fn vcs_path_at_true_for_a_committed_file() {
    let (_temp, root, before) = repository_with_a_deleted_file();
    let vcs = GitVcs::new();
    assert_eq!(vcs.path_at(&root, "HEAD", "notes.txt"), Ok(true));
    assert_eq!(vcs.path_at(&root, "HEAD", "src/lib.rs"), Ok(true));
    assert_eq!(vcs.path_at(&root, "HEAD", "src"), Ok(true));
    assert_eq!(vcs.path_at(&root, &before, "gone.txt"), Ok(true));
}

#[test]
fn vcs_path_at_false_for_a_deleted_file() {
    let (_temp, root, before) = repository_with_a_deleted_file();
    let vcs = GitVcs::new();
    assert_eq!(vcs.path_at(&root, "HEAD", "gone.txt"), Ok(false));
    assert_eq!(vcs.path_at(&root, "HEAD", "never.txt"), Ok(false));
    // A prefix of a name is no path, and a path is read from the root of the tree.
    assert_eq!(vcs.path_at(&root, &before, "gone"), Ok(false));
    assert_eq!(vcs.path_at(&root, &before, "lib.rs"), Ok(false));
}

#[test]
fn vcs_path_at_unknown_revision_is_an_error() {
    let (_temp, root, _before) = repository_with_a_deleted_file();
    let vcs = GitVcs::new();
    let unseen = "0123456789abcdef0123456789abcdef01234567";
    for revision in ["no-such-branch", unseen, "--all", ""] {
        let Err(error) = vcs.path_at(&root, revision, "notes.txt") else {
            panic!("{revision:?} should be an error");
        };
        assert!(matches!(error, PortError::NotFound(_)), "{error:?}");
        assert!(error.to_string().contains(revision), "{error}");
    }
}

/// A clone of a repository whose `feature/x` branch holds `only-on-origin.txt`; the clone has the
/// branch on `origin` alone, as `base-open.git` leaves it.
fn clone_with_a_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let (temp, upstream) = repository();
    git(&upstream, &["branch", "-M", "main"]);
    git(&upstream, &["switch", "--quiet", "-c", "feature/x"]);
    std::fs::write(upstream.join("only-on-origin.txt"), "x\n").expect("write");
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "--quiet", "-m", "Add a file"]);
    git(&upstream, &["switch", "--quiet", "main"]);
    let clone = temp.path().join("clone");
    git(
        temp.path(),
        &["clone", "--quiet", path_text(&upstream), path_text(&clone)],
    );
    (temp, clone)
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

#[test]
fn vcs_path_at_reads_a_branch_that_is_only_on_origin() {
    let (_temp, clone) = clone_with_a_remote_branch();
    let vcs = GitVcs::new();
    assert_eq!(
        vcs.path_at(&clone, "feature/x", "only-on-origin.txt"),
        Ok(true)
    );
    assert_eq!(vcs.path_at(&clone, "feature/x", "never.txt"), Ok(false));
    // `HEAD` is the clone's own, never the remote's default branch.
    assert_eq!(vcs.path_at(&clone, "HEAD", "only-on-origin.txt"), Ok(false));
}

#[test]
fn vcs_path_at_reads_origin_over_a_stale_local_branch() {
    let (_temp, clone) = clone_with_a_remote_branch();
    // A local branch of the same name, left behind at the start of the project.
    git(&clone, &["branch", "--no-track", "feature/x", "main"]);
    let vcs = GitVcs::new();
    assert_eq!(
        vcs.path_at(&clone, "feature/x", "only-on-origin.txt"),
        Ok(true)
    );
}

#[test]
fn vcs_path_at_reads_a_local_branch_the_remote_does_not_have() {
    let (_temp, clone) = clone_with_a_remote_branch();
    git(&clone, &["branch", "--no-track", "local-only", "main"]);
    let vcs = GitVcs::new();
    assert_eq!(vcs.path_at(&clone, "local-only", "notes.txt"), Ok(true));
    assert_eq!(
        vcs.path_at(&clone, "local-only", "only-on-origin.txt"),
        Ok(false)
    );
}

#[test]
fn vcs_path_at_no_repository_is_not_found() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        GitVcs::new().path_at(temp.path(), "feature/x", "notes.txt"),
        Err(PortError::NotFound(_))
    ));
}
