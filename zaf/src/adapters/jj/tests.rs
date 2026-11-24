//! Integration tests against throwaway jj repositories, plus the pure parsers the adapter
//! reads jj's output with.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-export-2";
const NOTES: &str = "one\ntwo\nthree\n";

/// A jj repository at `root` with a base bookmark; slice workspaces are made beside it.
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
}

fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {stderr}"
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn jj(dir: &Path, args: &[&str]) -> String {
    let mut full_args = vec!["--no-pager", "--color=never"];
    full_args.extend_from_slice(args);
    run(dir, "jj", &full_args)
}

fn write(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    std::fs::write(file, content).expect("write");
}

impl Fixture {
    fn new(colocate: bool) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("repo");
        let init_args: &[&str] = if colocate {
            &["git", "init", "--colocate", "repo"]
        } else {
            &["git", "init", "repo"]
        };
        jj(temp.path(), init_args);
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
        write(&root, "notes.txt", NOTES);
        jj(&root, &["commit", "-m", "Start the project"]);
        jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
        Self { _temp: temp, root }
    }

    /// A slice workspace on the base, the way a `slice-open` action leaves one.
    fn slice(&self) -> (PathBuf, BaseName) {
        let base_revset = bookmark(BASE);
        jj(
            &self.root,
            &["bookmark", "create", SLICE, "-r", &base_revset],
        );
        let dir = self
            .root
            .parent()
            .expect("temp")
            .join("workspaces")
            .join(SLICE);
        std::fs::create_dir_all(dir.parent().expect("parent")).expect("mkdir");
        let dir_text = dir.to_string_lossy();
        jj(
            &self.root,
            &[
                "workspace",
                "add",
                "--name",
                SLICE,
                "-r",
                &bookmark(SLICE),
                &dir_text,
            ],
        );
        (dir, BaseName::from(BASE))
    }

    /// Commits a file on the base from the root's own working copy.
    fn advance_base(&self, path: &str, content: &str) {
        jj(&self.root, &["new", &bookmark(BASE)]);
        write(&self.root, path, content);
        jj(&self.root, &["commit", "-m", "Advance the base"]);
        jj(&self.root, &["bookmark", "set", BASE, "-r", "@-"]);
    }

    fn target(&self, name: &str) -> String {
        jj(
            &self.root,
            &[
                "--ignore-working-copy",
                "log",
                "--no-graph",
                "-r",
                &bookmark(name),
                "-T",
                "commit_id",
            ],
        )
    }
}

#[test]
fn has_tag_says_whether_the_repository_holds_the_tag() {
    let fixture = Fixture::new(true);
    jj(&fixture.root, &["tag", "set", "-r", "@-", "app-v1.0.0"]);
    assert!(JjVcs.has_tag(&fixture.root, "app-v1.0.0").expect("tag"));
    assert!(!JjVcs.has_tag(&fixture.root, "app-v2.0.0").expect("tag"));
    assert!(
        !JjVcs
            .has_tag(&fixture.root, "app-v1")
            .expect("prefix is no tag")
    );
}

#[test]
fn jj_has_remote_false_without_remote() {
    let fixture = Fixture::new(true);
    assert!(!JjVcs.has_remote(&fixture.root).expect("remote"));
}

#[test]
fn jj_has_remote_true_with_origin() {
    let fixture = Fixture::new(false);
    jj(
        &fixture.root,
        &[
            "git",
            "remote",
            "add",
            "origin",
            "https://example.com/repo.git",
        ],
    );
    assert!(JjVcs.has_remote(&fixture.root).expect("remote"));
}

#[test]
fn jj_has_remote_errors_outside_a_repository() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        JjVcs.has_remote(temp.path()),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn tags_lists_every_tag_of_the_repository() {
    let fixture = Fixture::new(true);
    assert!(JjVcs.tags(&fixture.root).expect("tags").is_empty());
    jj(&fixture.root, &["tag", "set", "-r", "@-", "app-v1.0.0"]);
    jj(&fixture.root, &["tag", "set", "-r", "@-", "app-v0.9.0"]);
    assert_eq!(
        JjVcs.tags(&fixture.root).expect("tags"),
        ["app-v0.9.0", "app-v1.0.0"]
    );
}

#[test]
fn latest_commit_reads_the_workspace() {
    let fixture = Fixture::new(false);
    let (dir, _base) = fixture.slice();
    write(&dir, "by_hand.txt", "hand\n");
    jj(&dir, &["commit", "-m", "Made by hand"]);
    // The hand-made commit left the bookmark on the base; the read does not follow the bookmark.
    assert_eq!(fixture.target(SLICE), fixture.target(BASE));
    let hand_made = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    assert_ne!(hand_made, fixture.target(BASE));
    assert_eq!(
        JjVcs.latest_commit(&dir),
        Ok(CommitId::from(hand_made.as_str()))
    );
    // Uncommitted work is not a commit a bookmark would move to.
    write(&dir, "wip.txt", "wip\n");
    assert_eq!(
        JjVcs.latest_commit(&dir),
        Ok(CommitId::from(hand_made.as_str()))
    );
    jj(&dir, &["commit", "-m", "Finished"]);
    let latest = JjVcs.latest_commit(&dir).expect("latest commit");
    assert_ne!(latest.to_string(), hand_made);
    assert!(matches!(
        JjVcs.latest_commit(&dir.join("no-workspace")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_merge_in_progress_is_the_latest_commit() {
    let fixture = Fixture::new(false);
    let (dir, _base) = fixture.slice();
    write(&dir, "slice.txt", "slice\n");
    jj(&dir, &["commit", "-m", "On the slice"]);
    let slice = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    // A second line of work, then the working copy made a merge of both, as `sync-base` leaves it.
    jj(&dir, &["new", "root()"]);
    write(&dir, "other.txt", "other\n");
    jj(&dir, &["commit", "-m", "Elsewhere"]);
    let other = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    jj(&dir, &["new", &slice, &other]);
    let merge = jj(&dir, &["log", "--no-graph", "-r", "@", "-T", "commit_id"]);
    assert_eq!(
        JjVcs.latest_commit(&dir),
        Ok(CommitId::from(merge.as_str())),
        "the merge, never an error for two parents"
    );
    // Committed (which rewrites it, described), the merge is the parent like any other commit.
    jj(&dir, &["commit", "-m", "Merged"]);
    let merged = jj(&dir, &["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    assert_eq!(
        jj(
            &dir,
            &["log", "--no-graph", "-r", "@-", "-T", "parents.len()"]
        ),
        "2"
    );
    assert_eq!(
        JjVcs.latest_commit(&dir),
        Ok(CommitId::from(merged.as_str()))
    );
}

/// The commit id of a revset that names one commit.
fn id_of(dir: &Path, revset: &str) -> CommitId {
    CommitId::from(jj(dir, &["log", "--no-graph", "-r", revset, "-T", "commit_id"]).as_str())
}

/// A slice workspace with one commit of work, and that commit's id: the workspace's latest.
fn slice_with_work(fixture: &Fixture) -> (PathBuf, CommitId) {
    let (dir, _base) = fixture.slice();
    write(&dir, "work.txt", "work\n");
    jj(&dir, &["commit", "-m", "Work"]);
    let latest = id_of(&dir, "@-");
    assert_eq!(JjVcs.latest_commit(&dir), Ok(latest.clone()));
    (dir, latest)
}

#[test]
fn tree_of_is_one_for_an_empty_commit_and_another_for_a_change() {
    for colocate in [false, true] {
        let fixture = Fixture::new(colocate);
        let (dir, latest) = slice_with_work(&fixture);
        let root = fixture.root.as_path();
        let base = id_of(&dir, &bookmark(BASE));
        let work = JjVcs.tree_of(root, &latest).expect("tree of the work");
        assert_ne!(JjVcs.tree_of(root, &base), Ok(work.clone()));
        // The working copy is empty on top of the latest commit, and so is a commit above that.
        let empty = id_of(&dir, "@");
        assert_ne!(empty, latest);
        assert_eq!(JjVcs.tree_of(root, &empty), Ok(work.clone()));
        jj(&dir, &["new", "--no-edit", empty.as_ref(), "-m", "Empty"]);
        let emptier = id_of(&dir, &format!("children(commit_id(\"{empty}\"))"));
        assert_eq!(JjVcs.tree_of(&dir, &emptier), Ok(work));
        // A commit the repository does not hold, and text that is no commit id, are not found.
        let unseen = CommitId::from("0123456789abcdef0123456789abcdef01234567");
        assert!(matches!(
            JjVcs.tree_of(root, &unseen),
            Err(PortError::NotFound(_))
        ));
        assert!(matches!(
            JjVcs.tree_of(root, &CommitId::from("main")),
            Err(PortError::NotFound(_))
        ));
        // A directory that is no repository is not found.
        let nowhere = fixture.root.join("nowhere");
        assert!(matches!(
            JjVcs.tree_of(&nowhere, &latest),
            Err(PortError::NotFound(_))
        ));
    }
}

/// How many operations the repository's log holds, counted without a snapshot of its own.
fn operations(root: &Path) -> usize {
    jj(
        root,
        &[
            "--ignore-working-copy",
            "op",
            "log",
            "--no-graph",
            "-T",
            r#"id ++ "\n""#,
        ],
    )
    .lines()
    .count()
}

#[test]
fn gather_does_not_snapshot_root_working_copy() {
    for colocate in [false, true] {
        let fixture = Fixture::new(colocate);
        let root = fixture.root.as_path();
        let latest = id_of(root, "@-");
        // An uncommitted edit in the root: any jj command that snapshots would record it.
        write(root, "edited.txt", "not committed\n");
        let before = operations(root);
        let commits = [&latest];
        assert_eq!(JjVcs.trees_of(root, &commits).len(), 1);
        JjVcs.tree_of(root, &latest).expect("tree");
        JjVcs.has_tag(root, "app-v1.0.0").expect("tag");
        JjVcs.has_remote(root).expect("remote");
        JjVcs.tags(root).expect("tags");
        assert_eq!(
            operations(root),
            before,
            "a root read recorded an operation"
        );
    }
}

#[test]
fn trees_of_reads_each_commit_and_leaves_out_what_it_cannot() {
    let fixture = Fixture::new(true);
    let root = fixture.root.as_path();
    let (dir, latest) = slice_with_work(&fixture);
    let base = id_of(&dir, &bookmark(BASE));
    let unseen = CommitId::from("0123456789abcdef0123456789abcdef01234567");
    let asked = [&latest, &unseen, &base];
    let trees = JjVcs.trees_of(root, &asked);
    let ids: Vec<&CommitId> = trees.iter().map(|(commit, _)| commit).collect();
    assert_eq!(ids, [&latest, &base]);
    assert_eq!(
        trees[0].1,
        JjVcs.tree_of(root, &latest).expect("tree of the work")
    );
    let nowhere = fixture.root.join("nowhere");
    assert!(JjVcs.trees_of(&nowhere, &asked).is_empty());
}

#[test]
fn holds_an_empty_commit_on_top() {
    let fixture = Fixture::new(false);
    let (dir, latest) = slice_with_work(&fixture);
    // The latest commit holds itself.
    assert_eq!(JjVcs.holds(&dir, &latest), Ok(true));
    // The working copy, empty on top of the latest commit, holds its work; so does an empty
    // commit on top of that.
    let empty = id_of(&dir, "@");
    assert_eq!(JjVcs.holds(&dir, &empty), Ok(true));
    jj(&dir, &["new", "--no-edit", empty.as_ref(), "-m", "Empty"]);
    let emptier = id_of(&dir, &format!("children(commit_id(\"{empty}\"))"));
    assert_ne!(emptier, empty);
    assert_eq!(JjVcs.holds(&dir, &emptier), Ok(true));
}

#[test]
fn holds_false_for_a_different_tree() {
    let fixture = Fixture::new(false);
    let (dir, latest) = slice_with_work(&fixture);
    write(&dir, "more.txt", "more\n");
    let changed = id_of(&dir, "@");
    assert_ne!(changed, latest);
    assert_eq!(JjVcs.holds(&dir, &changed), Ok(false));
    // An empty commit on top of the one that changed something is no better.
    jj(&dir, &["new", "--no-edit", changed.as_ref(), "-m", "Above"]);
    let above = id_of(&dir, "description(\"Above\")");
    assert_eq!(JjVcs.holds(&dir, &above), Ok(false));
}

#[test]
fn holds_false_for_a_head_off_the_line() {
    let fixture = Fixture::new(false);
    let (dir, latest) = slice_with_work(&fixture);
    // A copy of the latest commit: the same tree, the same parent, and no descendant of it.
    jj(&dir, &["duplicate", latest.as_ref()]);
    let copy = id_of(
        &dir,
        &format!("description(\"Work\") ~ commit_id(\"{latest}\")"),
    );
    assert_ne!(copy, latest);
    assert_eq!(JjVcs.holds(&dir, &copy), Ok(false));
    // An ancestor, the base, is not a head that holds the work.
    let base = id_of(&dir, &bookmark(BASE));
    assert_eq!(JjVcs.holds(&dir, &base), Ok(false));
    // A head the repository does not have, never an error, and nothing that is no commit id.
    let unseen = CommitId::from("0123456789abcdef0123456789abcdef01234567");
    assert_eq!(JjVcs.holds(&dir, &unseen), Ok(false));
    for text in [
        "",
        "@",
        "nothing",
        "x | @",
        "\") | (\"",
        &latest.to_string()[..12],
    ] {
        assert_eq!(
            JjVcs.holds(&dir, &CommitId::from(text)),
            Ok(false),
            "{text}"
        );
    }
    assert!(matches!(
        JjVcs.holds(&dir.join("no-workspace"), &latest),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn diffs_come_as_stat_first_and_hunks_on_request() {
    let fixture = Fixture::new(false);
    let (dir, base) = fixture.slice();
    write(&dir, "notes.txt", "one\nTWO\nthree\n");
    jj(&dir, &["commit", "-m", "Shout"]);
    write(&dir, "src/a b.rs", "fn a() {}\nfn b() {}\n");
    fixture.advance_base("base-1.txt", "1\n");
    let stat = JjVcs.diff_stat(&dir, &base).expect("stat");
    assert_eq!(
        stat.to_string(),
        "notes.txt +1 -1\nsrc/a b.rs +2 -0\n2 files +3 -1\n"
    );
    let hunks = JjVcs.diff_hunks(&dir, &base, "src/a b.rs").expect("hunks");
    assert!(hunks.contains("+fn a() {}\n+fn b() {}"), "{hunks}");
    assert!(!hunks.contains("notes.txt"), "{hunks}");
    assert!(matches!(
        JjVcs.diff_hunks(&dir, &base, "src/c.rs"),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_directory_that_is_no_workspace_is_not_found() {
    let fixture = Fixture::new(false);
    let (_dir, base) = fixture.slice();
    let nowhere = fixture.root.join("not-a-workspace");
    assert!(matches!(
        JjVcs.diff_stat(&nowhere, &base),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_colocated_repository_reads_too() {
    let fixture = Fixture::new(true);
    let (dir, base) = fixture.slice();
    write(&dir, "a.txt", "a\n");
    let stat = JjVcs.diff_stat(&dir, &base).expect("stat");
    assert_eq!(stat.to_string(), "a.txt +1 -0\n1 files +1 -0\n");
    jj(&dir, &["commit", "-m", "Add a"]);
    assert!(JjVcs.latest_commit(&dir).is_ok());
}

#[test]
fn git_stat_counts_hunk_lines_per_file_in_path_order() {
    let diff = "\
diff --git a/z.txt b/z.txt
index 1..2 100644
--- a/z.txt
+++ b/z.txt
@@ -1,2 +1,2 @@
 keep
-old
+new
diff --git a/a dir/new file.txt b/a dir/new file.txt
new file mode 100644
--- /dev/null
+++ b/a dir/new file.txt
@@ -0,0 +1,2 @@
+--- not a header
+two
";
    let expected_files = vec![
        FileStat {
            path: "a dir/new file.txt".to_owned(),
            added: 2,
            removed: 0,
        },
        FileStat {
            path: "z.txt".to_owned(),
            added: 1,
            removed: 1,
        },
    ];
    assert_eq!(parse_git_stat(diff), expected_files);
}

#[test]
fn header_paths_take_the_new_side() {
    assert_eq!(header_path("a/x y.rs b/x y.rs"), "x y.rs");
    assert_eq!(header_path("a/old.rs b/new.rs"), "new.rs");
}

/// A repository with `notes.txt` and `src/lib.rs` committed, then `gone.txt` committed and
/// deleted; the second value is the commit before the deletion.
fn fixture_with_a_deleted_file() -> (Fixture, String) {
    let fixture = Fixture::new(true);
    write(&fixture.root, "src/lib.rs", "x\n");
    write(&fixture.root, "gone.txt", "x\n");
    jj(&fixture.root, &["commit", "-m", "Add files"]);
    let before = jj(
        &fixture.root,
        &["log", "--no-graph", "-r", "@-", "-T", "commit_id"],
    );
    std::fs::remove_file(fixture.root.join("gone.txt")).expect("remove");
    jj(&fixture.root, &["commit", "-m", "Delete gone.txt"]);
    (fixture, before.trim().to_owned())
}

#[test]
fn vcs_path_at_true_for_a_committed_file() {
    let (fixture, before) = fixture_with_a_deleted_file();
    let vcs = JjVcs;
    assert_eq!(vcs.path_at(&fixture.root, "@-", "notes.txt"), Ok(true));
    assert_eq!(vcs.path_at(&fixture.root, "@-", "src/lib.rs"), Ok(true));
    assert_eq!(vcs.path_at(&fixture.root, "@-", "src"), Ok(true));
    assert_eq!(vcs.path_at(&fixture.root, &before, "gone.txt"), Ok(true));
}

#[test]
fn vcs_path_at_false_for_a_deleted_file() {
    let (fixture, before) = fixture_with_a_deleted_file();
    let vcs = JjVcs;
    assert_eq!(vcs.path_at(&fixture.root, "@-", "gone.txt"), Ok(false));
    assert_eq!(vcs.path_at(&fixture.root, "@-", "never.txt"), Ok(false));
    assert_eq!(vcs.path_at(&fixture.root, &before, "gone"), Ok(false));
    assert_eq!(vcs.path_at(&fixture.root, &before, "lib.rs"), Ok(false));
}

#[test]
fn vcs_path_at_reads_a_bookmark_where_origin_has_it() {
    let (fixture, before) = fixture_with_a_deleted_file();
    let remote = fixture.root.parent().expect("temp").join("remote.git");
    let remote_text = remote.to_string_lossy();
    run(
        &fixture.root,
        "git",
        &["init", "--quiet", "--bare", &remote_text],
    );
    jj(
        &fixture.root,
        &["git", "remote", "add", "origin", &remote_text],
    );
    // The remote's `feature/x` is at the commit that still has `gone.txt`.
    jj(
        &fixture.root,
        &["bookmark", "create", "feature/x", "-r", &before],
    );
    jj(
        &fixture.root,
        &[
            "git",
            "push",
            "--remote",
            "origin",
            "--bookmark",
            "feature/x",
        ],
    );
    let vcs = JjVcs;
    assert_eq!(
        vcs.path_at(&fixture.root, "feature/x", "gone.txt"),
        Ok(true)
    );
    // A local bookmark that has moved off the remote's is not the one read.
    jj(&fixture.root, &["bookmark", "set", "feature/x", "-r", "@-"]);
    assert_eq!(
        vcs.path_at(&fixture.root, "feature/x", "gone.txt"),
        Ok(true)
    );
    // Nor is one needed: the bookmark on the remote alone is read.
    jj(&fixture.root, &["bookmark", "delete", "feature/x"]);
    assert_eq!(
        vcs.path_at(&fixture.root, "feature/x", "gone.txt"),
        Ok(true)
    );
    assert_eq!(
        vcs.path_at(&fixture.root, "feature/x", "never.txt"),
        Ok(false)
    );
}

#[test]
fn vcs_path_at_unknown_revision_is_an_error() {
    let (fixture, _before) = fixture_with_a_deleted_file();
    let vcs = JjVcs;
    let unseen = "0123456789abcdef0123456789abcdef01234567";
    for revision in ["no-such-bookmark", unseen, "--all", ""] {
        let Err(error) = vcs.path_at(&fixture.root, revision, "notes.txt") else {
            panic!("{revision:?} should be an error");
        };
        assert!(matches!(error, PortError::NotFound(_)), "{error:?}");
        assert!(error.to_string().contains(revision), "{error}");
    }
}
