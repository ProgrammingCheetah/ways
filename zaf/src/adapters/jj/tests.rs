//! Integration tests against throwaway jj repositories with a bare git remote,
//! plus the pure parsers the adapter reads jj's output with.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-export-2";
const NOTES: &str = "one\ntwo\nthree\n";

/// A jj repository at `root`, its base bookmark pushed to the bare remote at `remote`.
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
    vcs: JjVcs,
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
        let remote = temp.path().join("remote.git");
        let root = temp.path().join("repo");
        run(temp.path(), "git", &["init", "-q", "--bare", "remote.git"]);
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
        let remote_text = remote.to_string_lossy();
        jj(&root, &["git", "remote", "add", "origin", &remote_text]);
        write(&root, "notes.txt", NOTES);
        jj(&root, &["commit", "-m", "Start the project"]);
        jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
        jj(&root, &["git", "push", "--bookmark", BASE]);
        let vcs = JjVcs::new(&root, &WorkspacesSection::default());
        Self {
            _temp: temp,
            root,
            remote,
            vcs,
        }
    }

    fn slice(&self) -> (SliceName, BaseName) {
        let (slice, base) = (SliceName::from(SLICE), BaseName::from(BASE));
        assert_eq!(self.vcs.open_slice(&slice, &base), Ok(SliceOpening::Opened));
        (slice, base)
    }

    fn slice_dir(&self, slice: &SliceName) -> PathBuf {
        self.vcs.slice_dir(slice).expect("slice dir")
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

    fn remote_target(&self, name: &str) -> String {
        let git_dir = self.remote.to_string_lossy();
        let reference = format!("refs/heads/{name}");
        let stdout = run(
            &self.root,
            "git",
            &["--git-dir", &git_dir, "rev-parse", &reference],
        );
        stdout.trim().to_owned()
    }

    fn workspaces(&self) -> String {
        jj(&self.root, &["--ignore-working-copy", "workspace", "list"])
    }
}

#[test]
fn slices_open_once_per_base_and_drop_quietly() {
    let fixture = Fixture::new(false);
    let (slice, base) = fixture.slice();
    let dir = fixture.slice_dir(&slice);
    assert!(dir.join(".jj").is_dir());
    assert!(dir.join("notes.txt").is_file());
    assert!(fixture.workspaces().contains(SLICE));
    assert_eq!(fixture.target(SLICE), fixture.target(BASE));
    assert_eq!(
        fixture.vcs.open_slice(&slice, &base),
        Ok(SliceOpening::AlreadyOpen)
    );
    let other_base = BaseName::from("feature/v2");
    assert!(matches!(
        fixture.vcs.open_slice(&slice, &other_base),
        Err(PortError::Conflict(_))
    ));
    fixture.vcs.drop_slice(&slice).expect("drop");
    assert!(!dir.exists());
    assert!(!dir.parent().expect("parent").exists());
    assert!(!fixture.workspaces().contains(SLICE));
    fixture
        .vcs
        .drop_slice(&slice)
        .expect("a second drop is quiet");
    assert!(matches!(
        fixture.vcs.is_clean(&slice),
        Err(PortError::NotFound(_))
    ));
    assert_eq!(
        fixture.vcs.open_slice(&slice, &base),
        Ok(SliceOpening::Opened)
    );
}

#[test]
fn a_missing_base_or_an_escaping_name_is_refused() {
    let fixture = Fixture::new(false);
    let slice = SliceName::from(SLICE);
    assert!(matches!(
        fixture
            .vcs
            .open_slice(&slice, &BaseName::from("feature/none")),
        Err(PortError::NotFound(_))
    ));
    let escaping = SliceName::from("../outside");
    assert!(matches!(
        fixture.vcs.open_slice(&escaping, &BaseName::from(BASE)),
        Err(PortError::Rejected(_))
    ));
}

#[test]
fn commit_and_publish_follow_the_work() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    assert_eq!(fixture.vcs.is_clean(&slice), Ok(true));
    assert!(matches!(
        fixture.vcs.commit(&slice, "empty"),
        Err(PortError::Rejected(_))
    ));
    write(
        &fixture.slice_dir(&slice),
        "src/export.rs",
        "fn export() {}\n",
    );
    assert_eq!(fixture.vcs.is_clean(&slice), Ok(false));
    let commit = fixture
        .vcs
        .commit(&slice, "Add the CSV writer")
        .expect("commit");
    assert_eq!(commit.as_ref(), fixture.target(SLICE));
    assert_eq!(fixture.vcs.is_clean(&slice), Ok(true));
    assert_eq!(fixture.vcs.publish(&slice), Ok(Publication::Published));
    assert_eq!(fixture.remote_target(SLICE), commit.as_ref());
    assert_eq!(
        fixture.vcs.publish(&slice),
        Ok(Publication::AlreadyPublished)
    );
    write(
        &fixture.slice_dir(&slice),
        "src/export.rs",
        "fn export2() {}\n",
    );
    let second = fixture.vcs.commit(&slice, "Rename").expect("commit");
    assert_ne!(second, commit);
    assert_eq!(fixture.vcs.publish(&slice), Ok(Publication::Published));
    assert_eq!(fixture.remote_target(SLICE), second.as_ref());
}

#[test]
fn publish_never_overwrites_work_the_remote_has() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    write(&fixture.slice_dir(&slice), "a.txt", "a\n");
    fixture.vcs.commit(&slice, "Add a").expect("commit");
    assert_eq!(fixture.vcs.publish(&slice), Ok(Publication::Published));
    let temp = fixture.root.parent().expect("temp").to_owned();
    let remote_text = fixture.remote.to_string_lossy();
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
    let remote_work = fixture.remote_target(SLICE);
    write(&fixture.slice_dir(&slice), "c.txt", "c\n");
    fixture.vcs.commit(&slice, "Add c").expect("commit");
    assert!(matches!(
        fixture.vcs.publish(&slice),
        Err(PortError::Conflict(_))
    ));
    assert_eq!(fixture.remote_target(SLICE), remote_work);
}

#[test]
fn base_merges_report_their_outcome() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(BaseMerge::UpToDate));
    fixture.advance_base("base-1.txt", "1\n");
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(BaseMerge::Merged));
    assert_eq!(fixture.target(SLICE), fixture.target(BASE));
    assert!(fixture.slice_dir(&slice).join("base-1.txt").is_file());
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(BaseMerge::UpToDate));
    write(&fixture.slice_dir(&slice), "slice.txt", "s\n");
    let own = fixture
        .vcs
        .commit(&slice, "Add slice work")
        .expect("commit");
    fixture.advance_base("base-2.txt", "2\n");
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(BaseMerge::Merged));
    let merge = fixture.target(SLICE);
    assert_ne!(merge, own.as_ref());
    let parents = jj(
        &fixture.root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            &format!("{}-", bookmark(SLICE)),
            "-T",
            r#"commit_id ++ "\n""#,
        ],
    );
    assert_eq!(parents.lines().count(), 2);
    assert!(fixture.slice_dir(&slice).join("base-2.txt").is_file());
    assert_eq!(fixture.vcs.is_clean(&slice), Ok(true));
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(BaseMerge::UpToDate));
}

#[test]
fn a_conflicting_merge_leaves_the_slice_unchanged() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    let slice_notes = "one\nslice\nthree\n";
    write(&fixture.slice_dir(&slice), "notes.txt", slice_notes);
    fixture.vcs.commit(&slice, "Edit notes").expect("commit");
    let before = fixture.target(SLICE);
    fixture.advance_base("notes.txt", "one\nbase\nthree\n");
    let conflicted = BaseMerge::Conflicted {
        paths: vec!["notes.txt".to_owned()],
    };
    assert_eq!(fixture.vcs.merge_base(&slice), Ok(conflicted));
    assert_eq!(fixture.target(SLICE), before);
    assert_eq!(fixture.vcs.is_clean(&slice), Ok(true));
    let notes = std::fs::read_to_string(fixture.slice_dir(&slice).join("notes.txt")).expect("read");
    assert_eq!(notes, slice_notes);
}

#[test]
fn merging_over_uncommitted_work_is_rejected() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    write(&fixture.slice_dir(&slice), "wip.txt", "wip\n");
    fixture.advance_base("base-1.txt", "1\n");
    assert!(matches!(
        fixture.vcs.merge_base(&slice),
        Err(PortError::Rejected(_))
    ));
}

#[test]
fn diffs_come_as_stat_first_and_hunks_on_request() {
    let fixture = Fixture::new(false);
    let (slice, _base) = fixture.slice();
    let dir = fixture.slice_dir(&slice);
    write(&dir, "notes.txt", "one\nTWO\nthree\n");
    fixture.vcs.commit(&slice, "Shout").expect("commit");
    write(&dir, "src/a b.rs", "fn a() {}\nfn b() {}\n");
    fixture.advance_base("base-1.txt", "1\n");
    let stat = fixture.vcs.diff_stat(&slice).expect("stat");
    assert_eq!(
        stat.to_string(),
        "notes.txt +1 -1\nsrc/a b.rs +2 -0\n2 files +3 -1\n"
    );
    let hunks = fixture.vcs.diff_hunks(&slice, "src/a b.rs").expect("hunks");
    assert!(hunks.contains("+fn a() {}\n+fn b() {}"), "{hunks}");
    assert!(!hunks.contains("notes.txt"), "{hunks}");
    assert!(matches!(
        fixture.vcs.diff_hunks(&slice, "src/c.rs"),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_colocated_repository_takes_slices_too() {
    let fixture = Fixture::new(true);
    let (slice, base) = fixture.slice();
    write(&fixture.slice_dir(&slice), "a.txt", "a\n");
    let commit = fixture.vcs.commit(&slice, "Add a").expect("commit");
    assert_eq!(fixture.vcs.publish(&slice), Ok(Publication::Published));
    assert_eq!(fixture.remote_target(SLICE), commit.as_ref());
    assert_eq!(
        fixture.vcs.open_slice(&slice, &base),
        Ok(SliceOpening::AlreadyOpen)
    );
    fixture.vcs.drop_slice(&slice).expect("drop");
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

#[test]
fn resolve_list_lines_yield_their_paths() {
    assert_eq!(
        command::conflicted_path("d i/x y.txt    2-sided conflict"),
        Some("d i/x y.txt".to_owned())
    );
    assert_eq!(
        command::conflicted_path("f.txt  3-sided conflict including 1 deletion"),
        Some("f.txt".to_owned())
    );
    assert_eq!(command::conflicted_path("Warning: nothing"), None);
}
