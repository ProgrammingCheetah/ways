//! The ways repository's `issue-file` action (docs/zaf-actions.md §7): run through `zaf do`
//! against the fake `gh`, it files an issue under an epic, with the body from a file, the labels
//! given and the epic's milestone, links it as the epic's sub-issue and hands its number back as
//! the last line of its log; an epic with a sub-issue of the same title hands that one back and
//! files nothing.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, outcome, write};

const EPIC: &str = "3";
const TITLE: &str = "Add the CSV writer";
const BODY: &str = "Write the rows as CSV.\n";
/// The number the fake files the issue as, and its database id.
const NUMBER: &str = "12";
const ID: &str = "4242";

/// The forge names another repository, before the board, so a run that files on [`REPO`] read
/// the board's.
const MANIFEST: &str = r#"[project]
name = "example"

[forge]
kind = "github"
repo = "example/code"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/issue-file");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project with the `issue-file` action in its actions folder, its gh scene empty.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = project.root.join(".zaf/actions");
    write(&actions, "issue-file", &action_text());
    fs::set_permissions(
        actions.join("issue-file"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("mode");
    project
}

/// A body file outside the root, by its absolute path.
fn body_file(project: &Project) -> String {
    let file = project.temp.path().join("body.md");
    fs::write(&file, BODY).expect("body");
    file.to_string_lossy().into_owned()
}

/// `zaf do --more issue-file <args>` run in `dir`: the exit code, the action's whole log (stdout)
/// and everything said (the log, then the `FAIL` line zaf puts on stderr).
fn file_in(project: &Project, dir: &Path, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "issue-file"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf_in(dir, &command));
    let said = format!("{stdout}{stderr}");
    (code, stdout, said)
}

/// [`file_in`] at the project root.
fn file(project: &Project, args: &[&str]) -> (i32, String, String) {
    file_in(project, &project.root, args)
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// The scene of a filing that goes through: the epic has one sub-issue of another title and the
/// milestone `milestone` (empty for none), and gh files the issue as #12 with the id 4242.
fn filing(project: &mut Project, milestone: &str) {
    project
        .on(
            "api repos/*/sub_issues --paginate*",
            "5\tAdd the CSV reader\n",
        )
        .on("issue view *", &format!("{milestone}\n"))
        .on(
            "issue create *",
            &format!("Creating issue in {REPO}\n\nhttps://github.com/{REPO}/issues/{NUMBER}\n"),
        )
        .on("api repos/* --jq .id", &format!("{ID}\n"))
        .on("api repos/*/sub_issues --method POST*", "");
}

/// The epic's sub-issues, as the fake records the call.
fn list_call() -> String {
    format!(
        "api repos/{REPO}/issues/{EPIC}/sub_issues --paginate --jq .[] | \"\\(.number)\\t\\(.title)\""
    )
}

/// The epic's milestone.
fn view_call() -> String {
    format!("issue view {EPIC} --repo {REPO} --json milestone --jq .milestone.title // \"\"")
}

/// The new issue's database id.
fn id_call() -> String {
    format!("api repos/{REPO}/issues/{NUMBER} --jq .id")
}

/// The link under the epic.
fn link_call() -> String {
    format!("api repos/{REPO}/issues/{EPIC}/sub_issues --method POST -F sub_issue_id={ID} --silent")
}

#[test]
fn issue_file_files_labels_and_links_under_the_epic() {
    let mut project = project();
    filing(&mut project, "0.2.1");
    let body = body_file(&project);

    let (code, log, said) = file(&project, &[EPIC, TITLE, &body, "task", "zaf"]);

    assert_eq!(code, 0, "{said}");
    assert_eq!(handed_back(&log), NUMBER, "{said}");
    // It looks for a sub-issue of that title, reads the epic's milestone, files the issue with
    // the body file, each label and the milestone, then reads its id and links it by that id.
    assert_eq!(
        project.gh_calls(),
        [
            list_call(),
            view_call(),
            format!(
                "issue create --repo {REPO} --title {TITLE} --body-file {body} --label task --label zaf --milestone 0.2.1"
            ),
            id_call(),
            link_call(),
        ]
    );
}

#[test]
fn issue_file_without_a_milestone_or_labels_passes_neither() {
    let mut project = project();
    filing(&mut project, "");
    let body = body_file(&project);

    let (code, log, said) = file(&project, &[EPIC, TITLE, &body]);

    assert_eq!(code, 0, "{said}");
    assert_eq!(handed_back(&log), NUMBER, "{said}");
    assert_eq!(
        project.gh_calls(),
        [
            list_call(),
            view_call(),
            format!("issue create --repo {REPO} --title {TITLE} --body-file {body}"),
            id_call(),
            link_call(),
        ]
    );
}

#[test]
fn issue_file_reads_a_relative_body_file_from_the_callers_directory() {
    let mut project = project();
    filing(&mut project, "0.2.1");
    let deep = project.root.join("src/deep");
    write(&deep, "notes/body.md", BODY);

    let (code, log, said) = file_in(&project, &deep, &[EPIC, TITLE, "notes/body.md"]);

    assert_eq!(code, 0, "{said}");
    assert_eq!(handed_back(&log), NUMBER, "{said}");
    // gh runs at the root, so the action hands it the caller's file by its absolute path.
    let body = fs::canonicalize(&deep).expect("deep").join("notes/body.md");
    assert_eq!(
        project.gh_calls()[2],
        format!(
            "issue create --repo {REPO} --title {TITLE} --body-file {} --milestone 0.2.1",
            body.display()
        )
    );
}

#[test]
fn issue_file_hands_back_the_sub_issue_with_the_same_title() {
    let mut project = project();
    project.on(
        "api repos/*/sub_issues --paginate*",
        &format!("5\tAdd the CSV reader\n9\t{TITLE} and reader\n7\t{TITLE}\n"),
    );
    let body = body_file(&project);

    let (code, log, said) = file(&project, &[EPIC, TITLE, &body, "task"]);

    assert_eq!(code, 0, "{said}");
    // The exact title, not the one it starts.
    assert_eq!(handed_back(&log), "7", "{said}");
    // Nothing was filed: the list was the only call.
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn issue_file_matches_a_title_with_a_backslash_as_written() {
    let title = r"Escape \n in a path like C:\temp";
    let mut project = project();
    project.on(
        "api repos/*/sub_issues --paginate*",
        &format!("5\tEscape n in a path like C:temp\n8\t{title}\n"),
    );
    let body = body_file(&project);

    let (code, log, said) = file(&project, &[EPIC, title, &body]);

    assert_eq!(code, 0, "{said}");
    // A retry finds the one filed before, so no second issue is filed.
    assert_eq!(handed_back(&log), "8", "{said}");
    assert_eq!(project.gh_calls(), [list_call()]);
}

#[test]
fn issue_file_that_cannot_link_exits_3_with_the_number_last() {
    let mut project = project();
    filing(&mut project, "0.2.1");
    project.fail_first("api repos/*/sub_issues --method POST*", "HTTP 422");
    let body = body_file(&project);

    let (code, log, said) = file(&project, &[EPIC, TITLE, &body]);

    assert_eq!(code, 3, "{said}");
    assert_eq!(handed_back(&log), NUMBER, "{said}");
    assert!(said.contains("could not be linked under #3"), "{said}");
    assert_eq!(project.gh_calls().last(), Some(&link_call()));

    // An id it cannot read is the same: filed, not linked, and no link is tried.
    let mut project = self::project();
    filing(&mut project, "0.2.1");
    project.fail_first("api repos/* --jq .id", "HTTP 502");

    let (code, log, said) = file(&project, &[EPIC, TITLE, &body]);

    assert_eq!(code, 3, "{said}");
    assert_eq!(handed_back(&log), NUMBER, "{said}");
    assert_eq!(project.gh_calls().last(), Some(&id_call()));
}

#[test]
fn issue_file_fails_with_2_when_gh_fails_before_filing() {
    // The create refused: nothing after it.
    let mut project = project();
    filing(&mut project, "0.2.1");
    project.fail_first("issue create *", "HTTP 502");
    let body = body_file(&project);
    let (code, _, said) = file(&project, &[EPIC, TITLE, &body]);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("HTTP 502"), "{said}");
    assert_eq!(
        project.gh_calls(),
        [
            list_call(),
            view_call(),
            format!(
                "issue create --repo {REPO} --title {TITLE} --body-file {body} --milestone 0.2.1"
            ),
        ]
    );

    // The sub-issues could not be listed: nothing filed.
    let mut project = self::project();
    project.fail_on("api repos/*/sub_issues --paginate*", "HTTP 404");
    let (code, _, said) = file(&project, &[EPIC, TITLE, &body]);
    assert_eq!(code, 2, "{said}");
    assert_eq!(project.gh_calls(), [list_call()]);

    // The epic could not be read: nothing filed.
    let mut project = self::project();
    filing(&mut project, "0.2.1");
    project.fail_first("issue view *", "HTTP 404");
    let (code, _, said) = file(&project, &[EPIC, TITLE, &body]);
    assert_eq!(code, 2, "{said}");
    assert_eq!(project.gh_calls(), [list_call(), view_call()]);

    // gh printed no issue URL: no number to read an id for.
    let mut project = self::project();
    filing(&mut project, "0.2.1");
    project.on_first("issue create *", "done\n");
    let (code, _, said) = file(&project, &[EPIC, TITLE, &body]);
    assert_eq!(code, 2, "{said}");
    assert!(said.contains("gh printed no issue number"), "{said}");
    assert_eq!(project.gh_calls().len(), 3, "{said}");
}

#[test]
fn issue_file_refuses_arguments_it_cannot_use() {
    let project = project();
    let body = body_file(&project);
    let missing = project.temp.path().join("missing.md");
    let missing = missing.to_string_lossy();
    let folder = project.temp.path().to_string_lossy();

    for args in [
        &[][..],
        &[EPIC, TITLE][..],
        &["", TITLE, &body][..],
        &["#", TITLE, &body][..],
        &["x3", TITLE, &body][..],
        &["3x", TITLE, &body][..],
        &[EPIC, "", &body][..],
        &[EPIC, TITLE, &missing][..],
        &[EPIC, TITLE, &folder][..],
        &[EPIC, TITLE, &body, ""][..],
        &[EPIC, TITLE, &body, "task", ""][..],
    ] {
        let (code, _, said) = file(&project, args);
        assert_eq!(code, 1, "{args:?}: {said}");
    }

    assert_eq!(project.gh_calls(), Vec::<String>::new());
}
