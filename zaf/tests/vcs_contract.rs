//! The Vcs port's contract: what every adapter behind [`Vcs`] does, checked the same way on
//! each.
//!
//! Each case is one function over an [`Arrange`]: an adapter's own way to put a repository, a
//! workspace and some work on disk (or in memory, for the fake), which the port itself does not
//! offer. The cases run on the in-memory fake, on the jj adapter against a real
//! `jj git init --colocate` repository and on the git adapter against a real `git init`
//! repository, each with a bare remote. A case for a tool that is not installed prints why it
//! is skipped and passes.
//!
//! The fixture every adapter starts from: a base `main` holding `notes.txt` (`one`, `two`) and
//! `kept.txt`, a repository with a remote called `origin`, and a workspace on `main` with no
//! work yet.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use zaf::adapters::git::GitVcs;
use zaf::adapters::jj::JjVcs;
use zaf::fakes::FakeVcs;
use zaf::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, SliceName, Vcs};

const NOTES: &str = "one\ntwo\n";
const KEPT: &str = "kept\n";

/// An adapter's way to arrange a repository for one case.
trait Arrange {
    /// The adapter under test.
    fn vcs(&self) -> &dyn Vcs;
    /// The repository's root.
    fn root(&self) -> &Path;
    /// The workspace a slice works in.
    fn workspace(&self) -> &Path;
    /// A directory that is no repository and no workspace.
    fn plain(&self) -> &Path;
    /// Gives the repository a remote, or takes it away.
    fn set_remote(&self, present: bool);
    /// Tags the base.
    fn tag(&self, name: &str);
    /// Writes `path` in the workspace and commits it; the new commit's id.
    fn commit(&self, path: &str, content: &str) -> CommitId;
    /// Writes `path` in the workspace and leaves it uncommitted: a change to a file the base
    /// holds, or an untracked new file.
    fn write(&self, path: &str, content: &str);
    /// An empty commit on top of `parent`, made without moving the workspace.
    fn empty_on(&self, parent: &CommitId) -> CommitId;
    /// A commit on top of `parent` whose tree is not its parent's, made without moving the
    /// workspace.
    fn changed_on(&self, parent: &CommitId) -> CommitId;
    /// A commit with `commit`'s tree and parent that is not `commit` and does not descend from
    /// it, made without moving the workspace.
    fn twin_of(&self, commit: &CommitId) -> CommitId;
    /// Leaves a merge of other work open in the workspace, conflict-free and not yet committed:
    /// what `sync-base` leaves when it merges the base in.
    fn open_merge(&self);
    /// Commits the open merge; the commit's id.
    fn finish_merge(&self) -> CommitId;
    /// What `git rev-parse <commit>^{tree}` says of `commit`, for an adapter over a git store;
    /// `None` for the fake, which has no store to ask.
    fn rev_parse_tree(&self, commit: &CommitId) -> Option<String>;
}

fn base() -> BaseName {
    BaseName::from("main")
}

/// The files of a stat, as `(path, added, removed)`.
fn listed(stat: &DiffStat) -> Vec<(&str, u32, u32)> {
    stat.files
        .iter()
        .map(
            |FileStat {
                 path,
                 added,
                 removed,
             }| (path.as_str(), *added, *removed),
        )
        .collect()
}

/// Work in the workspace: `added.txt` committed, `notes.txt` changed and not committed (one
/// line replaced, one added), `fresh.txt` untracked; `kept.txt` stays as the base has it.
fn work(arrange: &dyn Arrange) {
    arrange.commit("added.txt", "a\nb\n");
    arrange.write("notes.txt", "one\nTWO\nthree\n");
    arrange.write("fresh.txt", "x\ny\nz\n");
}

fn a_repository_with_a_remote_has_one(arrange: &dyn Arrange) {
    assert_eq!(arrange.vcs().has_remote(arrange.root()), Ok(true));
}

fn a_repository_without_a_remote_has_none(arrange: &dyn Arrange) {
    arrange.set_remote(false);
    assert_eq!(arrange.vcs().has_remote(arrange.root()), Ok(false));
}

fn tags_lists_every_tag(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    assert_eq!(vcs.tags(arrange.root()), Ok(Vec::new()));
    arrange.tag("app-v1.1.0");
    arrange.tag("app-v1.0.0");
    let mut tags = vcs.tags(arrange.root()).expect("tags");
    tags.sort();
    assert_eq!(tags, ["app-v1.0.0", "app-v1.1.0"]);
}

fn has_tag_is_true_for_a_tag_and_false_for_none(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.tag("app-v1.0.0");
    assert_eq!(vcs.has_tag(arrange.root(), "app-v1.0.0"), Ok(true));
    assert_eq!(vcs.has_tag(arrange.root(), "app-v2.0.0"), Ok(false));
    assert_eq!(
        vcs.has_tag(arrange.root(), "app-v1"),
        Ok(false),
        "a prefix is no tag"
    );
}

fn the_latest_commit_is_the_workspaces_last_commit(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    let committed = arrange.commit("added.txt", "a\nb\n");
    assert_eq!(
        vcs.latest_commit(arrange.workspace()),
        Ok(committed.clone())
    );
    arrange.write("notes.txt", "one\nTWO\n");
    arrange.write("fresh.txt", "x\n");
    assert_eq!(
        vcs.latest_commit(arrange.workspace()),
        Ok(committed.clone()),
        "uncommitted work is no part of it"
    );
    let next = arrange.commit("more.txt", "m\n");
    assert_ne!(next, committed);
    assert_eq!(vcs.latest_commit(arrange.workspace()), Ok(next));
}

fn the_diff_stat_counts_committed_unstaged_and_untracked_files(arrange: &dyn Arrange) {
    work(arrange);
    let stat = arrange
        .vcs()
        .diff_stat(arrange.workspace(), &base())
        .expect("stat");
    assert_eq!(
        listed(&stat),
        [
            ("added.txt", 2, 0),
            ("fresh.txt", 3, 0),
            ("notes.txt", 2, 1),
        ]
    );
}

fn the_diff_stat_of_a_workspace_without_work_is_empty(arrange: &dyn Arrange) {
    let stat = arrange
        .vcs()
        .diff_stat(arrange.workspace(), &base())
        .expect("stat");
    assert_eq!(stat, DiffStat::default());
}

fn the_hunks_of_a_committed_file(arrange: &dyn Arrange) {
    work(arrange);
    let hunks = arrange
        .vcs()
        .diff_hunks(arrange.workspace(), &base(), "added.txt")
        .expect("hunks");
    assert!(
        hunks.contains("diff --git a/added.txt b/added.txt"),
        "{hunks}"
    );
    assert!(hunks.contains("+a\n+b\n"), "{hunks}");
}

fn the_hunks_of_an_unstaged_file(arrange: &dyn Arrange) {
    work(arrange);
    let hunks = arrange
        .vcs()
        .diff_hunks(arrange.workspace(), &base(), "notes.txt")
        .expect("hunks");
    assert!(
        hunks.contains("diff --git a/notes.txt b/notes.txt"),
        "{hunks}"
    );
    assert!(hunks.contains("-two\n"), "{hunks}");
    assert!(hunks.contains("+TWO\n+three\n"), "{hunks}");
}

fn the_hunks_of_an_untracked_file(arrange: &dyn Arrange) {
    work(arrange);
    let hunks = arrange
        .vcs()
        .diff_hunks(arrange.workspace(), &base(), "fresh.txt")
        .expect("hunks");
    assert!(
        hunks.contains("diff --git a/fresh.txt b/fresh.txt"),
        "{hunks}"
    );
    assert!(hunks.contains("+x\n+y\n+z\n"), "{hunks}");
}

fn the_hunks_of_an_unchanged_file_are_not_found(arrange: &dyn Arrange) {
    work(arrange);
    let result = arrange
        .vcs()
        .diff_hunks(arrange.workspace(), &base(), "kept.txt");
    assert!(matches!(result, Err(PortError::NotFound(_))), "{result:?}");
}

/// A full commit id no repository holds.
fn unseen() -> CommitId {
    CommitId::from("0123456789abcdef0123456789abcdef01234567")
}

fn holds_an_empty_commit_on_top(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    let latest = vcs.latest_commit(arrange.workspace()).expect("latest");
    let empty = arrange.empty_on(&latest);
    assert_eq!(vcs.holds(arrange.workspace(), &empty), Ok(true));
    let emptier = arrange.empty_on(&empty);
    assert_eq!(vcs.holds(arrange.workspace(), &emptier), Ok(true));
}

fn holds_the_head_itself(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    arrange.write("notes.txt", "one\nTWO\n");
    let latest = vcs.latest_commit(arrange.workspace()).expect("latest");
    assert_eq!(
        vcs.holds(arrange.workspace(), &latest),
        Ok(true),
        "uncommitted work is no part of the latest commit"
    );
}

fn holds_false_for_a_head_with_a_different_tree(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    let latest = vcs.latest_commit(arrange.workspace()).expect("latest");
    let changed = arrange.changed_on(&latest);
    assert_eq!(vcs.holds(arrange.workspace(), &changed), Ok(false));
    let above = arrange.empty_on(&changed);
    assert_eq!(
        vcs.holds(arrange.workspace(), &above),
        Ok(false),
        "an empty commit on top of a change is no better"
    );
}

fn holds_false_for_a_head_off_the_line(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    let latest = vcs.latest_commit(arrange.workspace()).expect("latest");
    let twin = arrange.twin_of(&latest);
    assert_ne!(twin, latest);
    assert_eq!(vcs.holds(arrange.workspace(), &twin), Ok(false));
}

fn holds_false_for_a_head_the_repository_does_not_hold(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    assert_eq!(vcs.holds(arrange.workspace(), &unseen()), Ok(false));
}

fn vcs_contract_holds_false_while_merge_open(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    arrange.commit("added.txt", "a\nb\n");
    let head = vcs.latest_commit(arrange.workspace()).expect("latest");
    assert_eq!(vcs.holds(arrange.workspace(), &head), Ok(true));
    arrange.open_merge();
    assert_eq!(
        vcs.holds(arrange.workspace(), &head),
        Ok(false),
        "an open merge is past the head"
    );
    let merged = arrange.finish_merge();
    assert_ne!(merged, head);
    assert_eq!(vcs.latest_commit(arrange.workspace()), Ok(merged.clone()));
    assert_eq!(
        vcs.holds(arrange.workspace(), &merged),
        Ok(true),
        "the committed merge is held"
    );
}

fn vcs_contract_tree_of_matches_rev_parse(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    let root = arrange.root();
    let first = arrange.commit("added.txt", "a\nb\n");
    let second = arrange.commit("more.txt", "m\n");
    let first_tree = vcs.tree_of(root, &first).expect("tree of the first");
    let second_tree = vcs.tree_of(root, &second).expect("tree of the second");
    assert_ne!(first_tree, second_tree, "a change makes another tree");
    for (commit, tree) in [(&first, &first_tree), (&second, &second_tree)] {
        if let Some(expected) = arrange.rev_parse_tree(commit) {
            assert_eq!(tree.as_str(), expected, "the tree of {commit}");
        }
    }
    let twin = arrange.twin_of(&second);
    assert_eq!(
        vcs.tree_of(root, &twin),
        Ok(second_tree),
        "a commit beside it holds the same tree"
    );
    let result = vcs.tree_of(root, &unseen());
    assert!(matches!(result, Err(PortError::NotFound(_))), "{result:?}");
}

fn vcs_contract_empty_commit_same_tree(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    let root = arrange.root();
    let latest = arrange.commit("added.txt", "a\nb\n");
    let tree = vcs.tree_of(root, &latest).expect("tree");
    let empty = arrange.empty_on(&latest);
    assert_ne!(empty, latest);
    assert_eq!(vcs.tree_of(root, &empty), Ok(tree.clone()));
    let emptier = arrange.empty_on(&empty);
    assert_eq!(vcs.tree_of(root, &emptier), Ok(tree.clone()));
    let changed = arrange.changed_on(&latest);
    assert_ne!(vcs.tree_of(root, &changed), Ok(tree));
}

fn a_plain_directory_is_not_found(arrange: &dyn Arrange) {
    let vcs = arrange.vcs();
    let plain = arrange.plain();
    let is_not_found = |result: Result<(), PortError>, read: &str| {
        assert!(
            matches!(result, Err(PortError::NotFound(_))),
            "{read}: {result:?}"
        );
    };
    is_not_found(vcs.latest_commit(plain).map(drop), "latest_commit");
    is_not_found(vcs.diff_stat(plain, &base()).map(drop), "diff_stat");
    is_not_found(
        vcs.diff_hunks(plain, &base(), "notes.txt").map(drop),
        "diff_hunks",
    );
    is_not_found(vcs.has_tag(plain, "app-v1.0.0").map(drop), "has_tag");
    is_not_found(vcs.has_remote(plain).map(drop), "has_remote");
    is_not_found(vcs.tags(plain).map(drop), "tags");
    is_not_found(vcs.holds(plain, &unseen()).map(drop), "holds");
    is_not_found(vcs.tree_of(plain, &unseen()).map(drop), "tree_of");
}

// ---- the in-memory fake ----

/// The fake, told what a repository would hold: it keeps the files' lines to count a change the
/// way a diff does.
struct FakeArrange {
    vcs: FakeVcs,
    slice: SliceName,
    root: PathBuf,
    workspace: PathBuf,
    plain: PathBuf,
    files: RefCell<BTreeMap<String, String>>,
}

impl FakeArrange {
    fn new() -> Self {
        let vcs = FakeVcs::default();
        let slice = SliceName::from("feature/task");
        let root = PathBuf::from("/fake/repo");
        let workspace = PathBuf::from("/fake/work");
        vcs.set_root(&root);
        vcs.add_remote();
        vcs.open(&slice, &base());
        vcs.place(&slice, &workspace).expect("placed");
        let files = BTreeMap::from([
            ("notes.txt".to_owned(), NOTES.to_owned()),
            ("kept.txt".to_owned(), KEPT.to_owned()),
        ]);
        Self {
            vcs,
            slice,
            root,
            workspace,
            plain: PathBuf::from("/fake/plain"),
            files: RefCell::new(files),
        }
    }

    /// Records `content` for `path` against what the base holds: lines the base has no more
    /// are removed, lines it did not have are added, in a hunk of git's shape.
    fn edit(&self, path: &str, content: &str) {
        let base_content = match path {
            "notes.txt" => NOTES,
            "kept.txt" => KEPT,
            _ => "",
        };
        let old: Vec<&str> = base_content.lines().collect();
        let new: Vec<&str> = content.lines().collect();
        let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let tail = old[head..]
            .iter()
            .rev()
            .zip(new[head..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let removed = &old[head..old.len() - tail];
        let added = &new[head..new.len() - tail];
        let mut hunks = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@\n");
        for line in removed {
            hunks.push_str(&format!("-{line}\n"));
        }
        for line in added {
            hunks.push_str(&format!("+{line}\n"));
        }
        let (added, removed) = (added.len() as u32, removed.len() as u32);
        self.vcs
            .edit(&self.slice, path, added, removed, &hunks)
            .expect("edited");
        self.files
            .borrow_mut()
            .insert(path.to_owned(), content.to_owned());
    }
}

impl Arrange for FakeArrange {
    fn vcs(&self) -> &dyn Vcs {
        &self.vcs
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn plain(&self) -> &Path {
        &self.plain
    }

    fn set_remote(&self, present: bool) {
        if present {
            self.vcs.add_remote();
        } else {
            self.vcs.remove_remote();
        }
    }

    fn tag(&self, name: &str) {
        self.vcs.tag(name);
    }

    fn commit(&self, path: &str, content: &str) -> CommitId {
        self.edit(path, content);
        self.vcs
            .commit(&self.slice, &format!("Write {path}"))
            .expect("committed")
    }

    fn write(&self, path: &str, content: &str) {
        self.edit(path, content);
    }

    fn empty_on(&self, parent: &CommitId) -> CommitId {
        self.vcs.empty_commit(parent).expect("empty commit")
    }

    fn changed_on(&self, parent: &CommitId) -> CommitId {
        self.vcs.changed_commit(parent).expect("changed commit")
    }

    fn twin_of(&self, commit: &CommitId) -> CommitId {
        self.vcs.sibling_commit(commit).expect("sibling commit")
    }

    fn open_merge(&self) {
        self.vcs.open_merge(&self.slice).expect("merge opened");
    }

    fn finish_merge(&self) -> CommitId {
        self.vcs.finish_merge(&self.slice).expect("merge finished")
    }

    fn rev_parse_tree(&self, _commit: &CommitId) -> Option<String> {
        None
    }
}

// ---- the real tools ----

/// Whether `tool` runs; a case that needs one that does not says so and passes.
fn installed(tool: &str) -> bool {
    let found = Command::new(tool).arg("--version").output().is_ok();
    if !found {
        eprintln!("skipped: {tool} is not installed");
    }
    found
}

/// Runs `program` in `dir`, in an environment no user or system config reaches, and returns its
/// stdout.
fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut full = vec![
        "-c",
        "user.name=Test Author",
        "-c",
        "user.email=a@example.com",
        "-c",
        "commit.gpgsign=false",
    ];
    full.extend_from_slice(args);
    run(dir, "git", &full)
}

fn jj(dir: &Path, args: &[&str]) -> String {
    let mut full = vec!["--no-pager", "--color=never"];
    full.extend_from_slice(args);
    run(dir, "jj", &full)
}

fn write_file(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    std::fs::write(file, content).expect("write");
}

/// A bare remote with `main` in it, holding the fixture's two files, made through a seed clone.
fn bare_remote(temp: &Path) -> PathBuf {
    let origin = temp.join("origin.git");
    let origin_text = origin.to_str().expect("utf8");
    git(
        temp,
        &["init", "--quiet", "--bare", "-b", "main", origin_text],
    );
    let seed = temp.join("seed");
    git(temp, &["clone", "--quiet", origin_text, "seed"]);
    git(&seed, &["checkout", "--quiet", "-b", "main"]);
    write_file(&seed, "notes.txt", NOTES);
    write_file(&seed, "kept.txt", KEPT);
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "--quiet", "-m", "Start the project"]);
    git(&seed, &["push", "--quiet", "origin", "main"]);
    origin
}

/// A commit made in the git store at `root` with `git commit-tree`, so the workspace stays where
/// it is: `tree` (a git tree-ish) on top of `parent`. A branch keeps it, which is also how jj
/// comes to see a commit made behind its back.
fn side_commit(root: &Path, tree: &str, parent: &str) -> CommitId {
    let id = git(
        root,
        &[
            "commit-tree",
            tree,
            "-p",
            parent,
            "-m",
            &format!("Side of {tree}"),
        ],
    );
    git(root, &["branch", "--force", &format!("side-{id}"), &id]);
    CommitId::from(id.as_str())
}

/// The empty tree, written into the store at `root`: no work's tree equals it.
fn empty_tree(root: &Path) -> String {
    let output = Command::new("git")
        .args(["hash-object", "-t", "tree", "-w", "--stdin"])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn");
    assert!(output.status.success(), "hash-object failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The git adapter on a clone of the remote, with the slice in a linked worktree on a branch cut
/// from `origin/main`.
struct GitArrange {
    _temp: tempfile::TempDir,
    vcs: GitVcs,
    origin: PathBuf,
    root: PathBuf,
    workspace: PathBuf,
    plain: PathBuf,
}

impl GitArrange {
    fn new() -> Option<Self> {
        if !installed("git") {
            return None;
        }
        let temp = tempfile::tempdir().expect("temp dir");
        let origin = bare_remote(temp.path());
        let origin_text = origin.to_str().expect("utf8");
        let root = temp.path().join("repo");
        git(temp.path(), &["clone", "--quiet", origin_text, "repo"]);
        let workspace = temp.path().join("work");
        git(
            &root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "feature/task",
                workspace.to_str().expect("utf8"),
                "origin/main",
            ],
        );
        let plain = temp.path().join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        Some(Self {
            vcs: GitVcs::new(),
            origin,
            root,
            workspace,
            plain,
            _temp: temp,
        })
    }
}

impl Arrange for GitArrange {
    fn vcs(&self) -> &dyn Vcs {
        &self.vcs
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn plain(&self) -> &Path {
        &self.plain
    }

    fn set_remote(&self, present: bool) {
        if present {
            let origin = self.origin.to_str().expect("utf8");
            git(&self.root, &["remote", "add", "origin", origin]);
        } else {
            git(&self.root, &["remote", "remove", "origin"]);
        }
    }

    fn tag(&self, name: &str) {
        git(&self.root, &["tag", name, "origin/main"]);
    }

    fn commit(&self, path: &str, content: &str) -> CommitId {
        write_file(&self.workspace, path, content);
        git(&self.workspace, &["add", path]);
        git(
            &self.workspace,
            &["commit", "--quiet", "-m", &format!("Write {path}")],
        );
        CommitId::from(git(&self.workspace, &["rev-parse", "HEAD"]).as_str())
    }

    fn write(&self, path: &str, content: &str) {
        write_file(&self.workspace, path, content);
    }

    fn empty_on(&self, parent: &CommitId) -> CommitId {
        side_commit(&self.root, &format!("{parent}^{{tree}}"), parent.as_str())
    }

    fn changed_on(&self, parent: &CommitId) -> CommitId {
        side_commit(&self.root, &empty_tree(&self.root), parent.as_str())
    }

    fn twin_of(&self, commit: &CommitId) -> CommitId {
        side_commit(
            &self.root,
            &format!("{commit}^{{tree}}"),
            &format!("{commit}^"),
        )
    }

    fn open_merge(&self) {
        git(&self.root, &["branch", "side", "origin/main"]);
        git(&self.workspace, &["checkout", "--quiet", "side"]);
        write_file(&self.workspace, "other.txt", "o\n");
        git(&self.workspace, &["add", "other.txt"]);
        git(
            &self.workspace,
            &["commit", "--quiet", "-m", "Write other.txt"],
        );
        git(&self.workspace, &["checkout", "--quiet", "feature/task"]);
        git(
            &self.workspace,
            &["merge", "--quiet", "--no-ff", "--no-commit", "side"],
        );
        git(&self.workspace, &["rev-parse", "--verify", "MERGE_HEAD"]);
    }

    fn finish_merge(&self) -> CommitId {
        git(&self.workspace, &["commit", "--quiet", "--no-edit"]);
        CommitId::from(git(&self.workspace, &["rev-parse", "HEAD"]).as_str())
    }

    fn rev_parse_tree(&self, commit: &CommitId) -> Option<String> {
        Some(git(
            &self.root,
            &["rev-parse", &format!("{commit}^{{tree}}")],
        ))
    }
}

/// The jj adapter on a colocated repository with `main` pushed to a bare remote, and the slice
/// in a jj workspace on a bookmark of its own, the way `slice-open` leaves one.
struct JjArrange {
    _temp: tempfile::TempDir,
    vcs: JjVcs,
    origin: PathBuf,
    root: PathBuf,
    workspace: PathBuf,
    plain: PathBuf,
}

impl JjArrange {
    fn new() -> Option<Self> {
        if !installed("jj") || !installed("git") {
            return None;
        }
        let temp = tempfile::tempdir().expect("temp dir");
        let origin = bare_remote(temp.path());
        let origin_text = origin.to_str().expect("utf8");
        let root = temp.path().join("repo");
        jj(temp.path(), &["git", "init", "--colocate", "repo"]);
        jj(
            &root,
            &["config", "set", "--repo", "user.name", "Test Author"],
        );
        jj(
            &root,
            &["config", "set", "--repo", "user.email", "a@example.com"],
        );
        jj(&root, &["git", "remote", "add", "origin", origin_text]);
        jj(&root, &["git", "fetch", "--remote", "origin"]);
        jj(&root, &["bookmark", "track", "main", "--remote", "origin"]);
        jj(&root, &["new", "main"]);
        let workspace = temp.path().join("work");
        jj(&root, &["bookmark", "create", "feature/task", "-r", "main"]);
        jj(
            &root,
            &[
                "workspace",
                "add",
                "--name",
                "task",
                "-r",
                "feature/task",
                workspace.to_str().expect("utf8"),
            ],
        );
        let plain = temp.path().join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        Some(Self {
            vcs: JjVcs,
            origin,
            root,
            workspace,
            plain,
            _temp: temp,
        })
    }
}

impl Arrange for JjArrange {
    fn vcs(&self) -> &dyn Vcs {
        &self.vcs
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn plain(&self) -> &Path {
        &self.plain
    }

    fn set_remote(&self, present: bool) {
        if present {
            let origin = self.origin.to_str().expect("utf8");
            jj(&self.root, &["git", "remote", "add", "origin", origin]);
        } else {
            jj(&self.root, &["git", "remote", "remove", "origin"]);
        }
    }

    fn tag(&self, name: &str) {
        jj(&self.root, &["tag", "set", "-r", "main", name]);
    }

    fn commit(&self, path: &str, content: &str) -> CommitId {
        write_file(&self.workspace, path, content);
        jj(&self.workspace, &["commit", "-m", &format!("Write {path}")]);
        let id = jj(
            &self.workspace,
            &["log", "--no-graph", "-r", "@-", "-T", "commit_id"],
        );
        CommitId::from(id.as_str())
    }

    fn write(&self, path: &str, content: &str) {
        write_file(&self.workspace, path, content);
    }

    fn empty_on(&self, parent: &CommitId) -> CommitId {
        side_commit(&self.root, &format!("{parent}^{{tree}}"), parent.as_str())
    }

    fn changed_on(&self, parent: &CommitId) -> CommitId {
        side_commit(&self.root, &empty_tree(&self.root), parent.as_str())
    }

    fn twin_of(&self, commit: &CommitId) -> CommitId {
        side_commit(
            &self.root,
            &format!("{commit}^{{tree}}"),
            &format!("{commit}^"),
        )
    }

    fn open_merge(&self) {
        let own = jj(
            &self.workspace,
            &["log", "--no-graph", "-r", "@-", "-T", "commit_id"],
        );
        jj(&self.workspace, &["new", "main", "-m", "Side"]);
        write_file(&self.workspace, "other.txt", "o\n");
        let side = jj(
            &self.workspace,
            &["log", "--no-graph", "-r", "@", "-T", "commit_id"],
        );
        jj(&self.workspace, &["new", &own, &side]);
    }

    fn finish_merge(&self) -> CommitId {
        jj(&self.workspace, &["commit", "-m", "Merge the base"]);
        let id = jj(
            &self.workspace,
            &["log", "--no-graph", "-r", "@-", "-T", "commit_id"],
        );
        CommitId::from(id.as_str())
    }

    fn rev_parse_tree(&self, commit: &CommitId) -> Option<String> {
        Some(git(
            &self.root,
            &["rev-parse", &format!("{commit}^{{tree}}")],
        ))
    }
}

/// One test per case and adapter: `fake::<case>`, `jj::<case>` and `git::<case>`.
macro_rules! contract {
    ($($case:ident),* $(,)?) => {
        mod fake {
            $(
                #[test]
                fn $case() {
                    super::$case(&super::FakeArrange::new());
                }
            )*
        }

        mod jj {
            $(
                #[test]
                fn $case() {
                    let Some(arrange) = super::JjArrange::new() else {
                        return;
                    };
                    super::$case(&arrange);
                }
            )*
        }

        mod git {
            $(
                #[test]
                fn $case() {
                    let Some(arrange) = super::GitArrange::new() else {
                        return;
                    };
                    super::$case(&arrange);
                }
            )*
        }
    };
}

contract![
    a_repository_with_a_remote_has_one,
    a_repository_without_a_remote_has_none,
    tags_lists_every_tag,
    has_tag_is_true_for_a_tag_and_false_for_none,
    the_latest_commit_is_the_workspaces_last_commit,
    the_diff_stat_counts_committed_unstaged_and_untracked_files,
    the_diff_stat_of_a_workspace_without_work_is_empty,
    the_hunks_of_a_committed_file,
    the_hunks_of_an_unstaged_file,
    the_hunks_of_an_untracked_file,
    the_hunks_of_an_unchanged_file_are_not_found,
    holds_an_empty_commit_on_top,
    holds_the_head_itself,
    holds_false_for_a_head_with_a_different_tree,
    holds_false_for_a_head_off_the_line,
    holds_false_for_a_head_the_repository_does_not_hold,
    vcs_contract_holds_false_while_merge_open,
    vcs_contract_tree_of_matches_rev_parse,
    vcs_contract_empty_commit_same_tree,
    a_plain_directory_is_not_found,
];
