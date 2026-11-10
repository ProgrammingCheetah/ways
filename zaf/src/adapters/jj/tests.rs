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
