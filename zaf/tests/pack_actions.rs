//! The teams pack carries the 25 standard actions a project copies into its `.zaf/actions/`
//! (docs/zaf-actions.md §2, §7). Every file in `plugins/teams/actions/` is executable, parses as an
//! action, and has the mode, group and phases of the ways repository's copy. Outside the ways
//! repository, where the pack is not beside the crate, nothing runs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use zaf::app::actions::{ActionMode, PhaseEnd, marks_jj, parse};

mod common;

use common::{Project, REPO, fixtures, outcome, path_with_zaf, run, write};

const NAMES: [&str; 25] = [
    "base-open",
    "ci",
    "ci-rerun",
    "ci-wait",
    "commit",
    "design",
    "fix",
    "implement",
    "merge",
    "merge-base",
    "objectives",
    "papers-commit",
    "pr-merge",
    "pr-open",
    "push",
    "release-open",
    "review",
    "review-epic",
    "slice-drop",
    "slice-merge",
    "slice-open",
    "slice-pr",
    "sync-base",
    "workspace-drop",
    "workspace-open",
];

/// The git versions the pack carries beside the jj ones (design addendum D28): each takes its
/// metadata from the ways repository's copy of the name without `.git`.
const GIT_NAMES: [&str; 9] = [
    "base-open.git",
    "commit.git",
    "merge-base.git",
    "papers-commit.git",
    "push.git",
    "release-open.git",
    "slice-pr.git",
    "workspace-drop.git",
    "workspace-open.git",
];

/// The actions the pack carries for the machine tier alone (design addendum D20): they install in
/// `~/.zaf/actions/`, so the ways repository's `.zaf/actions/` holds no copy to compare with.
const MACHINE_NAMES: [&str; 2] = ["bug-report", "bug-report-query"];

#[test]
fn pack_actions_parse() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let mut found: Vec<String> = fs::read_dir(&pack)
        .expect("the pack's actions folder reads")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .collect();
    found.sort();
    let mut expected: Vec<&str> = NAMES
        .iter()
        .chain(GIT_NAMES.iter())
        .chain(MACHINE_NAMES.iter())
        .copied()
        .collect();
    expected.sort_unstable();
    assert_eq!(
        found, expected,
        "the pack holds exactly the 25 actions, their git versions and the machine-tier ones"
    );

    for name in expected {
        let file = pack.join(name);
        let mode = fs::metadata(&file).expect("metadata").permissions().mode();
        assert_ne!(mode & 0o111, 0, "{name} is not executable");
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let parsed = parse(&file, &text).unwrap_or_else(|error| panic!("{error}"));
        if MACHINE_NAMES.contains(&name) {
            assert_eq!(
                parsed.metadata.mode,
                ActionMode::parse("755").expect("a mode")
            );
            continue;
        }

        let ways_name = name.strip_suffix(".git").unwrap_or(name);
        let ways_file = root.join(".zaf/actions").join(ways_name);
        let ways_text =
            fs::read_to_string(&ways_file).unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        let ways = parse(&ways_file, &ways_text).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(parsed.metadata.mode, ways.metadata.mode, "{name}: mode");
        assert_eq!(parsed.metadata.group, ways.metadata.group, "{name}: group");
        assert_eq!(
            parsed.metadata.phases, ways.metadata.phases,
            "{name}: phases"
        );
    }
}

/// The files in `pack` (a folder of actions) read as `(name, text)`, sorted by name.
fn pack_texts(pack: &Path) -> Vec<(String, String)> {
    let mut texts: Vec<(String, String)> = fs::read_dir(pack)
        .expect("the pack's actions folder reads")
        .map(|entry| {
            let name = entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(pack.join(&name))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            (name, text)
        })
        .collect();
    texts.sort();
    texts
}

/// The actions of `pack` marked `# zaf vcs: jj` (the metadata line only, as the runner reads it,
/// #490) that have no `<name>.git` beside them.
fn jj_without_git(pack: &Path) -> Vec<String> {
    pack_texts(pack)
        .into_iter()
        .filter(|(name, text)| !name.ends_with(".git") && marks_jj(text))
        .map(|(name, _)| name)
        .filter(|name| !pack.join(format!("{name}.git")).exists())
        .collect()
}

/// The `.git` files of `pack` with no file of the same name without `.git` beside them.
fn git_without_sibling(pack: &Path) -> Vec<String> {
    pack_texts(pack)
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| {
            name.strip_suffix(".git")
                .is_some_and(|base| !pack.join(base).exists())
        })
        .collect()
}

/// The `.git` files of `pack` whose exit codes (the numbers of the `# zaf <n>:` lines; the wording
/// names jj or git and so differs) are not those of their sibling.
fn git_with_other_codes(pack: &Path) -> Vec<String> {
    let codes = |file: &Path, text: &str| {
        parse(file, text)
            .unwrap_or_else(|error| panic!("{error}"))
            .metadata
            .codes
            .into_keys()
            .collect::<Vec<u8>>()
    };
    pack_texts(pack)
        .into_iter()
        .filter_map(|(name, text)| {
            let base = name.strip_suffix(".git")?.to_owned();
            let sibling = fs::read_to_string(pack.join(&base)).ok()?;
            let same = codes(&pack.join(&name), &text) == codes(&pack.join(&base), &sibling);
            (!same).then_some(name)
        })
        .collect()
}

fn pack_dir() -> Option<PathBuf> {
    let pack = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/actions");
    pack.is_dir().then_some(pack)
}

/// A git project takes `<name>.git` for an action marked `# zaf vcs: jj` (docs/zaf-actions.md §2,
/// design addendum Q5, D28), so each marked action of the pack has one. ways' own jj-only actions
/// under `.zaf/actions/` are out of reach: only the pack is read.
#[test]
fn every_jj_action_has_a_git_sibling() {
    let Some(pack) = pack_dir() else { return };
    let missing = jj_without_git(&pack);
    assert!(
        missing.is_empty(),
        "marked `# zaf vcs: jj` with no `<name>.git` beside them: {missing:?}"
    );
}

#[test]
fn every_git_action_has_a_sibling() {
    let Some(pack) = pack_dir() else { return };
    let orphans = git_without_sibling(&pack);
    assert!(
        orphans.is_empty(),
        "`.git` files with no sibling of the same name: {orphans:?}"
    );
}

#[test]
fn git_siblings_share_exit_codes() {
    let Some(pack) = pack_dir() else { return };
    let differing = git_with_other_codes(&pack);
    assert!(
        differing.is_empty(),
        "`.git` files whose exit-code lines differ from their sibling's: {differing:?}"
    );
}

/// The three checks name what they catch, on a pack built in a temporary folder.
#[test]
fn sibling_checks_name_what_they_catch() {
    let dir = std::env::temp_dir().join(format!("zaf-sibling-checks-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp folder");
    let action = |codes: &str, jj: bool| {
        format!(
            "#!/bin/sh\n# zaf description: x\n{codes}{}echo hi\n",
            if jj { "# zaf vcs: jj\n" } else { "" }
        )
    };
    let both = "# zaf 1: a\n# zaf 2: b\n";
    let put = |name: &str, text: String| fs::write(dir.join(name), text).expect("write");
    put("ok", action(both, true));
    put("ok.git", action(both, false));
    put("lonely", action(both, true));
    put("orphan.git", action(both, false));
    put("drift", action(both, true));
    put("drift.git", action("# zaf 1: a\n", false));
    put("plain", action(both, false));
    // A mention of jj outside the metadata line does not mark the action.
    put(
        "mention",
        "#!/bin/sh\n# zaf description: x\n# jj\necho vcs: jj\n".into(),
    );

    assert_eq!(jj_without_git(&dir), ["lonely"]);
    assert_eq!(git_without_sibling(&dir), ["orphan.git"]);
    assert_eq!(git_with_other_codes(&dir), ["drift.git"]);
    fs::remove_dir_all(&dir).expect("clean up");
}

/// Under a CI-only gate no review is ever posted on the forge, so the `review` phase ends when its
/// check exits 0 (docs/zaf-actions.md §5.1), in ways' copy and in the pack's alike.
#[test]
fn review_phase_ends_when_its_check_exits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let ways_file = root.join(".zaf/actions/review");
    let pack_file = root.join("plugins/teams/actions/review");
    if !pack_file.is_file() {
        return;
    }
    let read = |file: &PathBuf| {
        let text = fs::read_to_string(file).unwrap_or_else(|error| panic!("{file:?}: {error}"));
        parse(file, &text).unwrap_or_else(|error| panic!("{error}"))
    };
    let (ways, pack) = (read(&ways_file), read(&pack_file));

    assert_eq!(ways.metadata.ends, Some(PhaseEnd::Exit), "ways' review");
    assert_eq!(
        pack.metadata.ends,
        Some(PhaseEnd::Exit),
        "the pack's review"
    );
    assert_eq!(pack.metadata.kind, ways.metadata.kind, "kind");
    assert_eq!(pack.metadata.on_fail, ways.metadata.on_fail, "on-fail");
    assert_eq!(
        pack.metadata.description, ways.metadata.description,
        "description"
    );
}

/// The two Designer actions (docs/zaf-actions.md §7): `design` keeps the Engineer's terminal and is
/// the Engineer's alone; `review-epic` runs unattended under a declared bound and is no phase
/// action. Neither calls `gh`, and ways' copies are byte for byte the pack's.
#[test]
fn designer_actions_declare_their_terminal_and_mode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let read = |name: &str| {
        let file = pack.join(name);
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let ways = fs::read_to_string(root.join(".zaf/actions").join(name))
            .unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        assert_eq!(text, ways, "{name}: ways' copy is the pack's");
        assert!(
            !text
                .lines()
                .any(|line| line.split_whitespace().any(|word| word == "gh")),
            "{name} calls gh"
        );
        parse(&file, &text).unwrap_or_else(|error| panic!("{error}"))
    };

    let design = read("design").metadata;
    assert!(design.terminal, "design keeps the terminal");
    assert_eq!(design.mode, ActionMode::parse("700").expect("a mode"));
    assert_eq!((design.kind, design.ends), (None, None));

    let review = read("review-epic").metadata;
    assert!(!review.terminal, "review-epic runs unattended");
    assert_eq!(review.mode, ActionMode::DEFAULT);
    assert_eq!(review.timeout, Some(45));
    assert_eq!((review.kind, review.ends), (None, None));
    assert_eq!(review.codes.keys().copied().collect::<Vec<_>>(), [1, 2]);
}

/// A merge whose base conflicts starts `fix` (slice 8): `merge` declares `fix` as its `on-fail` and
/// tells its failures apart by code, and `sync-base`, which brings the base in for the programmer,
/// is a task action the implement and fix roles may run in every phase from `build` to `land`.
/// Ways' copies are byte for byte the pack's.
#[test]
fn merge_starts_fix_and_sync_base_is_the_programmers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let read = |name: &str| {
        let file = pack.join(name);
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let ways = fs::read_to_string(root.join(".zaf/actions").join(name))
            .unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        assert_eq!(text, ways, "{name}: ways' copy is the pack's");
        parse(&file, &text).unwrap_or_else(|error| panic!("{error}"))
    };

    let merge = read("merge").metadata;
    assert_eq!(merge.on_fail.as_ref().map(AsRef::as_ref), Some("fix"));
    assert_eq!(merge.codes.keys().copied().collect::<Vec<_>>(), [1, 2, 4]);
    assert!(
        merge.codes[&1].starts_with("conflicted"),
        "{:?}",
        merge.codes
    );

    let sync = read("sync-base").metadata;
    assert_eq!(sync.group, ["team", "release", "implement", "fix"]);
    assert_eq!(sync.mode, ActionMode::parse("750").expect("a mode"));
    assert_eq!(
        sync.phases
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["build", "check", "publish", "review", "land"]
    );
    assert_eq!((sync.kind, sync.ends), (None, None));
    // `sync-base` runs `merge-base` nested, under the fix or implement role it was called with.
    let merge_base = read("merge-base").metadata;
    assert_eq!(
        merge_base.group,
        ["team", "release", "implement", "fix", "merge"]
    );
    for name in ["fix", "slice-pr"] {
        read(name);
    }
}

/// The `commit` action's `--empty` option, in both copies. The tests sit in a module of the
/// file's name so `tools/run-test pack_actions::commit_empty_…` finds them.
mod pack_actions {
    use super::*;

    const COMMIT_SLICE: &str = "feature/v1-1-2";

    /// A jj workspace the way `workspace-open` leaves one (it records the base it was opened on),
    /// with one commit of work below an empty working copy.
    fn commit_slice() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path().join(".zaf/workspaces").join(COMMIT_SLICE);
        fs::create_dir_all(&dir).expect("workspace dir");
        let jj = |args: &[&str]| {
            let output = std::process::Command::new("jj")
                .args(["--no-pager", "--color=never"])
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("spawn jj");
            assert!(
                output.status.success(),
                "jj {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        jj(&["git", "init"]);
        jj(&["config", "set", "--repo", "user.name", "Test Author"]);
        jj(&[
            "config",
            "set",
            "--repo",
            "user.email",
            "author@example.com",
        ]);
        jj(&["config", "set", "--repo", "zaf.slice-base", "main"]);
        fs::write(dir.join("work.txt"), "work\n").expect("work");
        jj(&["commit", "-m", "The work"]);
        (temp, dir)
    }

    /// Runs one copy of `commit` on the slice, giving the exit code and what it printed.
    fn run_commit(copy: &str, temp: &tempfile::TempDir, args: &[&str]) -> (i32, String) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(copy)
            .join("commit");
        let output = std::process::Command::new("sh")
            .arg(&file)
            .args(args)
            .env("ZAF_ROOT", temp.path())
            .output()
            .expect("spawn sh");
        (
            output.status.code().expect("an exit code"),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    /// What jj prints of `template` for the revision `rev` of the workspace.
    fn commit_field(dir: &std::path::Path, rev: &str, template: &str) -> String {
        let output = std::process::Command::new("jj")
            .args([
                "--no-pager",
                "--color=never",
                "log",
                "--no-graph",
                "-r",
                rev,
            ])
            .args(["-T", template])
            .current_dir(dir)
            .output()
            .expect("spawn jj");
        assert!(output.status.success(), "jj log {rev}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    const COPIES: [&str; 2] = [".zaf/actions", "plugins/teams/actions"];

    // The git versions of `workspace-open`, `commit` and `workspace-drop`, run as scripts on a
    // throwaway repository with a bare `origin` holding `main`.

    const GIT_SLICE: &str = "feature/v1-1-2";

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
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
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A clone of a bare origin that holds `main` with one file, and the clone's directory.
    fn git_slice_project() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("temp dir");
        let origin = temp.path().join("origin.git");
        let root = temp.path().join("repo");
        git(
            temp.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        git(temp.path(), &["init", "-q", "-b", "main", "repo"]);
        git(
            &root,
            &["remote", "add", "origin", &origin.to_string_lossy()],
        );
        fs::write(root.join(".gitignore"), ".zaf/workspaces/\n").expect("ignore");
        fs::write(root.join("notes.txt"), "one\n").expect("notes");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Start"]);
        git(&root, &["push", "-q", "origin", "main"]);
        (temp, root)
    }

    /// Runs the pack's git version of `name`, giving the exit code and what it printed.
    fn run_git_action(
        root: &Path,
        name: &str,
        base: &str,
        vcs: &str,
        args: &[&str],
    ) -> (i32, String) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/teams/actions")
            .join(format!("{name}.git"));
        let output = std::process::Command::new("sh")
            .arg(&file)
            .args(args)
            .env("ZAF_ROOT", root)
            .env("ZAF_BASE", base)
            .env("ZAF_VCS", vcs)
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "a@example.com")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "a@example.com")
            .output()
            .expect("spawn sh");
        (
            output.status.code().expect("an exit code"),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    fn open_git_slice(root: &Path) -> PathBuf {
        let (code, text) =
            run_git_action(root, "workspace-open", "main", "git", &[GIT_SLICE, "main"]);
        assert_eq!(code, 0, "{text}");
        let dir = root.join(".zaf/workspaces").join(GIT_SLICE);
        assert_eq!(text.trim(), dir.to_string_lossy());
        dir
    }

    #[test]
    fn git_workspace_open_from_origin_base() {
        let (_temp, root) = git_slice_project();
        let dir = open_git_slice(&root);

        assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), GIT_SLICE);
        assert_eq!(
            git(&dir, &["rev-parse", "HEAD"]),
            git(&root, &["rev-parse", "refs/remotes/origin/main"])
        );
        // A second open hands the workspace back as it stands.
        let (code, text) = run_git_action(&root, "workspace-open", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(text.trim(), dir.to_string_lossy());
    }

    #[test]
    fn git_workspace_open_ignores_a_stale_local_base() {
        let (temp, root) = git_slice_project();
        // Another clone moves origin/main on; the root's local main stays behind.
        let other = temp.path().join("other");
        git(
            temp.path(),
            &[
                "clone",
                "-q",
                &temp.path().join("origin.git").to_string_lossy(),
                "other",
            ],
        );
        fs::write(other.join("later.txt"), "later\n").expect("later");
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "Go on"]);
        git(&other, &["push", "-q", "origin", "main"]);
        let stale = git(&root, &["rev-parse", "main"]);

        // Even a git config naming another base changes nothing: ZAF_BASE decides.
        git(&root, &["config", "zaf.slice-base", "elsewhere"]);
        let dir = open_git_slice(&root);

        let head = git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(head, git(&other, &["rev-parse", "HEAD"]));
        assert_ne!(head, stale);
        assert_eq!(git(&root, &["rev-parse", "main"]), stale);
    }

    #[test]
    fn git_commit_commits_new_and_edited_files() {
        let (_temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("notes.txt"), "one\ntwo\n").expect("edit");
        fs::write(dir.join("new.txt"), "new\n").expect("new");
        fs::create_dir_all(dir.join(".zaf/runs")).expect("zaf dir");
        fs::write(dir.join(".zaf/runs/state"), "x\n").expect("zaf state");

        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "The work"]);

        assert_eq!(code, 0, "{text}");
        let head = git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(text.trim(), head);
        assert_eq!(git(&dir, &["log", "-1", "--format=%s"]), "The work");
        let files = git(&dir, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(files.lines().collect::<Vec<_>>(), ["new.txt", "notes.txt"]);
        assert_eq!(git(&root, &["rev-parse", GIT_SLICE]), head);
    }

    #[test]
    fn git_commit_nothing_to_commit() {
        let (_temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("new.txt"), "new\n").expect("new");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "The work"]);
        assert_eq!(code, 0, "{text}");
        let head = git(&dir, &["rev-parse", "HEAD"]);

        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Again"]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("nothing to commit"), "{text}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
    }

    #[test]
    fn git_workspace_drop_keeps_the_branch() {
        let (_temp, root) = git_slice_project();
        let dir = open_git_slice(&root);

        let (code, text) = run_git_action(&root, "workspace-drop", "main", "git", &[GIT_SLICE]);

        assert_eq!(code, 0, "{text}");
        assert!(!dir.exists(), "the folder is gone");
        assert!(
            !root.join(".zaf/workspaces/feature").exists(),
            "empty parents go"
        );
        assert!(!git(&root, &["worktree", "list"]).contains(GIT_SLICE));
        assert_eq!(
            git(&root, &["rev-parse", "--verify", GIT_SLICE]),
            git(&root, &["rev-parse", "main"])
        );
        // A second drop is quiet.
        let (code, text) = run_git_action(&root, "workspace-drop", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 0, "{text}");
    }

    /// Commits `file` with `text` on `origin`'s `main` from a second clone.
    fn advance_origin(temp: &Path, file: &str, text: &str) {
        let other = temp.join("other");
        if !other.exists() {
            git(
                temp,
                &[
                    "clone",
                    "-q",
                    &temp.join("origin.git").to_string_lossy(),
                    "other",
                ],
            );
        }
        fs::write(other.join(file), text).expect("write");
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "Move the base"]);
        git(&other, &["push", "-q", "origin", "main"]);
    }

    fn merge_base(root: &Path, extra: &[&str]) -> (i32, String) {
        let mut args = vec![GIT_SLICE, "main"];
        args.extend_from_slice(extra);
        run_git_action(root, "merge-base", "main", "git", &args)
    }

    /// A slice with one commit of its own on `notes.txt`, and `origin/main` moved on over it.
    fn conflicting_pair() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("notes.txt"), "one\nslice\n").expect("slice edit");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Slice"]);
        assert_eq!(code, 0, "{text}");
        advance_origin(temp.path(), "notes.txt", "one\nbase\n");
        (temp, root, dir)
    }

    #[test]
    fn git_merge_base_clean_merge() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("mine.txt"), "mine\n").expect("slice file");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Slice"]);
        assert_eq!(code, 0, "{text}");
        advance_origin(temp.path(), "later.txt", "later\n");

        let (code, text) = merge_base(&root, &[]);

        assert_eq!(code, 0, "{text}");
        assert!(text.contains("merged main into"), "{text}");
        let origin_main = git(&dir, &["rev-parse", "refs/remotes/origin/main"]);
        let parents = git(&dir, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        assert!(parents.contains(&origin_main), "{parents}");
        assert_eq!(parents.split(' ').count(), 3, "a merge commit: {parents}");
        assert!(dir.join("later.txt").is_file() && dir.join("mine.txt").is_file());
        assert_eq!(git(&dir, &["status", "--porcelain"]), "");

        // Again: the base has nothing the slice lacks.
        let head = git(&dir, &["rev-parse", "HEAD"]);
        let (code, text) = merge_base(&root, &[]);
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("up to date"), "{text}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);

        // A call on another base or off a git project is refused.
        let (code, text) = run_git_action(&root, "merge-base", "main", "jj", &[GIT_SLICE, "main"]);
        assert_eq!(code, 2, "{text}");
        let (code, text) = run_git_action(&root, "merge-base", "main", "git", &[GIT_SLICE, "dev"]);
        assert_eq!(code, 2, "{text}");
    }

    #[test]
    fn git_merge_base_probe_leaves_the_workspace() {
        let (_temp, root, dir) = conflicting_pair();
        fs::write(dir.join("draft.txt"), "draft\n").expect("draft");
        let head = git(&dir, &["rev-parse", "HEAD"]);
        let status = git(&dir, &["status", "--porcelain"]);
        // The fetch moves the remote-tracking refs, as any fetch does; the branches stay.
        let local = [
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ];
        let refs = git(&root, &local);

        let (code, text) = merge_base(&root, &["--probe"]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("conflicts in:"), "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(&dir, &["status", "--porcelain"]), status);
        assert_eq!(git(&root, &local), refs);
        let merging = git_in_status(&dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
        assert!(!merging, "no merge was started");
    }

    #[test]
    fn git_merge_base_probe_answers_clean() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("mine.txt"), "mine\n").expect("slice file");
        run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Slice"]);
        advance_origin(temp.path(), "later.txt", "later\n");
        let head = git(&dir, &["rev-parse", "HEAD"]);

        let (code, text) = merge_base(&root, &["--probe"]);

        assert_eq!(code, 0, "{text}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
        assert!(!dir.join("later.txt").exists());
    }

    #[test]
    fn git_merge_base_conflict_stays_in_progress() {
        let (_temp, root, dir) = conflicting_pair();
        let head = git(&dir, &["rev-parse", "HEAD"]);

        let (code, text) = merge_base(&root, &[]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("conflicts in:"), "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        let merge_head = git(&dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
        assert_eq!(
            merge_head,
            git(&dir, &["rev-parse", "refs/remotes/origin/main"])
        );
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
        let notes = fs::read_to_string(dir.join("notes.txt")).expect("notes");
        assert!(notes.contains("<<<<<<<"), "{notes}");

        // Asked again before it is resolved, it refuses with the unmerged paths.
        let (code, text) = merge_base(&root, &["--keep"]);
        assert_eq!(code, 2, "{text}");
        assert!(text.contains("notes.txt"), "{text}");
    }

    /// A slice with a commit of its own on `notes.txt` and `other.txt`, `origin/main` moved on over
    /// both, and `merge-base --keep` stopped on the two conflicts.
    fn two_conflicts() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let (temp, root) = git_slice_project();
        fs::write(root.join("other.txt"), "one\n").expect("other");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Other"]);
        git(&root, &["push", "-q", "origin", "main"]);
        let dir = open_git_slice(&root);
        fs::write(dir.join("notes.txt"), "one\nslice\n").expect("slice notes");
        fs::write(dir.join("other.txt"), "one\nslice\n").expect("slice other");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Slice"]);
        assert_eq!(code, 0, "{text}");
        advance_origin(temp.path(), "notes.txt", "one\nbase\n");
        advance_origin(temp.path(), "other.txt", "one\nbase\n");
        let (code, text) = merge_base(&root, &["--keep"]);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains("notes.txt") && text.contains("other.txt"),
            "{text}"
        );
        (temp, root, dir)
    }

    #[test]
    fn commit_git_refuses_conflict_markers_in_open_merge() {
        let (_temp, root, dir) = two_conflicts();
        // One of the two is resolved by hand; the other still holds its markers.
        fs::write(dir.join("notes.txt"), "one\nboth\n").expect("resolve");
        let index = git(&dir, &["ls-files", "--stage"]);
        let head = git(&dir, &["rev-parse", "HEAD"]);

        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Merge"]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("other.txt"), "{text}");
        assert!(!text.contains("notes.txt"), "{text}");
        assert_eq!(git(&dir, &["ls-files", "--stage"]), index, "nothing staged");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
        git(&dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]);

        // Staged by hand with its markers, the file is still refused.
        git(&dir, &["add", "other.txt"]);
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Merge"]);
        assert_eq!(code, 1, "{text}");
        assert!(text.contains("other.txt"), "{text}");
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
    }

    #[test]
    fn commit_git_commits_resolved_merge() {
        let (_temp, root, dir) = two_conflicts();
        fs::write(dir.join("notes.txt"), "one\nboth\n").expect("resolve");
        fs::write(dir.join("other.txt"), "one\nboth\n").expect("resolve");

        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Merge"]);

        assert_eq!(code, 0, "{text}");
        let head = git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(text.trim(), head);
        git(&dir, &["rev-parse", "--verify", "HEAD^2"]);
        assert_eq!(git(&dir, &["status", "--porcelain"]), "");
    }

    #[test]
    fn git_merge_base_without_keep_refuses_uncommitted_work() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        advance_origin(temp.path(), "later.txt", "later\n");
        fs::write(dir.join("draft.txt"), "draft\n").expect("draft");

        let (code, text) = merge_base(&root, &[]);

        assert_eq!(code, 2, "{text}");
        assert!(text.contains("uncommitted work"), "{text}");
        assert!(!dir.join("later.txt").exists());
    }

    #[test]
    fn git_merge_base_keep_names_the_file_git_would_overwrite() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        advance_origin(temp.path(), "notes.txt", "one\nbase\n");
        fs::write(dir.join("notes.txt"), "one\ndraft\n").expect("draft");

        let (code, text) = merge_base(&root, &["--keep"]);

        assert_eq!(code, 2, "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        assert_eq!(
            fs::read_to_string(dir.join("notes.txt")).expect("notes"),
            "one\ndraft\n"
        );
    }

    /// Whether `git args` succeeds in `dir`.
    fn git_in_status(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git")
            .status
            .success()
    }

    /// `sync-base` runs `merge-base --keep` through `zaf do`; a stand-in `zaf` on the path hands
    /// that call to the pack's git version, which is what a git project's `zaf do` resolves.
    #[test]
    fn git_sync_base_keeps_uncommitted_work() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        advance_origin(temp.path(), "later.txt", "later\n");
        fs::write(dir.join("notes.txt"), "one\ndraft\n").expect("edit");
        fs::write(dir.join("draft.txt"), "draft\n").expect("draft");

        let pack = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/actions");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("bin");
        let shim = bin.join("zaf");
        fs::write(
            &shim,
            format!(
                "#!/bin/sh\n[ \"$1 $2 $3\" = \"do --more merge-base\" ] || exit 125\nshift 3\nexec sh {}/merge-base.git \"$@\"\n",
                pack.display()
            ),
        )
        .expect("shim");
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("mode");
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"));

        let output = std::process::Command::new("sh")
            .arg(pack.join("sync-base"))
            .arg("2")
            .env("PATH", path)
            .env("ZAF_ROOT", &root)
            .env("ZAF_SLICE", GIT_SLICE)
            .env("ZAF_BASE", "main")
            .env("ZAF_VCS", "git")
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "a@example.com")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "a@example.com")
            .output()
            .expect("spawn sh");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert_eq!(output.status.code(), Some(0), "{text}");
        assert!(text.contains("merged main into"), "{text}");
        assert!(dir.join("later.txt").is_file());
        // The uncommitted work is still there, uncommitted, and nothing was pushed.
        assert_eq!(
            fs::read_to_string(dir.join("notes.txt")).expect("notes"),
            "one\ndraft\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("draft.txt")).expect("draft"),
            "draft\n"
        );
        let status = git(&dir, &["status", "--porcelain"]);
        assert!(
            status.contains("notes.txt") && status.contains("draft.txt"),
            "{status}"
        );
        git(&dir, &["rev-parse", "--verify", "HEAD^2"]);
    }

    /// Each git version declares the exit codes of its jj sibling, and refuses off a git project.
    #[test]
    fn git_siblings_share_exit_codes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let pack = root.join("plugins/teams/actions");
        if !pack.is_dir() {
            return;
        }
        let read = |name: &str| {
            let file = pack.join(name);
            let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
            parse(&file, &text).unwrap_or_else(|error| panic!("{error}"))
        };
        for name in [
            "workspace-open",
            "commit",
            "workspace-drop",
            "merge-base",
            "papers-commit",
            "push",
            "slice-pr",
        ] {
            let (jj, git) = (read(name).metadata, read(&format!("{name}.git")).metadata);
            assert_eq!(
                git.codes.keys().collect::<Vec<_>>(),
                jj.codes.keys().collect::<Vec<_>>(),
                "{name}: exit codes"
            );
            assert_eq!(git.mode, jj.mode, "{name}: mode");
            assert_eq!(git.group, jj.group, "{name}: group");
            assert_eq!(git.phases, jj.phases, "{name}: phases");
        }

        let (_temp, project) = git_slice_project();
        for (name, args) in [
            ("workspace-open", vec![GIT_SLICE, "main"]),
            ("commit", vec![GIT_SLICE, "Msg"]),
            ("workspace-drop", vec![GIT_SLICE]),
            ("push", vec![GIT_SLICE]),
        ] {
            let (code, text) = run_git_action(&project, name, "main", "jj", &args);
            assert_eq!(code, 1, "{name}: {text}");
            let (code, text) = run_git_action(&project, name, "main", "git", &[]);
            assert_eq!(code, 1, "{name} usage: {text}");
        }
        assert!(!project.join(".zaf/workspaces").exists());
    }

    #[test]
    fn commit_copies_are_identical() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        if !root.join(COPIES[1]).is_dir() {
            return;
        }
        assert_eq!(
            fs::read(root.join(COPIES[0]).join("commit")).expect("ways' copy"),
            fs::read(root.join(COPIES[1]).join("commit")).expect("the pack's copy"),
        );
    }

    /// A pack git action and the project's copy of it, when the project keeps one, are the same bytes.
    #[test]
    fn action_copies_identical() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        if !root.join(COPIES[1]).is_dir() {
            return;
        }
        for name in ["commit.git", "slice-pr.git"] {
            let pack = fs::read(root.join(COPIES[1]).join(name)).expect("the pack's copy");
            if let Ok(ways) = fs::read(root.join(COPIES[0]).join(name)) {
                assert_eq!(ways, pack, "{name}: ways' copy is the pack's");
            }
        }
    }

    #[test]
    fn commit_empty_makes_one_empty_commit() {
        for copy in COPIES {
            let (temp, dir) = commit_slice();
            let work = commit_field(&dir, "@-", "commit_id");

            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "Re-run CI", "--empty"]);

            assert_eq!(code, 0, "{copy}: {text}");
            // One new commit, empty, on the work, with the message; its id is handed back.
            let made = commit_field(&dir, "@-", "commit_id");
            assert_eq!(text.lines().next(), Some(made.as_str()), "{copy}");
            assert_ne!(made, work, "{copy}");
            assert_eq!(commit_field(&dir, "@-", "empty"), "true", "{copy}");
            assert_eq!(
                commit_field(&dir, "@-", "description"),
                "Re-run CI",
                "{copy}"
            );
            assert_eq!(commit_field(&dir, "@--", "commit_id"), work, "{copy}");
            // The bookmark follows it, and the working copy is a fresh empty one.
            assert_eq!(
                commit_field(&dir, COMMIT_SLICE, "commit_id"),
                made,
                "{copy}"
            );
            assert_eq!(commit_field(&dir, "@", "empty"), "true", "{copy}");
        }
    }

    #[test]
    fn commit_empty_refuses_on_an_empty_latest() {
        for copy in COPIES {
            let (temp, dir) = commit_slice();
            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "Re-run CI", "--empty"]);
            assert_eq!(code, 0, "{copy}: {text}");
            let head = commit_field(&dir, "@-", "commit_id");

            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "Again", "--empty"]);

            assert_eq!(code, 1, "{copy}: {text}");
            assert!(text.contains("already empty"), "{copy}: {text}");
            assert_eq!(commit_field(&dir, "@-", "commit_id"), head, "{copy}");
        }
    }

    #[test]
    fn commit_empty_refuses_with_uncommitted_work() {
        for copy in COPIES {
            let (temp, dir) = commit_slice();
            let work = commit_field(&dir, "@-", "commit_id");
            fs::write(dir.join("more.txt"), "more\n").expect("more work");

            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "Re-run CI", "--empty"]);

            assert_eq!(code, 1, "{copy}: {text}");
            assert!(text.contains("uncommitted work"), "{copy}: {text}");
            assert_eq!(commit_field(&dir, "@-", "commit_id"), work, "{copy}");
        }
    }

    #[test]
    fn commit_without_empty_is_as_before() {
        for copy in COPIES {
            let (temp, dir) = commit_slice();
            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "Nothing"]);
            assert_eq!(code, 1, "{copy}: {text}");
            assert!(text.contains("nothing to commit"), "{copy}: {text}");

            fs::write(dir.join("more.txt"), "more\n").expect("more work");
            let (code, text) = run_commit(copy, &temp, &[COMMIT_SLICE, "More"]);
            assert_eq!(code, 0, "{copy}: {text}");
            assert_eq!(commit_field(&dir, "@-", "empty"), "false", "{copy}");
        }
    }

    // `ci-rerun` (design-addendum D20, F22): it runs as a script in a project whose `zaf` is the
    // built one, over the fake `gh` as the forge, with the real `commit` and `push` actions on a
    // jj workspace that pushes to a bare git remote.

    /// A commit the forge can hold as the head while the workspace's latest is another.
    const OTHER_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A project whose forge answers `head` and `checks`, with `ci-rerun` as `copy` ships it, the
    /// shipped `commit` and `push`, and the slice's workspace pushed once to a bare remote.
    fn rerun_scene(copy: &str, checks: &str) -> (Project, PathBuf, PathBuf) {
        use std::process::Command;
        let mut project = Project::new(MANIFEST);
        project
            .on("pr checks *", checks)
            .env("PATH", &path_with_zaf(&[&fixtures()]));
        // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
        let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        let root = repository();
        for name in ["ci-rerun", "commit", "push"] {
            let text = fs::read_to_string(root.join(copy).join(name)).expect("the action");
            write(&project.root, &format!(".zaf/actions/{name}"), &text);
            fs::set_permissions(
                project.root.join(".zaf/actions").join(name),
                fs::Permissions::from_mode(0o750),
            )
            .expect("mode");
        }
        let remote = project.temp.path().join("remote.git");
        let made = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .expect("git init");
        assert!(made.status.success(), "git init --bare");
        let dir = project.root.join(".zaf/workspaces").join(COMMIT_SLICE);
        fs::create_dir_all(&dir).expect("workspace dir");
        let jj = |args: &[&str]| {
            let output = Command::new("jj")
                .args(["--no-pager", "--color=never"])
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("spawn jj");
            assert!(
                output.status.success(),
                "jj {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        jj(&["git", "init"]);
        jj(&["config", "set", "--repo", "user.name", "Test Author"]);
        jj(&[
            "config",
            "set",
            "--repo",
            "user.email",
            "author@example.com",
        ]);
        jj(&["config", "set", "--repo", "zaf.slice-base", "main"]);
        jj(&["git", "remote", "add", "origin", &remote.to_string_lossy()]);
        fs::write(dir.join("work.txt"), "work\n").expect("work");
        jj(&["commit", "-m", "The work"]);
        jj(&["bookmark", "create", COMMIT_SLICE, "-r", "@-"]);
        let (code, text) = run_script(&project, "push", &[COMMIT_SLICE]);
        assert_eq!(code, 0, "{text}");
        // The forge holds the slice's latest commit as the pull request's head.
        forge_head(&mut project, &dir);
        (project, dir, remote)
    }

    /// The forge now answers the workspace's latest commit as the pull request's head.
    fn forge_head(project: &mut Project, dir: &Path) {
        let head = commit_field(dir, "@-", "commit_id");
        project.on_first("pr view *", &format!("{head}\n"));
    }

    /// Runs the project's copy of action `name` as a script with `target: task`'s environment.
    fn run_script(project: &Project, name: &str, args: &[&str]) -> (i32, String) {
        let file = project.root.join(".zaf/actions").join(name);
        let mut arguments = vec![file.to_string_lossy().into_owned()];
        arguments.extend(args.iter().map(|arg| (*arg).to_owned()));
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let output = project
            .command_of(Path::new("/bin/sh"), &project.root, &arguments)
            .env("ZAF_ROOT", &project.root)
            .env("ZAF_SLICE", COMMIT_SLICE)
            .env("ZAF_PR", "7")
            .output()
            .expect("spawn sh");
        (
            output.status.code().expect("an exit code"),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    /// The commits on the remote's slice branch, and its tip.
    fn remote_branch(remote: &Path) -> (usize, String) {
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(remote)
                .args(args)
                .output()
                .expect("spawn git");
            assert!(output.status.success(), "git {args:?}");
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        let branch = format!("refs/heads/{COMMIT_SLICE}");
        let count = git(&["rev-list", "--count", &branch]);
        (
            count.parse().expect("a count"),
            git(&["rev-parse", &branch]),
        )
    }

    const RED: &str = "fail\tzaf / test\thttps://example.test/job/1\npass\tzaf / lint\thttps://example.test/job/2\n";
    const GREEN: &str = "pass\tzaf / test\thttps://example.test/job/1\n";

    #[test]
    fn ci_rerun_copies_are_identical_and_say_what_changed() {
        let root = repository();
        if !root.join(COPIES[1]).is_dir() {
            return;
        }
        let text = fs::read_to_string(root.join(COPIES[1]).join("ci-rerun")).expect("pack copy");
        let ways = fs::read_to_string(root.join(COPIES[0]).join("ci-rerun")).expect("ways' copy");
        assert_eq!(text, ways, "ways' copy is the pack's");
        let parsed = parse(&root.join(COPIES[1]).join("ci-rerun"), &text)
            .unwrap_or_else(|error| panic!("{error}"));
        let metadata = parsed.metadata;
        assert_eq!(metadata.mode, ActionMode::parse("750").expect("a mode"));
        assert_eq!(metadata.group, ["fix"]);
        assert!(metadata.target.is_some(), "a task action");
        assert_eq!(metadata.codes.keys().copied().collect::<Vec<_>>(), [1, 2]);
        let after_block = text
            .lines()
            .find(|line| !line.starts_with("#!") && !line.starts_with("# zaf "))
            .expect("a body");
        assert!(after_block.starts_with("# changed:"), "{after_block}");
    }

    #[test]
    fn ci_rerun_runs_when_head_is_latest() {
        for copy in COPIES {
            let (project, dir, remote) = rerun_scene(copy, RED);
            let (before, _) = remote_branch(&remote);
            let head = commit_field(&dir, "@-", "commit_id");

            let (code, text) = run_script(&project, "ci-rerun", &["42"]);

            assert_eq!(code, 0, "{copy}: {text}");
            let (after, tip) = remote_branch(&remote);
            assert_eq!(after, before + 1, "{copy}: exactly one commit pushed");
            assert_eq!(tip, commit_field(&dir, "@-", "commit_id"), "{copy}");
            assert_eq!(commit_field(&dir, "@-", "empty"), "true", "{copy}");
            assert_eq!(
                commit_field(&dir, "@-", "description").trim(),
                format!("Re-run CI at {}", &head[..12]),
                "{copy}"
            );
        }
    }

    #[test]
    fn ci_rerun_refuses_with_no_failed_check() {
        for copy in COPIES {
            for checks in [GREEN, ""] {
                let (project, dir, remote) = rerun_scene(copy, checks);
                let before = remote_branch(&remote);
                let work = commit_field(&dir, "@-", "commit_id");

                let (code, text) = run_script(&project, "ci-rerun", &["42"]);

                assert_eq!(code, 1, "{copy}: {text}");
                assert!(text.contains("no check failed"), "{copy}: {text}");
                assert_eq!(remote_branch(&remote), before, "{copy}: nothing pushed");
                assert_eq!(commit_field(&dir, "@-", "commit_id"), work, "{copy}");
            }
        }
    }

    #[test]
    fn ci_rerun_refuses_empty_head() {
        for copy in COPIES {
            let (mut project, dir, remote) = rerun_scene(copy, RED);
            let (code, text) = run_script(&project, "ci-rerun", &["42"]);
            assert_eq!(code, 0, "{copy}: {text}");
            let pushed = remote_branch(&remote);
            let tip = commit_field(&dir, "@-", "commit_id");

            // The forge's head is now that empty commit: a second re-run is refused.
            forge_head(&mut project, &dir);
            let (code, text) = run_script(&project, "ci-rerun", &["42"]);

            assert_eq!(code, 1, "{copy}: {text}");
            assert!(text.contains("already empty"), "{copy}: {text}");
            assert_eq!(remote_branch(&remote), pushed, "{copy}: nothing pushed");
            assert_eq!(commit_field(&dir, "@-", "commit_id"), tip, "{copy}");
            assert_eq!(
                commit_field(&dir, "@", "empty"),
                "true",
                "{copy}: no commit made"
            );
        }
    }

    #[test]
    fn ci_rerun_refuses_head_not_latest() {
        for copy in COPIES {
            let (mut project, dir, remote) = rerun_scene(copy, RED);
            // The forge's head is a commit the workspace's latest is not, as after an empty
            // commit pushed by hand.
            project.on_first("pr view *", &format!("{OTHER_HEAD}\n"));
            let before = remote_branch(&remote);
            let tip = commit_field(&dir, "@-", "commit_id");

            let (code, text) = run_script(&project, "ci-rerun", &["42"]);

            assert_eq!(code, 1, "{copy}: {text}");
            assert!(text.contains("not the latest commit"), "{copy}: {text}");
            assert_eq!(remote_branch(&remote), before, "{copy}: nothing pushed");
            assert_eq!(commit_field(&dir, "@-", "commit_id"), tip, "{copy}");
            assert_eq!(
                commit_field(&dir, "@", "empty"),
                "true",
                "{copy}: no commit made"
            );
        }
    }

    // `fix` runs `ci-rerun` before its programmer (design-addendum D20, F22). The scene is the
    // re-run's, with `fix` beside it, a stand-in `claude` that is the programmer, and a `zaf`
    // that answers `brief` and `do slice-pr`, runs the shipped `ci-rerun` for `do ci-rerun`, and
    // leaves the rest to the built one. Every call of either lands in one log.

    /// The fix scene and the log of the programmer's and the verbs' calls.
    fn fix_scene(copy: &str, checks: &str) -> (Project, PathBuf, PathBuf, PathBuf) {
        let (mut project, dir, remote) = rerun_scene(copy, checks);
        let text = fs::read_to_string(repository().join(copy).join("fix")).expect("fix");
        write(&project.root, ".zaf/actions/fix", &text);
        let shim_dir = project.temp.path().join("fix-shim");
        let log = project.temp.path().join("fix.log");
        let real = env!("CARGO_BIN_EXE_zaf");
        let zaf = format!(
            "#!/bin/sh\necho \"zaf $*\" >> '{log}'\ncase \"$*\" in\n    \"brief \"*) printf '%s\\n%s\\n' /nonexistent/pack 1; exit 0 ;;\n    \"do slice-pr \"*) exit 0 ;;\n    \"do ci-rerun \"*) shift 2; exec /bin/sh \"$ZAF_ROOT/.zaf/actions/ci-rerun\" \"$@\" ;;\nesac\nexec '{real}' \"$@\"\n",
            log = log.display()
        );
        let claude = format!("#!/bin/sh\necho \"claude $*\" >> '{}'\n", log.display());
        write(&shim_dir, "zaf", &zaf);
        write(&shim_dir, "claude", &claude);
        for name in ["zaf", "claude"] {
            fs::set_permissions(shim_dir.join(name), fs::Permissions::from_mode(0o755))
                .expect("mode");
        }
        project.env("PATH", &path_with_zaf(&[&shim_dir, &fixtures()]));
        (project, dir, remote, log)
    }

    /// The calls the stand-ins saw, one per line.
    fn fix_calls(log: &Path) -> Vec<String> {
        fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn programmer_ran(calls: &[String]) -> bool {
        calls.iter().any(|call| call.starts_with("claude -p"))
    }

    #[test]
    fn fix_copies_are_identical_and_say_what_changed() {
        let root = repository();
        if !root.join(COPIES[1]).is_dir() {
            return;
        }
        let text = fs::read_to_string(root.join(COPIES[1]).join("fix")).expect("pack copy");
        let ways = fs::read_to_string(root.join(COPIES[0]).join("fix")).expect("ways' copy");
        assert_eq!(text, ways, "ways' copy is the pack's");
        let parsed = parse(&root.join(COPIES[1]).join("fix"), &text)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            parsed.metadata.description.contains("CI runs again first"),
            "{}",
            parsed.metadata.description
        );
        let after_block = text
            .lines()
            .find(|line| !line.starts_with("#!") && !line.starts_with("# zaf "))
            .expect("a body");
        assert!(after_block.starts_with("# changed:"), "{after_block}");
    }

    #[test]
    fn fix_reruns_ci_before_its_programmer() {
        for copy in COPIES {
            let (project, dir, remote, log) = fix_scene(copy, RED);
            let (before, _) = remote_branch(&remote);

            let (code, text) = run_script(&project, "fix", &["42"]);

            assert_eq!(code, 0, "{copy}: {text}");
            let calls = fix_calls(&log);
            assert!(!programmer_ran(&calls), "{copy}: {calls:?}");
            assert!(
                !calls.iter().any(|call| call.starts_with("zaf brief")),
                "{copy}: {calls:?}"
            );
            let (after, tip) = remote_branch(&remote);
            assert_eq!(after, before + 1, "{copy}: exactly one commit pushed");
            assert_eq!(tip, commit_field(&dir, "@-", "commit_id"), "{copy}");
            assert_eq!(commit_field(&dir, "@-", "empty"), "true", "{copy}");
        }
    }

    #[test]
    fn fix_at_an_empty_head_starts_its_programmer() {
        for copy in COPIES {
            let (mut project, dir, remote, log) = fix_scene(copy, RED);
            let (code, text) = run_script(&project, "ci-rerun", &["42"]);
            assert_eq!(code, 0, "{copy}: {text}");
            let pushed = remote_branch(&remote);
            forge_head(&mut project, &dir);

            // The head is now an empty commit: this tree has had its one fresh run.
            let (code, text) = run_script(&project, "fix", &["42"]);

            assert_eq!(code, 0, "{copy}: {text}");
            assert_eq!(remote_branch(&remote), pushed, "{copy}: nothing pushed");
            let calls = fix_calls(&log);
            assert!(programmer_ran(&calls), "{copy}: {calls:?}");
            // The programmer's round is published with `slice-pr`, and no `ci-rerun` follows it.
            let at = |prefix: &str| calls.iter().position(|call| call.starts_with(prefix));
            assert!(at("claude -p") < at("zaf do slice-pr"), "{copy}: {calls:?}");
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.starts_with("zaf do ci-rerun"))
                    .count(),
                1,
                "{copy}: {calls:?}"
            );
        }
    }

    #[test]
    fn fix_with_no_failed_check_starts_its_programmer() {
        for copy in COPIES {
            for checks in [GREEN, ""] {
                let (project, dir, remote, log) = fix_scene(copy, checks);
                let before = remote_branch(&remote);
                let work = commit_field(&dir, "@-", "commit_id");

                let (code, text) = run_script(&project, "fix", &["42"]);

                assert_eq!(code, 0, "{copy}: {text}");
                assert_eq!(remote_branch(&remote), before, "{copy}: nothing pushed");
                assert_eq!(commit_field(&dir, "@-", "commit_id"), work, "{copy}");
                let calls = fix_calls(&log);
                assert!(programmer_ran(&calls), "{copy}: {calls:?}");
            }
        }
    }

    #[test]
    fn fix_ends_with_2_when_ci_rerun_fails() {
        for copy in COPIES {
            let (mut project, _dir, remote, log) = fix_scene(copy, RED);
            // The forge is unreachable now: `ci-rerun` ends with 2, a backend that failed.
            project.fail_first("pr view *", "the forge is down");
            let before = remote_branch(&remote);

            let (code, text) = run_script(&project, "fix", &["42"]);

            assert_eq!(code, 2, "{copy}: {text}");
            assert_eq!(remote_branch(&remote), before, "{copy}: nothing pushed");
            let calls = fix_calls(&log);
            assert!(!programmer_ran(&calls), "{copy}: {calls:?}");
        }
    }

    const GIT_BASE: &str = "feature/1.0.0";

    const GIT_MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"

[vcs]
kind = "git"
"#;

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let mut full = vec![
            "-c",
            "user.name=Test Author",
            "-c",
            "user.email=author@example.com",
        ];
        full.extend_from_slice(args);
        run(dir, "git", &full)
    }

    /// A git project with `main` pushed to a bare `origin`, the fake forge naming `main`, and the
    /// pack's `base-open.git` in its actions folder.
    fn git_project() -> Project {
        let mut project = Project::new(GIT_MANIFEST);
        project
            .env("ZAF_VERSION", "1.0.0")
            .env("ZAF_BASE", GIT_BASE)
            .env("PATH", &path_with_zaf(&[&fixtures()]));
        project.on("api *", "main\n");
        let temp = project.temp.path().to_owned();
        let root = project.root.clone();
        run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
        git_in(&root, &["init", "-q", "-b", "main"]);
        let remote = temp.join("remote.git");
        git_in(
            &root,
            &["remote", "add", "origin", &remote.to_string_lossy()],
        );
        write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
        write(&root, "notes.txt", "one\n");
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/teams/actions/base-open.git");
        let text =
            fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let actions = root.join(".zaf/actions");
        write(&actions, "base-open.git", &text);
        fs::set_permissions(
            actions.join("base-open.git"),
            fs::Permissions::from_mode(0o750),
        )
        .expect("mode");
        git_in(&root, &["add", "-A"]);
        git_in(&root, &["commit", "-qm", "Start the project"]);
        git_in(&root, &["push", "-q", "origin", "main"]);
        // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
        let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        project
    }

    fn git_base_open(project: &Project) -> (i32, String, String) {
        outcome(&project.zaf(&["do", "--more", "base-open"]))
    }

    /// The commit a branch names in the bare remote, none when it holds no such branch.
    fn git_remote_ref(project: &Project, name: &str) -> Option<String> {
        let output = std::process::Command::new("git")
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

    /// What a checkout is: its `HEAD`, its branch, its branches, and its status.
    fn checkout_state(project: &Project) -> Vec<String> {
        [
            "rev-parse HEAD",
            "branch --show-current",
            "branch --list",
            "status --porcelain",
        ]
        .iter()
        .map(|args| git_in(&project.root, &args.split(' ').collect::<Vec<_>>()))
        .collect()
    }

    #[test]
    fn git_base_open_makes_the_branch_on_origin() {
        let project = git_project();
        let main = git_remote_ref(&project, "main").expect("origin holds main");
        assert_eq!(git_remote_ref(&project, GIT_BASE), None);

        let (code, stdout, stderr) = git_base_open(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(stdout.trim(), format!("made {GIT_BASE}"));
        assert_eq!(git_remote_ref(&project, GIT_BASE), Some(main));
    }

    #[test]
    fn git_base_open_leaves_the_checkout() {
        let project = git_project();
        write(&project.root, "wip.txt", "wip\n");
        write(&project.root, "notes.txt", "changed\n");
        let before = checkout_state(&project);

        let (code, stdout, stderr) = git_base_open(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(checkout_state(&project), before);
        assert_eq!(
            fs::read_to_string(project.root.join("notes.txt")).expect("notes"),
            "changed\n"
        );
    }

    #[test]
    fn git_base_open_is_quiet_when_open() {
        let project = git_project();
        let (code, stdout, stderr) = git_base_open(&project);
        assert_eq!(code, 0, "{stdout}{stderr}");
        let made = git_remote_ref(&project, GIT_BASE).expect("made");

        let (code, stdout, stderr) = git_base_open(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(stdout.trim(), "", "a branch handed back prints nothing");
        assert_eq!(git_remote_ref(&project, GIT_BASE), Some(made));
    }

    #[test]
    fn git_base_open_refuses_unrelated_history() {
        let project = git_project();
        let temp = project.temp.path();
        let other = temp.join("unrelated");
        run(temp, "git", &["init", "-q", "-b", "alone", "unrelated"]);
        write(&other, "alone.txt", "a\n");
        git_in(&other, &["add", "alone.txt"]);
        git_in(&other, &["commit", "-qm", "A commit that lacks main"]);
        let remote = temp.join("remote.git");
        git_in(
            &other,
            &[
                "push",
                "-q",
                &remote.to_string_lossy(),
                &format!("HEAD:refs/heads/{GIT_BASE}"),
            ],
        );
        let before = git_remote_ref(&project, GIT_BASE);

        let (code, stdout, stderr) = git_base_open(&project);

        assert_eq!(code, 1, "{stdout}{stderr}");
        assert!(
            format!("{stdout}{stderr}").contains(GIT_BASE),
            "the refusal names the branch: {stdout}{stderr}"
        );
        assert_eq!(git_remote_ref(&project, GIT_BASE), before);
    }

    // The git version of `papers-commit`: a git project with `base-open` having opened the
    // integration branch and a release folder of two files in the working copy.

    const PAPERS_FOLDER: &str = ".teams/releases/1.0.0";

    fn papers_project() -> Project {
        let mut project = git_project();
        project.env("ZAF_RELEASE_DIR", PAPERS_FOLDER);
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/teams/actions/papers-commit.git");
        let text =
            fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let actions = project.root.join(".zaf/actions");
        write(&actions, "papers-commit.git", &text);
        fs::set_permissions(
            actions.join("papers-commit.git"),
            fs::Permissions::from_mode(0o750),
        )
        .expect("mode");
        let (code, stdout, stderr) = git_base_open(&project);
        assert_eq!(code, 0, "{stdout}{stderr}");
        write(
            &project.root,
            &format!("{PAPERS_FOLDER}/scope.md"),
            "scope\n",
        );
        write(
            &project.root,
            &format!("{PAPERS_FOLDER}/mocks/design.md"),
            "design\n",
        );
        project
    }

    fn git_papers_commit(project: &Project) -> (i32, String, String) {
        outcome(&project.zaf(&["do", "--more", "papers-commit"]))
    }

    fn remote_git(project: &Project, args: &[&str]) -> String {
        run(&project.temp.path().join("remote.git"), "git", args)
            .trim()
            .to_owned()
    }

    #[test]
    fn git_papers_commit_commits_only_the_folder() {
        let project = papers_project();
        write(&project.root, "notes.txt", "edited\n");
        let base = git_remote_ref(&project, GIT_BASE).expect("the branch is open");

        let (code, stdout, stderr) = git_papers_commit(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        let made = git_remote_ref(&project, GIT_BASE).expect("the branch");
        assert_eq!(stdout.trim(), format!("committed {made}"));
        assert_eq!(
            remote_git(&project, &["rev-parse", &format!("{made}^")]),
            base
        );
        assert_eq!(
            remote_git(
                &project,
                &[
                    "diff-tree",
                    "-r",
                    "--no-commit-id",
                    "--name-status",
                    &base,
                    &made
                ]
            ),
            format!("A\t{PAPERS_FOLDER}/mocks/design.md\nA\t{PAPERS_FOLDER}/scope.md")
        );
        assert_eq!(
            remote_git(&project, &["show", &format!("{made}:notes.txt")]),
            "one"
        );
    }

    #[test]
    fn git_papers_commit_leaves_the_checkout_and_index() {
        let project = papers_project();
        write(&project.root, "notes.txt", "edited\n");
        git_in(&project.root, &["add", "notes.txt"]);
        write(&project.root, "wip.txt", "wip\n");
        let before = checkout_state(&project);
        let index = git_in(&project.root, &["ls-files", "--stage"]);

        let (code, stdout, stderr) = git_papers_commit(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(checkout_state(&project), before);
        assert_eq!(git_in(&project.root, &["ls-files", "--stage"]), index);
        assert_eq!(
            fs::read_to_string(project.root.join("notes.txt")).expect("notes"),
            "edited\n"
        );
    }

    #[test]
    fn git_papers_commit_is_quiet_when_current() {
        let project = papers_project();
        let (code, stdout, stderr) = git_papers_commit(&project);
        assert_eq!(code, 0, "{stdout}{stderr}");
        let made = git_remote_ref(&project, GIT_BASE).expect("made");

        let (code, stdout, stderr) = git_papers_commit(&project);

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(stdout.trim(), "", "nothing new prints nothing");
        assert_eq!(git_remote_ref(&project, GIT_BASE), Some(made));
    }

    #[test]
    fn git_papers_commit_refuses_without_the_base() {
        let project = papers_project();
        git_in(
            &project.root,
            &["push", "-q", "origin", &format!(":refs/heads/{GIT_BASE}")],
        );
        assert_eq!(git_remote_ref(&project, GIT_BASE), None);

        let (code, stdout, stderr) = git_papers_commit(&project);

        assert_eq!(code, 1, "{stdout}{stderr}");
        assert!(
            format!("{stdout}{stderr}").contains(GIT_BASE),
            "the refusal names the branch: {stdout}{stderr}"
        );
    }

    // The git versions of `push` and `slice-pr`, on a git project with a bare `origin` and the
    // fake forge, where task 2 of epic 1 is claimed and a pull request opens as number 7.

    const PR_BASE: &str = "feature/v1";
    const PR_SLICE: &str = "feature/v1-1-2";

    const CLAIMED: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[{"body":"claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}]}"#;

    fn pr_project() -> Project {
        let mut project = Project::new(GIT_MANIFEST);
        let temp = project.temp.path().to_owned();
        let root = project.root.clone();
        run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
        git_in(&root, &["init", "-q", "-b", "main"]);
        let remote = temp.join("remote.git");
        git_in(
            &root,
            &["remote", "add", "origin", &remote.to_string_lossy()],
        );
        write(&root, ".gitignore", ".zaf/*\n");
        write(&root, "notes.txt", "one\ntwo\n");
        git_in(&root, &["add", "-A"]);
        git_in(&root, &["commit", "-qm", "Start the project"]);
        git_in(&root, &["branch", PR_BASE]);
        git_in(&root, &["push", "-q", "origin", "main", PR_BASE]);

        project
            .on("issue list --label claimed *", "[{\"number\":2}]")
            .on("issue view 2 *", CLAIMED)
            .on(&format!("pr list --head {PR_SLICE} *"), "")
            .on(
                "pr create *",
                &format!("https://github.com/{REPO}/pull/7\n"),
            )
            .on("pr view 7 *", "abc\n")
            .env("ZAF_VERSION", "v1")
            // `commit` makes a commit: a runner has no global identity to do it under.
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "a@example.com")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "a@example.com")
            .env("PATH", &path_with_zaf(&[&fixtures()]));
        // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
        let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
        assert_eq!(code, 0, "{stdout}{stderr}");

        let ways = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let actions = root.join(".zaf/actions");
        for (name, from) in [
            ("workspace-open.git", "plugins/teams/actions"),
            ("commit.git", "plugins/teams/actions"),
            ("push.git", "plugins/teams/actions"),
            ("slice-pr.git", "plugins/teams/actions"),
            ("pr-open", "plugins/teams/actions"),
            ("slice-open", ".zaf/actions"),
        ] {
            let file = ways.join(from).join(name);
            let text = fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
            write(&actions, name, &text);
            fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750))
                .expect("mode");
        }
        project
    }

    fn pr_action(project: &Project, args: &[&str]) -> (i32, String) {
        let mut command = vec!["do", "--more"];
        command.extend_from_slice(args);
        let (code, stdout, stderr) = outcome(&project.zaf(&command));
        (code, format!("{stdout}{stderr}"))
    }

    fn pr_open_slice(project: &Project) -> PathBuf {
        let (code, text) = pr_action(project, &["slice-open", "2"]);
        assert_eq!(code, 0, "{text}");
        project
            .root
            .canonicalize()
            .expect("canonical root")
            .join(".zaf/workspaces")
            .join(PR_SLICE)
    }

    fn last_line(text: &str) -> &str {
        text.lines()
            .rfind(|line| !line.trim().is_empty())
            .expect("a line")
    }

    fn pr_creates(project: &Project) -> usize {
        project
            .gh_calls()
            .iter()
            .filter(|call| call.starts_with("pr create"))
            .count()
    }

    #[test]
    fn git_slice_pr_opens_a_pull_request() {
        let project = pr_project();
        let dir = pr_open_slice(&project);
        write(&dir, "writer.csv", "a,b\n");

        let (code, text) = pr_action(&project, &["slice-pr", "2"]);

        assert_eq!(code, 0, "{text}");
        assert_eq!(last_line(&text), "7", "{text}");
        // What was left is committed under the title, and the branch is on origin at that commit.
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%s"]),
            "Export the list (#2)"
        );
        let head = git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(git_remote_ref(&project, PR_SLICE), Some(head));
        let create = project
            .gh_calls()
            .into_iter()
            .find(|call| call.starts_with("pr create"))
            .expect("a pr create call");
        assert!(
            create.contains(&format!(
                "--head {PR_SLICE} --base {PR_BASE} --title Export the list (#2) --body Refs #2."
            )),
            "{create}"
        );
    }

    #[test]
    fn git_slice_pr_updates_the_same_pull_request() {
        let mut project = pr_project();
        let dir = pr_open_slice(&project);
        write(&dir, "writer.csv", "a,b\n");
        let (code, text) = pr_action(&project, &["slice-pr", "2"]);
        assert_eq!(code, 0, "{text}");
        let first = git(&dir, &["rev-parse", "HEAD"]);
        // From now on the forge has the pull request open for the branch.
        project.on_first(
            &format!("pr list --head {PR_SLICE} *"),
            &format!("7\thttps://github.com/{REPO}/pull/7\n"),
        );
        write(&dir, "reader.csv", "c,d\n");

        let (code, text) = pr_action(&project, &["slice-pr", "2"]);

        assert_eq!(code, 0, "{text}");
        assert_eq!(last_line(&text), "7", "{text}");
        let head = git(&dir, &["rev-parse", "HEAD"]);
        assert_ne!(head, first);
        assert_eq!(git_remote_ref(&project, PR_SLICE), Some(head));
        // The branch moved forward from the first commit, and no second request was made.
        assert_eq!(git(&dir, &["rev-parse", "HEAD~1"]), first);
        assert_eq!(pr_creates(&project), 1);
    }

    #[test]
    fn git_push_refuses_a_non_forward_move() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        fs::write(dir.join("notes.txt"), "one\nmine\n").expect("edit");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Mine"]);
        assert_eq!(code, 0, "{text}");

        // Another clone puts work on origin's branch that the slice does not hold.
        let other = temp.path().join("other");
        git(
            temp.path(),
            &[
                "clone",
                "-q",
                &temp.path().join("origin.git").to_string_lossy(),
                "other",
            ],
        );
        git(&other, &["checkout", "-q", "-b", GIT_SLICE]);
        fs::write(other.join("notes.txt"), "one\ntheirs\n").expect("edit");
        git(&other, &["commit", "-q", "-am", "Theirs"]);
        git(&other, &["push", "-q", "origin", GIT_SLICE]);
        let theirs = git(&other, &["rev-parse", "HEAD"]);

        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("origin has work"), "{text}");
        let origin = temp.path().join("origin.git");
        assert_eq!(
            git(&origin, &["rev-parse", &format!("refs/heads/{GIT_SLICE}")]),
            theirs
        );

        // Origin's branch ahead of the slice's is a backward move: refused as well.
        git(&dir, &["fetch", "-q", "origin", GIT_SLICE]);
        git(&dir, &["reset", "-q", "--hard", "main"]);
        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 1, "{text}");
        assert_eq!(
            git(&origin, &["rev-parse", &format!("refs/heads/{GIT_SLICE}")]),
            theirs
        );
    }

    #[test]
    fn git_push_moves_forward_and_is_quiet_when_there() {
        let (temp, root) = git_slice_project();
        let dir = open_git_slice(&root);
        let origin = temp.path().join("origin.git");
        let branch = format!("refs/heads/{GIT_SLICE}");
        fs::write(dir.join("notes.txt"), "one\ntwo\n").expect("edit");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "One"]);
        assert_eq!(code, 0, "{text}");
        let first = git(&dir, &["rev-parse", "HEAD"]);

        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(text.lines().last(), Some(first.as_str()), "{text}");
        assert_eq!(git(&origin, &["rev-parse", &branch]), first);

        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(text.trim(), first);

        fs::write(dir.join("notes.txt"), "one\ntwo\nthree\n").expect("edit");
        let (code, text) = run_git_action(&root, "commit", "main", "git", &[GIT_SLICE, "Two"]);
        assert_eq!(code, 0, "{text}");
        let second = git(&dir, &["rev-parse", "HEAD"]);
        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(git(&origin, &["rev-parse", &branch]), second);

        // Uncommitted work is refused, and nothing moves.
        fs::write(dir.join("notes.txt"), "more\n").expect("edit");
        let (code, text) = run_git_action(&root, "push", "main", "git", &[GIT_SLICE]);
        assert_eq!(code, 1, "{text}");
        assert!(text.contains("uncommitted work"), "{text}");
    }

    #[test]
    fn git_slice_pr_refuses_unmerged_paths() {
        let project = pr_project();
        let dir = pr_open_slice(&project);
        write(&dir, "notes.txt", "one\nmine\n");
        git_in(&dir, &["commit", "-qam", "Mine"]);
        // The base moves on the same line, and the merge of it stops with a conflict.
        let temp = project.temp.path();
        let other = temp.join("other");
        git_in(
            temp,
            &[
                "clone",
                "-q",
                &temp.join("remote.git").to_string_lossy(),
                "other",
            ],
        );
        git_in(&other, &["checkout", "-q", PR_BASE]);
        write(&other, "notes.txt", "one\ntheirs\n");
        git_in(&other, &["commit", "-qam", "Theirs"]);
        git_in(&other, &["push", "-q", "origin", PR_BASE]);
        git_in(&dir, &["fetch", "-q", "origin"]);
        let merge = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test Author",
                "-c",
                "user.email=a@example.com",
            ])
            .args(["merge", "--no-edit", &format!("origin/{PR_BASE}")])
            .current_dir(&dir)
            .output()
            .expect("git merge");
        assert!(!merge.status.success(), "the merge conflicts");

        let (code, text) = pr_action(&project, &["slice-pr", "2"]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        // Nothing was pushed, committed or opened, and the merge is still open.
        assert_eq!(git_remote_ref(&project, PR_SLICE), None);
        assert_eq!(pr_creates(&project), 0);
        assert!(!text.contains("zaf do push"), "{text}");
        git_in(&dir, &["rev-parse", "--verify", "-q", "MERGE_HEAD"]);
    }

    #[test]
    fn slice_pr_git_refuses_markers_in_head() {
        let project = pr_project();
        let dir = pr_open_slice(&project);
        write(&dir, "notes.txt", "one\nmine\n");
        git_in(&dir, &["commit", "-qam", "Mine"]);
        let temp = project.temp.path();
        let other = temp.join("other");
        git_in(
            temp,
            &[
                "clone",
                "-q",
                &temp.join("remote.git").to_string_lossy(),
                "other",
            ],
        );
        git_in(&other, &["checkout", "-q", PR_BASE]);
        write(&other, "notes.txt", "one\ntheirs\n");
        git_in(&other, &["commit", "-qam", "Theirs"]);
        git_in(&other, &["push", "-q", "origin", PR_BASE]);
        git_in(&dir, &["fetch", "-q", "origin"]);
        let merge = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test Author",
                "-c",
                "user.email=a@example.com",
            ])
            .args(["merge", "--no-edit", &format!("origin/{PR_BASE}")])
            .current_dir(&dir)
            .output()
            .expect("git merge");
        assert!(!merge.status.success(), "the merge conflicts");
        // The merge is committed over its markers, by hand.
        git_in(&dir, &["add", "-A"]);
        git_in(&dir, &["commit", "-qm", "Merge the base"]);
        git_in(&dir, &["rev-parse", "--verify", "HEAD^2"]);

        let (code, text) = pr_action(&project, &["slice-pr", "2"]);

        assert_eq!(code, 1, "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        assert_eq!(git_remote_ref(&project, PR_SLICE), None);
        assert_eq!(pr_creates(&project), 0);

        // Resolved and committed again, the slice publishes.
        write(&dir, "notes.txt", "one\nboth\n");
        git_in(&dir, &["commit", "-qam", "Resolve"]);
        let (code, text) = pr_action(&project, &["slice-pr", "2"]);
        assert_eq!(code, 0, "{text}");
    }

    // `implement` takes the workspace an earlier run opened, whatever the project's VCS (design
    // addendum D28). It runs as a script with stand-ins for `zaf` and `claude` that log each call.

    const IMPLEMENT_SLICE: &str = "feature/1-2-42";

    /// Runs the pack's `implement 42` with `vcs` as `ZAF_VCS` when given, and gives the exit code,
    /// the calls the stand-ins saw and what it printed.
    fn run_implement(copy: &str, root: &Path, vcs: Option<&str>) -> (i32, Vec<String>, String) {
        let shims = tempfile::tempdir().expect("temp dir");
        let log = shims.path().join("calls.log");
        let zaf = format!(
            "#!/bin/sh\necho \"zaf $*\" >> '{log}'\ncase \"$*\" in\n    \"brief \"*) printf '%s\\n%s\\n' /nonexistent/pack 1 ;;\n    \"do --more slice-open\"*) echo opened-another ;;\nesac\nexit 0\n",
            log = log.display()
        );
        let claude = format!("#!/bin/sh\necho \"claude $*\" >> '{}'\n", log.display());
        write(shims.path(), "zaf", &zaf);
        write(shims.path(), "claude", &claude);
        for name in ["zaf", "claude"] {
            fs::set_permissions(shims.path().join(name), fs::Permissions::from_mode(0o755))
                .expect("mode");
        }
        let script = repository().join(copy).join("implement");
        let path = format!(
            "{}:{}",
            shims.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg(&script)
            .arg("42")
            .current_dir(root)
            .env("PATH", path)
            .env("ZAF_ROOT", root)
            .env("ZAF_SLICE", IMPLEMENT_SLICE)
            .env_remove("ZAF_VCS");
        if let Some(vcs) = vcs {
            command.env("ZAF_VCS", vcs);
        }
        let output = command.output().expect("spawn sh");
        let calls = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        (
            output.status.code().expect("an exit code"),
            calls,
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )
    }

    fn opened_a_slice(calls: &[String]) -> bool {
        calls.iter().any(|call| call.contains("slice-open"))
    }

    #[test]
    fn implement_takes_an_open_git_worktree() {
        for copy in COPIES {
            let (_temp, root) = git_slice_project();
            let dir = open_git_slice_named(&root, IMPLEMENT_SLICE);
            assert!(dir.join(".git").is_file(), "a worktree's .git is a file");

            let (code, calls, text) = run_implement(copy, &root, Some("git"));

            assert_eq!(code, 0, "{copy}: {text}");
            assert!(!opened_a_slice(&calls), "{copy}: {calls:?}");
            let claude = calls
                .iter()
                .find(|call| call.starts_with("claude -p"))
                .unwrap_or_else(|| panic!("{copy}: the programmer ran: {calls:?}"));
            assert!(
                claude.contains(&format!("Workspace: {}.", dir.display())),
                "{copy}: {claude}"
            );
        }
    }

    #[test]
    fn implement_opens_a_git_slice_when_none_is_open() {
        let (_temp, root) = git_slice_project();

        let (code, calls, text) = run_implement(COPIES[1], &root, Some("git"));

        assert_eq!(code, 0, "{text}");
        assert!(opened_a_slice(&calls), "{calls:?}");
    }

    #[test]
    fn implement_jj_workspace_unchanged() {
        for copy in COPIES {
            for vcs in [None, Some("jj")] {
                let (_temp, root) = git_slice_project();
                let dir = root.join(".zaf/workspaces").join(IMPLEMENT_SLICE);
                fs::create_dir_all(dir.join(".jj")).expect("a jj workspace");

                let (code, calls, text) = run_implement(copy, &root, vcs);

                assert_eq!(code, 0, "{copy} {vcs:?}: {text}");
                assert!(!opened_a_slice(&calls), "{copy} {vcs:?}: {calls:?}");
                assert!(
                    calls
                        .iter()
                        .any(|call| call.contains(&format!("Workspace: {}.", dir.display()))),
                    "{copy} {vcs:?}: {calls:?}"
                );

                // With no workspace a jj project opens one, as before.
                fs::remove_dir_all(&dir).expect("remove");
                let (code, calls, text) = run_implement(copy, &root, vcs);
                assert_eq!(code, 0, "{copy} {vcs:?}: {text}");
                assert!(opened_a_slice(&calls), "{copy} {vcs:?}: {calls:?}");
            }
        }
    }

    /// Neither action calls jj or carries the mark #490 gave a jj action.
    #[test]
    fn implement_and_slice_merge_hold_no_jj_mark_and_run_no_jj() {
        for copy in COPIES {
            for name in ["implement", "slice-merge"] {
                let file = repository().join(copy).join(name);
                let text = fs::read_to_string(&file).expect("action");
                assert!(!text.contains("# zaf vcs:"), "{copy}/{name}: a vcs mark");
                for line in text
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('#'))
                {
                    assert!(
                        !line.split_whitespace().any(|word| word == "jj"),
                        "{copy}/{name}: {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn implement_copies_are_identical_and_say_what_changed() {
        let root = repository();
        let pack = fs::read_to_string(root.join(COPIES[1]).join("implement")).expect("pack");
        let ways = fs::read_to_string(root.join(COPIES[0]).join("implement")).expect("ways");
        assert_eq!(pack, ways, "ways' copy is the pack's");
        assert!(why_line_of(&pack), "no `# changed:` line after the block");
    }

    fn why_line_of(text: &str) -> bool {
        text.lines()
            .find(|line| !line.starts_with("#!") && !line.starts_with("# zaf "))
            .is_some_and(|line| line.starts_with("# changed:"))
    }

    fn open_git_slice_named(root: &Path, slice: &str) -> PathBuf {
        let (code, text) = run_git_action(root, "workspace-open", "main", "git", &[slice, "main"]);
        assert_eq!(code, 0, "{text}");
        root.join(".zaf/workspaces").join(slice)
    }

    // In a git project `slice-merge` merges at `ZAF_HEAD` and drops the worktree with the git
    // version of `workspace-drop`, which the built zaf resolves for that name.

    #[test]
    fn git_slice_merge_merges_at_zaf_head() {
        let mut scene = scene_in(git_project());
        scene.forge_head(LAND);
        let root = scene.project.root.clone();
        let dir = root.join(".zaf/workspaces/feature/1-2-42");
        git_in(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/1-2-42",
                &dir.to_string_lossy(),
                "main",
            ],
        );
        let drop =
            fs::read_to_string(repository().join("plugins/teams/actions/workspace-drop.git"))
                .expect("workspace-drop.git");
        write(&root, ".zaf/actions/workspace-drop.git", &drop);
        fs::set_permissions(
            root.join(".zaf/actions/workspace-drop.git"),
            fs::Permissions::from_mode(0o750),
        )
        .expect("mode");
        // The stand-in answers `workspace-drop` too, so the real one runs: take that rule out.
        let shim = scene.project.temp.path().join("shim/zaf");
        let text = fs::read_to_string(&shim).expect("shim");
        write(
            &scene.project.temp.path().join("shim"),
            "zaf",
            &text.replace(" | \"do workspace-drop\"*", ""),
        );
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("mode");

        let (code, stdout, stderr) = outcome(&scene.run(Some(LAND)));

        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(
            scene.project.gh_calls().last(),
            Some(&Scene::merge_call(LAND))
        );
        assert!(
            scene
                .calls()
                .iter()
                .any(|call| call.starts_with("board done 42")),
            "{:?}",
            scene.calls()
        );
        assert!(!dir.exists(), "the worktree is removed");
        assert!(
            !git_in(&root, &["worktree", "list", "--porcelain"]).contains("1-2-42"),
            "the worktree is pruned"
        );
        // The branch stays, as a drop leaves it.
        git_in(
            &root,
            &["rev-parse", "--verify", "refs/heads/feature/1-2-42"],
        );
    }
}

// `slice-merge` merges only at `ZAF_HEAD` (design-addendum D18, F20): the head it names is the one
// Land was derived at, so the forge, which pins the merge to it, refuses a head pushed after. It
// runs as a script with a stand-in `zaf` that answers the steps the merge does not test (the base
// probe, the board, the workspace) and passes `pr-merge` on to the built zaf over the fake `gh`,
// which plays the forge.

/// Land was derived at this head.
const LAND: &str = "0123456789abcdef0123456789abcdef01234567";
/// The head the pull request holds after a push that came later.
const PUSHED: &str = "89abcdef0123456789abcdef0123456789abcdef";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Scene {
    project: Project,
    log: PathBuf,
}

/// A project with ways' `pr-merge`, and a `zaf` first on `PATH` that records each call and answers
/// all but `pr-merge` itself, which the built zaf runs.
fn scene() -> Scene {
    scene_in(Project::new(MANIFEST))
}

/// The scene over a project of the caller's making, a git one for instance.
fn scene_in(mut project: Project) -> Scene {
    let shim_dir = project.temp.path().join("shim");
    let log = project.temp.path().join("shim.log");
    let real = env!("CARGO_BIN_EXE_zaf");
    let shim = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n    \"do --more merge-base\"* | \"board done\"* | \"do workspace-drop\"*) exit 0 ;;\nesac\nexec '{real}' \"$@\"\n",
        log.display()
    );
    write(&shim_dir, "zaf", &shim);
    fs::set_permissions(shim_dir.join("zaf"), fs::Permissions::from_mode(0o755)).expect("mode");
    project.env("PATH", &path_with_zaf(&[&shim_dir, &fixtures()]));
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let text = fs::read_to_string(repository().join(".zaf/actions/pr-merge")).expect("action");
    write(&project.root, ".zaf/actions/pr-merge", &text);
    fs::set_permissions(
        project.root.join(".zaf/actions/pr-merge"),
        fs::Permissions::from_mode(0o750),
    )
    .expect("mode");
    Scene { project, log }
}

impl Scene {
    /// The forge holds `head` as the pull request's head: it merges a pinned head only when it is
    /// that one.
    fn forge_head(&mut self, head: &str) {
        let pinned = format!("pr merge 7 --merge --match-head-commit {head} --repo {REPO}");
        self.project
            .on("pr view *", "OPEN\n")
            .on(&pinned, "")
            .fail_on(
                "pr merge *",
                "Head branch was modified. Review and try again.",
            );
    }

    /// `slice-merge 42` as the pack ships it, with `zaf_head` as `ZAF_HEAD` when given.
    fn run(&self, zaf_head: Option<&str>) -> Output {
        let script = repository().join("plugins/teams/actions/slice-merge");
        let mut command = self.project.command_of(
            Path::new("/bin/sh"),
            &self.project.root,
            &[&script.to_string_lossy(), "42"],
        );
        command
            .env("ZAF_ROOT", &self.project.root)
            .env("ZAF_SLICE", "feature/1-2-42")
            .env("ZAF_BASE", "feature/1")
            .env("ZAF_PR", "7")
            .env_remove("ZAF_HEAD");
        if let Some(head) = zaf_head {
            command.env("ZAF_HEAD", head);
        }
        command.output().expect("run slice-merge")
    }

    /// The calls the stand-in `zaf` saw, one per line.
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn merge_call(head: &str) -> String {
        format!("pr merge 7 --merge --match-head-commit {head} --repo {REPO}")
    }
}

#[test]
fn slice_merge_merges_at_zaf_head() {
    let mut scene = scene();
    scene.forge_head(LAND);

    let (code, stdout, stderr) = outcome(&scene.run(Some(LAND)));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        scene.project.gh_calls().last(),
        Some(&Scene::merge_call(LAND))
    );
    let calls = scene.calls();
    assert!(
        calls.iter().any(|call| call.starts_with("board done 42")),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("do workspace-drop")),
        "{calls:?}"
    );
}

#[test]
fn slice_merge_refuses_a_head_pushed_after_land() {
    let mut scene = scene();
    // Land was derived at LAND; the pull request has moved on to PUSHED.
    scene.forge_head(PUSHED);

    let (code, stdout, stderr) = outcome(&scene.run(Some(LAND)));

    assert_eq!(code, 1, "{stdout}{stderr}");
    // The merge named ZAF_HEAD, not the pushed head, and nothing after it ran.
    assert_eq!(
        scene.project.gh_calls().last(),
        Some(&Scene::merge_call(LAND))
    );
    let calls = scene.calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("board done")),
        "{calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.starts_with("do workspace-drop")),
        "{calls:?}"
    );
}

#[test]
fn slice_merge_refuses_without_zaf_head() {
    let mut scene = scene();
    scene.forge_head(PUSHED);

    let (code, stdout, stderr) = outcome(&scene.run(None));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("ZAF_HEAD"), "{stderr}");
    // Nothing ran: no probe, no merge, no board, no workspace.
    assert_eq!(scene.calls(), Vec::<String>::new());
    assert_eq!(scene.project.gh_calls(), Vec::<String>::new());
}

/// Both copies are one file, which says what changed right after its `# zaf` block, and its `1`
/// line names `ZAF_HEAD`.
#[test]
fn slice_merge_copies_are_identical_and_say_what_changed() {
    let root = repository();
    let pack = root.join("plugins/teams/actions/slice-merge");
    if !pack.is_file() {
        return;
    }
    let text = fs::read_to_string(&pack).expect("the pack's copy");
    let ways = fs::read_to_string(root.join(".zaf/actions/slice-merge")).expect("ways' copy");
    assert_eq!(text, ways, "ways' copy is the pack's");
    let parsed = parse(&pack, &text).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        parsed.metadata.codes[&1].contains("ZAF_HEAD"),
        "{:?}",
        parsed.metadata.codes
    );
    let after_block = text
        .lines()
        .find(|line| !line.starts_with("#!") && !line.starts_with("# zaf "))
        .expect("a body");
    assert!(
        after_block.starts_with("# changed:"),
        "no `# changed:` line right after the `# zaf` block"
    );
}

/// The `# changed:` line right after an action's `# zaf` block, the one `zaf update` reads as the why.
fn why_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| !line.starts_with("#!") && !line.starts_with("# zaf "))
        .filter(|line| line.starts_with("# changed:"))
}

/// Whether an action says why it differs from its tagged copy: it equals that copy, or its why line
/// is one the tagged copy lacked.
fn says_why(tagged: &str, current: &str) -> bool {
    tagged == current
        || why_line(current).is_some_and(|why| !tagged.lines().any(|line| line == why))
}

#[test]
fn says_why_wants_a_new_changed_line_of_a_changed_action() {
    let tagged = "#!/bin/sh\n# zaf 1: x\n# changed: old\nbody\n";
    assert!(says_why(tagged, tagged), "equal needs none");
    let new = "#!/bin/sh\n# zaf 1: x\n# changed: new\nbody2\n";
    assert!(says_why(tagged, new));
    let stale = "#!/bin/sh\n# zaf 1: x\n# changed: old\nbody2\n";
    assert!(!says_why(tagged, stale), "an old line says nothing new");
    let none = "#!/bin/sh\n# zaf 1: x\nbody2\n";
    assert!(!says_why(tagged, none));
    let late = "#!/bin/sh\n# zaf 1: x\nbody2\n# changed: late\n";
    assert!(
        !says_why(tagged, late),
        "zaf update reads only the line after the block"
    );
}

/// Every pack action that differs from its copy at the last release tag carries a `# changed:` line
/// that copy lacked (`zaf update` shows it as the why); an action new since the tag needs none.
/// Release tags are `zaf-vX.Y.Z`; the nearest one reachable from HEAD is the last release. With no
/// tag reachable the test fails: a checkout without tags would otherwise pass it unchecked.
#[test]
fn every_changed_action_says_why() {
    let root = repository();
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("run git")
    };
    let described = git(&["describe", "--tags", "--abbrev=0", "--match", "*v[0-9]*"]);
    assert!(
        described.status.success(),
        "no release tag (v*) is reachable from HEAD: the tags are missing, fetch them \
         (git fetch --tags); {}",
        String::from_utf8_lossy(&described.stderr)
    );
    let tag = String::from_utf8_lossy(&described.stdout).trim().to_owned();

    let mut unexplained = Vec::new();
    for entry in fs::read_dir(&pack).expect("the pack's actions folder reads") {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        let shown = git(&["show", &format!("{tag}:plugins/teams/actions/{name}")]);
        if !shown.status.success() {
            continue; // new since the tag
        }
        let tagged = String::from_utf8_lossy(&shown.stdout).into_owned();
        let current =
            fs::read_to_string(pack.join(&name)).unwrap_or_else(|error| panic!("{name}: {error}"));
        if !says_why(&tagged, &current) {
            unexplained.push(name);
        }
    }
    unexplained.sort();
    assert!(
        unexplained.is_empty(),
        "changed since {tag} with no new `# changed:` line right after the `# zaf` block: \
         {unexplained:?}"
    );
}
