//! The ways repository's `ship` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with a bare remote and a fake `gh`, `ship <bookmark> <base> <title>
//! [<body>]` takes the root checkout's change (`@`, or `@-` under an empty `@`), describes it with
//! the title when it has no description, pushes it under the bookmark, then runs `pr-open`,
//! `ci-wait`, a squash `pr-merge` pinned to the head CI passed and `sync <base>`, and hands back
//! the pull request number. Nothing to ship exits 1 before any gh call; a failed check or a
//! refused merge exits 1 and checks still running exit 3, each with the number last and no sync.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "main";
const BOOKMARK: &str = "feature/ship-it";
const TITLE: &str = "Ship the feature";

/// The head commit the fake forge reports for pull request 7.
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
/// The link of the one check, its job 2.
const LINK: &str = "https://github.com/example/tracker/actions/runs/1/job/2";

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

/// The actions `ship` is built from, and `ship` itself, each with the mode it declares.
const ACTIONS: [(&str, u32); 5] = [
    ("pr-open", 0o750),
    ("ci-wait", 0o755),
    ("pr-merge", 0o750),
    ("sync", 0o750),
    ("ship", 0o700),
];

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with the base pushed to a bare remote, the actions
/// in its actions folder, the built `zaf` on `PATH` so `ship` can call it, and `ci-wait` reading
/// the checks without waiting between reads.
fn project() -> Project {
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
    let remote = remote_dir(&project);
    jj(
        &root,
        &["git", "remote", "add", "origin", &remote.to_string_lossy()],
    );
    // The actions are the project's own files, so they sit in the base: the working copy moves
    // between commits when the base advances, and must not lose them.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\n");
    let actions = root.join(".zaf/actions");
    for (name, mode) in ACTIONS {
        write(&actions, name, &action_text(name));
        fs::set_permissions(actions.join(name), fs::Permissions::from_mode(mode)).expect("mode");
    }
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run,
    // and before the first commit so the `.zaf/.gitignore` it writes is in every commit.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);

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
    project
        .env("PATH", &search_path.to_string_lossy())
        .env("CI_WAIT_INTERVAL", "0")
        .env("CI_WAIT_GRACE", "0");
    project
}

/// The fake forge for pull request 7: none open for the bookmark, the new one is 7, its head is
/// `HEAD`, its one check is in `bucket`, it is open and merges.
fn forge(project: &mut Project, bucket: &str) {
    project
        .on("pr list *", "")
        .on("pr create *", "https://github.com/example/tracker/pull/7\n")
        .on("pr view 7 --json headRefOid*", &format!("{HEAD}\n"))
        .on("pr checks *", &format!("{bucket}\tgate\t{LINK}\n"))
        .on("pr view 7 --json state*", "OPEN\n")
        .on("run view *", "error: the gate failed\n")
        .on("pr merge *", "");
}

/// `zaf do --more ship <args>`: the exit code, the whole log (stdout) and everything printed.
fn ship(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "ship"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    let text = format!("{stdout}{stderr}");
    (code, stdout, text)
}

/// The last non-empty line of a log: what the action handed back.
fn last_line(log: &str) -> &str {
    log.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
}

fn create_call(title: &str) -> String {
    format!("pr create --head {BOOKMARK} --base {BASE} --title {title} --body  --repo {REPO}")
}

fn merge_call() -> String {
    format!("pr merge 7 --squash --match-head-commit {HEAD} --repo {REPO}")
}

fn remote_dir(project: &Project) -> PathBuf {
    project.temp.path().join("remote.git")
}

/// Runs git on the bare remote.
fn remote_git(project: &Project, args: &[&str]) -> String {
    let git_dir = remote_dir(project);
    let mut full_args = vec!["--git-dir", git_dir.to_str().expect("a UTF-8 path")];
    full_args.extend_from_slice(args);
    run(project.temp.path(), "git", &full_args)
}

/// The commit the remote holds for a branch, if it has the branch.
fn remote_ref(project: &Project, branch: &str) -> Option<String> {
    let reference = format!("refs/heads/{branch}");
    let found = remote_git(
        project,
        &["for-each-ref", "--format=%(objectname)", &reference],
    );
    let found = found.trim();
    (!found.is_empty()).then(|| found.to_owned())
}

/// The first line of the message of the commit the remote holds for a branch.
fn remote_subject(project: &Project, branch: &str) -> String {
    let reference = format!("refs/heads/{branch}");
    remote_git(project, &["log", "-1", "--format=%s", &reference])
        .trim()
        .to_owned()
}

/// One field of the revisions a revset names in the root repository.
fn field(root: &Path, revset: &str, template: &str) -> String {
    jj(
        root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            template,
        ],
    )
}

fn commit(root: &Path, revset: &str) -> String {
    field(root, revset, "commit_id")
}

fn bookmark_at(project: &Project, name: &str) -> String {
    commit(&project.root, &format!("bookmarks(exact:\"{name}\")"))
}

/// The change id of a revision, after a snapshot so a file on disk is in it.
fn change_of(project: &Project, revset: &str) -> String {
    jj(&project.root, &["status"]);
    field(&project.root, revset, "change_id")
}

/// The commit the root's working copy is and the one it sits on, after a snapshot.
fn working_copy(project: &Project) -> (String, String) {
    jj(&project.root, &["status"]);
    (commit(&project.root, "@"), commit(&project.root, "@-"))
}

/// Pushes a commit to the base from a second clone, as another merged pull request would, so the
/// root's local and remote-tracking base both lag until something fetches.
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
    write(&other, "merged.txt", "merged\n");
    run(&other, "git", &["add", "merged.txt"]);
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
            "Another merged pull request",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", BASE]);
}

/// The pushed change is where the bookmark is, locally and on the remote, and the last of the gh
/// calls is the squash merge at the head CI passed.
fn assert_shipped(project: &Project, change: &str, text: &str) -> String {
    let shipped = commit(&project.root, change);
    assert_eq!(bookmark_at(project, BOOKMARK), shipped, "{text}");
    assert_eq!(
        remote_ref(project, BOOKMARK).as_deref(),
        Some(&*shipped),
        "{text}"
    );
    let calls = project.gh_calls();
    assert_eq!(calls.last(), Some(&merge_call()), "{calls:#?}\n{text}");
    shipped
}

/// The working copy is a new, empty change on the base as the remote has it, and not on `change`.
fn assert_on_the_base(project: &Project, change: &str, text: &str) {
    let base = remote_ref(project, BASE).expect("the base");
    assert_eq!(bookmark_at(project, BASE), base, "{text}");
    let (_, parent) = working_copy(project);
    assert_eq!(parent, base, "{text}");
    assert_eq!(field(&project.root, "@", "empty"), "true", "{text}");
    assert_ne!(field(&project.root, "@", "change_id"), change, "{text}");
}

#[test]
fn ship_ships_the_change_in_the_working_copy() {
    let mut project = project();
    forge(&mut project, "pass");
    write(&project.root, "feature.txt", "feature\n");
    let change = change_of(&project, "@");
    advance_base_elsewhere(&project);

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), "7", "{text}");
    assert_shipped(&project, &change, &text);
    // The change had no description: the title is its description, on the remote too.
    assert_eq!(
        field(&project.root, &change, "description"),
        format!("{TITLE}\n")
    );
    assert_eq!(remote_subject(&project, BOOKMARK), TITLE);
    assert!(project.gh_calls().contains(&create_call(TITLE)), "{text}");
    // The sync moved the working copy off the change, onto the base the other merge moved.
    assert_on_the_base(&project, &change, &text);
    assert!(project.root.join("merged.txt").is_file());
    assert!(!project.root.join("feature.txt").exists());
}

#[test]
fn ship_ships_the_change_under_an_empty_working_copy() {
    let mut project = project();
    forge(&mut project, "pass");
    write(&project.root, "feature.txt", "feature\n");
    jj(&project.root, &["commit", "-m", "Add the feature"]);
    let change = change_of(&project, "@-");
    let described = commit(&project.root, &change);

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), "7", "{text}");
    // A change with a description keeps it, and is pushed as it was.
    let shipped = assert_shipped(&project, &change, &text);
    assert_eq!(shipped, described);
    assert_eq!(remote_subject(&project, BOOKMARK), "Add the feature");
    assert!(project.gh_calls().contains(&create_call(TITLE)), "{text}");
    assert_on_the_base(&project, &change, &text);
    assert!(!project.root.join("feature.txt").exists());
}

#[test]
fn ship_refuses_with_1_when_there_is_nothing_to_ship() {
    let mut project = project();
    forge(&mut project, "pass");
    // The working copy is empty on the base: its parent is already in the base.
    let before = working_copy(&project);

    let (code, _, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("nothing to ship"), "{text}");
    assert_eq!(project.gh_calls(), Vec::<String>::new(), "{text}");
    assert_eq!(bookmark_at(&project, BOOKMARK), "");
    assert_eq!(remote_ref(&project, BOOKMARK), None);
    assert_eq!(working_copy(&project), before);
}

#[test]
fn ship_stops_on_a_failed_check_with_the_number_last() {
    let mut project = project();
    forge(&mut project, "fail");
    write(&project.root, "feature.txt", "feature\n");
    let change = change_of(&project, "@");

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 1, "{text}");
    assert_eq!(last_line(&log), "7", "{text}");
    assert!(text.contains("failed: gate (fail)"), "{text}");
    // The change was pushed and the pull request opened, but nothing was merged or synced: the
    // working copy is the new change ship made on the pushed one.
    let calls = project.gh_calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("pr merge")),
        "{calls:#?}"
    );
    assert!(calls.contains(&create_call(TITLE)), "{calls:#?}");
    let shipped = commit(&project.root, &change);
    assert_eq!(remote_ref(&project, BOOKMARK).as_deref(), Some(&*shipped));
    assert_eq!(working_copy(&project).1, shipped);
    assert!(project.root.join("feature.txt").is_file());
}

#[test]
fn ship_stops_with_3_while_the_checks_still_run() {
    let mut project = project();
    // No wait at all: the first read of the checks is past the deadline.
    project.env("CI_WAIT_MINUTES", "0");
    forge(&mut project, "pending");
    write(&project.root, "feature.txt", "feature\n");
    let change = change_of(&project, "@");

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 3, "{text}");
    assert_eq!(last_line(&log), "7", "{text}");
    assert!(text.contains("still running: gate"), "{text}");
    let calls = project.gh_calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("pr merge")),
        "{calls:#?}"
    );
    let shipped = commit(&project.root, &change);
    assert_eq!(remote_ref(&project, BOOKMARK).as_deref(), Some(&*shipped));
    assert_eq!(working_copy(&project).1, shipped);
}

#[test]
fn ship_stops_on_a_refused_merge_with_the_number_last() {
    let mut project = project();
    forge(&mut project, "pass");
    project.fail_first(
        "pr merge *",
        "GraphQL: Head branch was modified. Review and try the merge again.",
    );
    write(&project.root, "feature.txt", "feature\n");
    let change = change_of(&project, "@");

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 1, "{text}");
    assert_eq!(last_line(&log), "7", "{text}");
    assert!(text.contains("Head branch was modified"), "{text}");
    // The merge was asked for at the head CI passed, once; no sync followed.
    let shipped = assert_shipped(&project, &change, &text);
    assert_eq!(working_copy(&project).1, shipped);
}

#[test]
fn ship_maps_a_failed_pr_open_to_2_with_no_number() {
    let mut project = project();
    forge(&mut project, "pass");
    project.fail_first("pr create *", "HTTP 502: Bad Gateway");
    write(&project.root, "feature.txt", "feature\n");

    let (code, log, text) = ship(&project, &[BOOKMARK, BASE, TITLE]);

    assert_eq!(code, 2, "{text}");
    // The reason is in ship's log; no pull request is open, so no number is handed back, and
    // nothing past it ran.
    assert!(text.contains("HTTP 502: Bad Gateway"), "{text}");
    assert_ne!(last_line(&log), "7", "{text}");
    let calls = project.gh_calls();
    assert_eq!(calls.last(), Some(&create_call(TITLE)), "{calls:#?}");
}

#[test]
fn ship_refuses_with_1_what_it_cannot_use() {
    let mut project = project();
    forge(&mut project, "pass");
    // A change to ship, so the arguments are the only reason to refuse.
    write(&project.root, "feature.txt", "feature\n");
    let change = change_of(&project, "@");

    for args in [
        &[][..],
        &[BOOKMARK, BASE][..],
        &[BOOKMARK, BASE, TITLE, "body", "extra"][..],
        &["", BASE, TITLE][..],
        &["-x", BASE, TITLE][..],
        &["feature/\"quoted", BASE, TITLE][..],
        &["feature\\slashed", BASE, TITLE][..],
        &[BOOKMARK, "", TITLE][..],
        &[BOOKMARK, "-x", TITLE][..],
        &[BASE, BASE, TITLE][..],
        &[BOOKMARK, BASE, ""][..],
    ] {
        let (code, _, text) = ship(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    // Refused before anything moved: no gh call, no bookmark, the change undescribed.
    assert_eq!(project.gh_calls(), Vec::<String>::new());
    assert_eq!(bookmark_at(&project, BOOKMARK), "");
    assert_eq!(remote_ref(&project, BOOKMARK), None);
    assert_eq!(field(&project.root, &change, "description"), "");
}
