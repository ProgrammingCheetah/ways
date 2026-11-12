//! The ways repository's `pr-open` action (docs/zaf-actions.md §3, §4, §7): run through `zaf do`
//! against the fake `gh`, it opens a slice's pull request and hands the number back as the last
//! line of its log, and a slice with an open one hands that number back and opens nothing. The
//! action asks `zaf forge open`, so the built zaf runs first on `PATH`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use common::{Project, REPO, fixtures, outcome, path_with_zaf, write};

const SLICE: &str = "feature/v1-1-2";
const BASE: &str = "feature/v1";
const TITLE: &str = "Add the CSV writer";
const BODY: &str = "Closes #2";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/pr-open");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project with the `pr-open` action in its actions folder, its gh scene empty.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project.env("PATH", &path_with_zaf(&[&fixtures()]));
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = project.root.join(".zaf/actions");
    write(&actions, "pr-open", &action_text());
    fs::set_permissions(actions.join("pr-open"), fs::Permissions::from_mode(0o750)).expect("mode");
    project
}

/// `zaf do pr-open <args>` with its whole log: the exit code and the lines.
fn open(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "pr-open"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn open_slice(project: &Project) -> (i32, String) {
    open(project, &[SLICE, BASE, TITLE, BODY])
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// The `gh pr list` call for the slice, as the fake records it.
fn list_call() -> String {
    format!("pr list --head {SLICE} --state open --json number --jq .[].number --repo {REPO}")
}

#[test]
fn pr_open_hands_back_the_number() {
    let mut project = project();
    project.on("pr list *", "").on(
        "pr create *",
        &format!("Creating pull request\nhttps://github.com/{REPO}/pull/7\n"),
    );

    let (code, text) = open_slice(&project);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "7", "{text}");
    // It looks for an open pull request of the slice, then opens one from the slice onto the
    // base with the title and body as given.
    assert_eq!(
        project.gh_calls(),
        [
            list_call(),
            format!(
                "pr create --head {SLICE} --base {BASE} --title {TITLE} --body {BODY} --repo {REPO}"
            ),
        ]
    );
}

#[test]
fn pr_open_returns_the_open_one() {
    let mut project = project();
    project.on("pr list *", "5\n");

    let (code, text) = open_slice(&project);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "5", "{text}");
    // Nothing was opened: the list was the only call.
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn pr_open_keeps_a_title_and_body_with_spaces_and_newlines_whole() {
    let mut project = project();
    project.on("pr list *", "").on(
        "pr create *",
        &format!("https://github.com/{REPO}/pull/12\n"),
    );

    let (code, text) = open(&project, &[SLICE, BASE, "Fix  the \"quote\"", "one\ntwo"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(handed_back(&text), "12", "{text}");
    assert_eq!(
        project.gh_calls()[1],
        format!(
            "pr create --head {SLICE} --base {BASE} --title Fix  the \"quote\" --body one\ntwo --repo {REPO}"
        )
    );
}

#[test]
fn pr_open_without_a_forge_fails_with_2_and_asks_no_forge() {
    let mut project = project();
    fs::write(
        project.root.join(".zaf.toml"),
        "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n",
    )
    .expect("manifest");
    project.on("pr list *", "9\n");

    let (code, text) = open_slice(&project);

    assert_eq!(code, 2, "{text}");
    assert!(text.contains("no [forge] section"), "{text}");
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn pr_open_refuses_arguments_it_cannot_use() {
    let project = project();

    for args in [
        &[][..],
        &[SLICE, BASE, TITLE][..],
        &[SLICE, BASE, TITLE, BODY, "extra"][..],
        &["", BASE, TITLE, BODY][..],
        &["-x", BASE, TITLE, BODY][..],
        &[SLICE, "", TITLE, BODY][..],
        &[SLICE, "-x", TITLE, BODY][..],
        &[SLICE, BASE, "", BODY][..],
    ] {
        let (code, text) = open(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }

    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn pr_open_fails_with_2_when_gh_fails_or_prints_no_number() {
    let mut project = project();
    project.fail_on("pr list *", "HTTP 502");
    let (code, text) = open_slice(&project);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("HTTP 502"), "{text}");

    let mut project = self::project();
    project
        .on("pr list *", "")
        .fail_on("pr create *", "a pull request already exists");
    let (code, text) = open_slice(&project);
    assert_eq!(code, 2, "{text}");

    let mut project = self::project();
    project.on("pr list *", "").on("pr create *", "done\n");
    let (code, text) = open_slice(&project);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("no pull request number"), "{text}");
}
