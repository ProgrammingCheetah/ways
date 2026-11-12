//! `zaf forge open|merge|checks|head|log` (docs/zaf-actions.md §4): the built binary against the
//! fake `gh` for a GitHub forge and the fake `curl` for a Forgejo one. Each verb prints what the
//! standard actions read, exits 1 for a refusal and 2 for a backend failure.

mod common;

use common::{Curl, Project, REPO, fixtures, outcome};

const GITHUB: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

const FORGEJO: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "forgejo"
repo = "example/tracker"
url = "http://forgejo.test"
token_env = "ZAF_TEST_FORGE_TOKEN"
"#;

const SLICE: &str = "feature/v1-1-2";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const API: &str = "http://forgejo.test/api/v1/repos/example/tracker";

/// A project whose verbs have made the zaf home already, so no notice reaches a run.
fn project(manifest: &str) -> Project {
    let project = Project::new(manifest);
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

fn forge(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["forge"];
    command.extend_from_slice(args);
    outcome(&project.zaf(&command))
}

#[test]
fn open_prints_the_number_and_says_when_one_was_open_already() {
    let mut project = project(GITHUB);
    project.on("pr list *", "").on(
        "pr create *",
        &format!("https://github.com/{REPO}/pull/7\n"),
    );
    let (code, stdout, stderr) = forge(
        &project,
        &[
            "open",
            "--",
            SLICE,
            "feature/v1",
            "-Add the writer",
            "Closes #2",
        ],
    );
    assert_eq!((code, stdout.as_str()), (0, "7\n"), "{stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        project.gh_calls()[1],
        format!(
            "pr create --head {SLICE} --base feature/v1 --title -Add the writer --body Closes #2 --repo {REPO}"
        )
    );

    let mut project = self::project(GITHUB);
    project.on("pr list *", "5\n");
    let (code, stdout, stderr) = forge(&project, &["open", SLICE, "feature/v1", "T", ""]);
    assert_eq!((code, stdout.as_str()), (0, "5\n"), "{stderr}");
    assert_eq!(
        stderr,
        format!("pull request #5 is already open for {SLICE}\n")
    );
    let (code, stdout, _) = forge(&project, &["--json", "open", SLICE, "feature/v1", "T", ""]);
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(json, serde_json::json!({ "pr": 5, "opened": false }));
}

#[test]
fn open_refuses_a_branch_or_title_it_cannot_use_and_asks_no_forge() {
    let project = project(GITHUB);
    for (args, said) in [
        (
            ["-x", "feature/v1", "T", "B"],
            "head `-x` is not a usable branch name",
        ),
        ([SLICE, "", "T", "B"], "base `` is not a usable branch name"),
        (
            [SLICE, "feature/v1", "", "B"],
            "a pull request needs a title",
        ),
    ] {
        let mut command = vec!["open", "--"];
        command.extend_from_slice(&args);
        let (code, _, stderr) = forge(&project, &command);
        assert_eq!(code, 1, "{args:?}: {stderr}");
        assert_eq!(stderr, format!("zaf forge open: {said}\n"));
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn merge_prints_nothing_and_refuses_with_1_or_fails_with_2() {
    let mut project = project(GITHUB);
    project.on("pr view 7 *", "OPEN\n").on("pr merge *", "");
    let (code, stdout, stderr) = forge(
        &project,
        &["merge", "#7", "--squash", "--match-head-commit", SHA],
    );
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    assert_eq!(
        project.gh_calls()[1],
        format!("pr merge 7 --squash --match-head-commit {SHA} --repo {REPO}")
    );

    // A head that is no commit id is refused before gh is asked.
    let (code, _, stderr) = forge(&project, &["merge", "7", "--match-head-commit", "main"]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "zaf forge merge: `main` is not a commit id\n");
    assert_eq!(project.gh_calls().len(), 2);

    let mut project = self::project(GITHUB);
    project
        .on("pr view 7 *", "OPEN\n")
        .fail_on(
            "pr merge 7 *",
            "X Pull request #7 Base branch was modified.",
        )
        .on("pr view 8 *", "CLOSED\n")
        .fail_on("pr view 9 *", "HTTP 502");
    let (code, _, stderr) = forge(&project, &["merge", "7"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("not mergeable yet"), "{stderr}");
    let (code, _, stderr) = forge(&project, &["merge", "8"]);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(
        stderr,
        "zaf forge merge: rejected: pull request #8 is closed unmerged\n"
    );
    let (code, _, stderr) = forge(&project, &["merge", "9"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("HTTP 502"), "{stderr}");
}

#[test]
fn checks_head_and_log_print_what_the_actions_read() {
    let link = format!("https://github.com/{REPO}/actions/runs/31/job/102");
    let mut project = project(GITHUB);
    project
        .on(
            "pr checks 7 *",
            &format!("fail\tzaf / test\t{link}\npass\tzaf / fmt\t\n"),
        )
        .on("pr view 7 *", &format!("{SHA}\n"))
        .on("pr view 8 *", "\n")
        .on("run view --job 102 *", "zaf / test\tRun tests\tpanicked\n");

    let (code, stdout, stderr) = forge(&project, &["checks", "7"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!("fail\tzaf / test\t{link}\npass\tzaf / fmt\t\n")
    );
    let (_, stdout, _) = forge(&project, &["--json", "checks", "7"]);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(json[0]["bucket"], "fail");
    assert_eq!(json[1]["link"], serde_json::Value::Null);

    assert_eq!(
        forge(&project, &["head", "7"]),
        (0, format!("{SHA}\n"), String::new())
    );
    assert_eq!(
        forge(&project, &["head", "8"]),
        (0, String::new(), String::new())
    );

    let (code, stdout, stderr) = forge(&project, &["log", "--", "zaf / test", &link]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "--- failed log lines of zaf / test (job 102)\nzaf / test\tRun tests\tpanicked\n"
    );
    let (code, stdout, _) = forge(&project, &["log", "external", "https://ci.example.com/b/5"]);
    assert_eq!((code, stdout.as_str()), (0, ""));
}

#[test]
fn a_project_with_no_forge_fails_with_2() {
    let manifest = GITHUB.replace(
        "[forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n",
        "",
    );
    let project = project(&manifest);
    for args in [
        &["open", SLICE, "feature/v1", "T", "B"][..],
        &["merge", "7"][..],
        &["checks", "7"][..],
        &["head", "7"][..],
        &["log", "ci", "https://example.com"][..],
    ] {
        let (code, _, stderr) = forge(&project, args);
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(stderr.contains("no [forge] section"), "{stderr}");
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

/// A Forgejo project whose `curl` is the fake, with the token set.
fn forgejo_project() -> (Project, Curl) {
    let mut project = project(FORGEJO);
    let curl = Curl::new(project.temp.path());
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [fixtures().join("forgejo"), fixtures()]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    project
        .env("PATH", &search_path.to_string_lossy())
        .env("FAKE_CURL_DIR", &curl.dir.to_string_lossy())
        .env("ZAF_TEST_FORGE_TOKEN", "test-token");
    (project, curl)
}

fn pull(number: u64, head: &str, state: &str, merged: bool) -> String {
    serde_json::json!({
        "number": number,
        "state": state,
        "merged": merged,
        "head": { "ref": head, "sha": SHA, "repo_id": 9 },
        "base": { "ref": "feature/v1", "sha": "fedcba", "repo_id": 9 },
    })
    .to_string()
}

#[test]
fn a_forgejo_forge_opens_merges_and_reads_its_checks_over_the_api() {
    let (project, mut curl) = forgejo_project();
    curl.on(&format!("GET {API}/pulls[?]state=open*"), 200, "[]")
        .on(
            &format!("POST {API}/pulls"),
            201,
            &pull(12, SLICE, "open", false),
        )
        .on(
            &format!("GET {API}/pulls/12"),
            200,
            &pull(12, SLICE, "open", false),
        )
        .on(&format!("POST {API}/pulls/12/merge"), 200, "")
        .on(
            &format!("GET {API}/commits/{SHA}/statuses*"),
            200,
            &serde_json::json!([
                { "id": 1, "status": "pending", "context": "ci / test (pull_request)",
                  "target_url": "/example/tracker/actions/runs/4/jobs/0",
                  "created_at": "2026-11-12T10:00:00Z" },
                { "id": 2, "status": "failure", "context": "ci / test (pull_request)",
                  "target_url": "/example/tracker/actions/runs/4/jobs/0",
                  "created_at": "2026-11-12T10:05:00Z" },
            ])
            .to_string(),
        );

    let (code, stdout, stderr) = forge(&project, &["open", SLICE, "feature/v1", "T", "B"]);
    assert_eq!((code, stdout.as_str()), (0, "12\n"), "{stderr}");

    let (code, stdout, stderr) = forge(&project, &["head", "12"]);
    assert_eq!((code, stdout), (0, format!("{SHA}\n")), "{stderr}");

    let link = "http://forgejo.test/example/tracker/actions/runs/4/jobs/0";
    let (code, stdout, stderr) = forge(&project, &["checks", "12"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("fail\tci / test (pull_request)\t{link}\n"));

    let (code, stdout, stderr) = forge(&project, &["log", "--", "ci / test (pull_request)", link]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "--- ci / test (pull_request): the forge has no API for a job's log; read it at {link}\n"
        )
    );

    let (code, stdout, stderr) = forge(&project, &["merge", "12", "--match-head-commit", SHA]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));

    assert_eq!(
        curl.asked(),
        [
            format!("GET {API}/pulls?state=open&page=1&limit=50"),
            format!("POST {API}/pulls"),
            format!("GET {API}/pulls/12"),
            format!("GET {API}/pulls/12"),
            format!("GET {API}/commits/{SHA}/statuses?page=1&limit=50"),
            format!("GET {API}/pulls/12"),
            format!("POST {API}/pulls/12/merge"),
        ]
    );
}

#[test]
fn a_forgejo_merge_it_cannot_make_yet_says_so_and_exits_1() {
    let (project, mut curl) = forgejo_project();
    curl.on(
        &format!("GET {API}/pulls/12"),
        200,
        &pull(12, SLICE, "open", false),
    )
    .on(
        &format!("POST {API}/pulls/12/merge"),
        405,
        r#"{"message":"Please try again later"}"#,
    );
    let (code, _, stderr) = forge(&project, &["merge", "12"]);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(
        stderr,
        "zaf forge merge: conflict: pull request #12 is not mergeable yet: Forgejo answered 405 Please try again later\n"
    );
}

#[test]
fn a_forgejo_forge_without_its_token_fails_with_2() {
    let (mut project, _curl) = forgejo_project();
    project.env("ZAF_TEST_FORGE_TOKEN", "");
    let (code, _, stderr) = forge(&project, &["head", "12"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("ZAF_TEST_FORGE_TOKEN"), "{stderr}");
}
