//! The ways repository's `slice-open`, `slice-pr` and `slice-drop` actions (docs/zaf-actions.md
//! §3, §7; rulings 26, 27 and 39): run through `zaf do` on a throwaway jj repository with a bare
//! remote and a fake `gh` that knows task 2 of epic 1, each acts on the task's slice by the
//! values zaf hands it, hands back what zaf keeps as the task's record, and runs only in the
//! phases it names.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";

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

/// The actions the three are built from, and the three.
const ACTIONS: [&str; 8] = [
    "workspace-open",
    "workspace-drop",
    "commit",
    "push",
    "pr-open",
    "slice-open",
    "slice-pr",
    "slice-drop",
];

/// Task 2 of epic 1, claimed by a team instance, as `gh issue view` prints it.
const CLAIMED: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[{"body":"claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}]}"#;

/// Task 2 of epic 1, not yet claimed.
const READY: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote, the actions in
/// its actions folder, the built `zaf` on `PATH` so they can call it, and a fake `gh` where task 2
/// holds `ticket`.
fn project_with(ticket: &str) -> Project {
    let mut project = Project::new(MANIFEST);
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
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

    // The board and the forge: the claim is the label and a comment, a pull request is number 7.
    project
        .on("issue list --label claimed *", "[{\"number\":2}]")
        .on("issue view 2 *", ticket)
        .on(&format!("pr list --head {SLICE} *"), "")
        .on(
            "pr create *",
            &format!("https://github.com/{REPO}/pull/7\n"),
        )
        .on("pr view 7 *", r#"{"headRefOid":"abc"}"#)
        .env("ZAF_VERSION", "v1");

    // The built `zaf` first on PATH, then the fake gh, so a body can say `zaf do`.
    let bin = Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder")
        .to_owned();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [bin, fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    project.env("PATH", &search_path.to_string_lossy());

    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    for name in ACTIONS {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
    }
    project
}

/// A project whose task 2 is claimed: it stands in `claim`.
fn project() -> Project {
    project_with(CLAIMED)
}

/// `zaf do --more <args>` with its whole log: the exit code and the lines.
fn action(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// The slice's workspace, where `workspace-open` puts it.
fn workspace(project: &Project) -> PathBuf {
    project
        .root
        .canonicalize()
        .expect("canonical root")
        .join(".zaf/workspaces")
        .join(SLICE)
}

/// What zaf kept of what an action handed back: the record `name` under `.zaf/slices`.
fn record(project: &Project, name: &str) -> Option<String> {
    fs::read_to_string(project.root.join(".zaf/slices").join(name)).ok()
}

/// Opens the slice and returns its workspace.
fn open(project: &Project) -> PathBuf {
    let (code, text) = action(project, &["slice-open", "2"]);
    assert_eq!(code, 0, "{text}");
    workspace(project)
}

/// The commit a revset names in the root repository.
fn commit_at(root: &Path, revset: &str) -> String {
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

fn bookmark_at(root: &Path, name: &str) -> String {
    commit_at(root, &format!("bookmarks(exact:\"{name}\")"))
}

/// A field of a commit in a workspace.
fn commit_field(dir: &Path, revset: &str, template: &str) -> String {
    jj(dir, &["log", "--no-graph", "-r", revset, "-T", template])
}

/// Stores a task check's record as `zaf check` would: the last one, with `verdict`.
fn record_check(project: &Project, verdict: &str) {
    write(
        &project.root,
        ".zaf/runs/check-task-2.json",
        &format!(r#"{{"target":"task #2","verdict":"{verdict}","lines":[]}}"#),
    );
}

#[test]
fn slices_have_valid_metadata() {
    for name in ["slice-open", "slice-pr", "slice-drop"] {
        let text = action_text(name);
        // The header zaf reads: a description, a team and release group, a task target.
        for line in [
            "# zaf mode: 750",
            "# zaf group: team, release",
            "# zaf target: task",
        ] {
            assert!(text.lines().any(|found| found == line), "{name}: {line}");
        }
        assert!(
            text.lines()
                .any(|line| line.starts_with("# zaf description: ")),
            "{name} has no description"
        );
    }
    // The phases each runs in: open before the slice exists; the other two from `build`, which a
    // failed task check keeps (ruling 26), and drop also once done.
    for (name, phases) in [
        ("slice-open", "# zaf phases: claim"),
        ("slice-pr", "# zaf phases: build, check, review"),
        ("slice-drop", "# zaf phases: build, check, done"),
    ] {
        let text = action_text(name);
        assert!(text.lines().any(|line| line == phases), "{name}: {phases}");
    }
}

#[test]
fn slice_open_hands_back_the_path() {
    let project = project();

    let (code, text) = action(&project, &["slice-open", "2"]);

    assert_eq!(code, 0, "{text}");
    // The last line is the workspace, an absolute path of the directory that now exists.
    let path = handed_back(&text);
    let dir = workspace(&project);
    assert_eq!(
        Path::new(path).canonicalize().expect("the path exists"),
        dir,
        "{text}"
    );
    assert!(
        dir.join("notes.txt").is_file(),
        "the workspace holds the base"
    );
    // zaf kept it as the task's workspace record, and the slice's bookmark sits on the base.
    assert_eq!(record(&project, "2").as_deref(), Some(path), "{text}");
    assert_eq!(
        bookmark_at(&project.root, SLICE),
        bookmark_at(&project.root, BASE)
    );
}

#[test]
fn slice_open_past_claim_refuses() {
    let project = project();
    let dir = open(&project);
    let opened = record(&project, "2");

    // The slice is open: the task is in `build`, past `claim`.
    let (code, text) = action(&project, &["slice-open", "2"]);

    assert_eq!(code, 125, "{text}");
    assert!(text.contains("NOT_RUN slice-open: "), "{text}");
    assert!(text.contains("2 is in build, not claim"), "{text}");
    assert_eq!(record(&project, "2"), opened);
    assert!(dir.is_dir());

    // A task nobody has claimed is before `claim`, and no slice is made for it.
    let unclaimed = project_with(READY);
    let (code, text) = action(&unclaimed, &["slice-open", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(text.contains("2 is in unclaimed, not claim"), "{text}");
    assert!(!workspace(&unclaimed).exists());
    assert_eq!(record(&unclaimed, "2"), None);
}

#[test]
fn slice_pr_skips_an_empty_commit() {
    let project = project();
    let dir = open(&project);
    // The work is already committed: the working copy holds nothing more.
    write(&dir, "writer.csv", "a,b\n");
    jj(&dir, &["commit", "-m", "Add the CSV writer"]);
    let committed = commit_field(&dir, "@-", "commit_id");

    let (code, text) = action(&project, &["slice-pr", "2"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    // No commit step ran and none was made: the work is as it was, pushed under the bookmark.
    assert!(!text.contains("zaf do commit"), "{text}");
    assert!(text.contains("zaf do push"), "{text}");
    assert!(text.contains("zaf do pr-open"), "{text}");
    assert_eq!(commit_field(&dir, "@-", "commit_id"), committed);
    assert_eq!(
        commit_field(&dir, "@-", "description").trim(),
        "Add the CSV writer"
    );
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
    assert_eq!(record(&project, "2.pr").as_deref(), Some("7"));
}

#[test]
fn slice_pr_titles_the_request() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");

    let (code, text) = action(&project, &["slice-pr", "2"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    // What was left is committed under the title, which is the task's with its number.
    assert!(text.contains("zaf do commit"), "{text}");
    assert_eq!(
        commit_field(&dir, "@-", "description").trim(),
        "Export the list (#2)"
    );
    assert_eq!(
        commit_field(&dir, "@", "empty"),
        "true",
        "the work is all committed"
    );
    assert_eq!(
        bookmark_at(&project.root, SLICE),
        commit_field(&dir, "@-", "commit_id")
    );
    // The request names the slice, the base, the title and the task it refers to.
    let create = project
        .gh_calls()
        .into_iter()
        .find(|call| call.starts_with("pr create"))
        .expect("a pr create call");
    assert!(
        create.contains(&format!(
            "--head {SLICE} --base {BASE} --title Export the list (#2) --body Refs #2."
        )),
        "{create}"
    );
    assert_eq!(record(&project, "2.pr").as_deref(), Some("7"));
}

#[test]
fn slice_drop_keeps_the_branch() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["commit", SLICE, "Add the CSV writer"]);
    assert_eq!(code, 0, "{text}");
    let committed = bookmark_at(&project.root, SLICE);
    assert_ne!(committed, bookmark_at(&project.root, BASE));

    let (code, text) = action(&project, &["slice-drop", "2"]);

    assert_eq!(code, 0, "{text}");
    // The workspace and the task's records go; the branch with its work stays.
    assert!(!dir.exists(), "the workspace is still there");
    assert_eq!(record(&project, "2"), None);
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
    // Dropped is back in `claim`: another drop has no slice to act on.
    let (code, text) = action(&project, &["slice-drop", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(text.contains("2 is in claim"), "{text}");
}

#[test]
fn drop_in_check_then_reopen() {
    let project = project();
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["commit", SLICE, "Add the CSV writer"]);
    assert_eq!(code, 0, "{text}");
    let committed = bookmark_at(&project.root, SLICE);
    // The task's checks have passed: it is in `check`, where a drop is allowed (ruling 27).
    record_check(&project, "clean");

    let (code, text) = action(&project, &["slice-drop", "2"]);

    assert_eq!(code, 0, "{text}");
    // Everything zaf kept of the slice is forgotten, so the task is back in `claim`.
    assert!(!dir.exists());
    assert_eq!(record(&project, "2"), None);
    assert!(!project.root.join(".zaf/runs/check-task-2.json").exists());

    // And it opens again: a fresh workspace on the branch, with the work the drop kept.
    let (code, text) = action(&project, &["slice-open", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(dir.is_dir(), "{text}");
    assert_eq!(
        Path::new(handed_back(&text))
            .canonicalize()
            .expect("the path exists"),
        dir
    );
    assert!(record(&project, "2").is_some());
    assert_eq!(
        fs::read_to_string(dir.join("writer.csv")).expect("the kept work"),
        "a,b\n"
    );
    assert_eq!(bookmark_at(&project.root, SLICE), committed);
}

#[test]
fn slice_pr_runs_from_build() {
    // Before the slice is open the task is in `claim`: nothing to publish.
    let project = project();
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 125, "{text}");
    assert!(
        text.contains("2 is in claim, not build, check, review"),
        "{text}"
    );
    assert!(
        project
            .gh_calls()
            .iter()
            .all(|call| !call.starts_with("pr "))
    );

    // From `build` it publishes, though no check has run: CI is the only gate (ruling 39).
    let dir = open(&project);
    write(&dir, "writer.csv", "a,b\n");
    let (code, text) = action(&project, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");

    // A failed task check keeps the task in `build` (ruling 26), and it publishes then too.
    let failed = project_with(CLAIMED);
    let dir = open(&failed);
    write(&dir, "writer.csv", "a,b\n");
    record_check(&failed, "must-fix");
    let (code, text) = action(&failed, &["slice-pr", "2"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    assert_eq!(record(&failed, "2.pr").as_deref(), Some("7"));
}
