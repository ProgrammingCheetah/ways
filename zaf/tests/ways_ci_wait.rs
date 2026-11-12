//! The ways repository's `ci-wait` action (docs/zaf-actions.md §3, §7): run through `zaf do`
//! against a fake `gh`, it waits for a pull request's checks to settle at its head and hands the
//! head commit back; a failed or cancelled check is named last with its job's failed log above,
//! exit 1; checks still running at the deadline are named, exit 3; a pull request with no checks
//! after the grace passes; and a gh that fails is exit 2. The action asks `zaf forge head`,
//! `checks` and `log`, so the built zaf runs first on `PATH`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Curl, Project, REPO, fixtures, outcome, path_with_zaf, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

/// The pull request's head commit, as `gh pr view` prints it.
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// A head the pull request had before a push.
const OLD_HEAD: &str = "fedcba9876543210fedcba9876543210fedcba98";

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/ci-wait");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project with the `ci-wait` action in its actions folder, its gh scene empty. Nothing sleeps
/// between two reads, and the wait's own defaults are set so a caller's environment cannot
/// change them.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = project.root.join(".zaf/actions");
    write(&actions, "ci-wait", &action_text());
    fs::set_permissions(actions.join("ci-wait"), fs::Permissions::from_mode(0o755)).expect("mode");
    project
        .env("PATH", &path_with_zaf(&[&fixtures()]))
        .env("CI_WAIT_INTERVAL", "0")
        .env("CI_WAIT_MINUTES", "20")
        .env("CI_WAIT_GRACE", "90");
    project
}

/// `zaf do --more ci-wait <args>`: the exit code, the whole log (stdout) and what zaf said
/// about the run (stderr).
fn wait(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "ci-wait"];
    command.extend_from_slice(args);
    outcome(&project.zaf(&command))
}

/// The last non-empty line: the value an action hands back.
fn handed_back(text: &str) -> &str {
    text.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// The last `count` lines of the log.
fn last_lines(text: &str, count: usize) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].to_vec()
}

/// An Actions job's link, as `gh pr checks` prints it.
fn link(job: u32) -> String {
    format!("https://github.com/{REPO}/actions/runs/31/job/{job}")
}

/// One line of `gh pr checks`, already through its `--jq`: bucket, name and link.
fn check(bucket: &str, name: &str, link: &str) -> String {
    format!("{bucket}\t{name}\t{link}\n")
}

fn view_call(pr: &str) -> String {
    format!("pr view {pr} --json headRefOid --jq .headRefOid --repo {REPO}")
}

fn checks_call(pr: &str) -> String {
    format!(
        "pr checks {pr} --json bucket,name,link --jq .[] | [.bucket, .name, .link] | @tsv --repo {REPO}"
    )
}

fn run_call(job: u32) -> String {
    format!("run view --job {job} --log-failed --repo {REPO}")
}

#[test]
fn ci_wait_hands_back_the_head_when_every_check_passed() {
    let checks = [
        check("pass", "zaf / test", &link(101)),
        check("skipping", "zaf / release", &link(102)),
    ]
    .concat();
    for pr in ["7", "#7"] {
        let mut project = project();
        project
            .on("pr view *", &format!("{HEAD}\n"))
            .on("pr checks *", &checks);

        let (code, log, said) = wait(&project, &[pr]);

        assert_eq!(code, 0, "{pr}: {log}{said}");
        assert_eq!(handed_back(&log), HEAD, "{pr}: {log}");
        // The head, the checks, and the head again to see that no push moved it meanwhile; a
        // pass reads no log.
        assert_eq!(
            project.gh_calls(),
            [view_call("7"), checks_call("7"), view_call("7")],
            "{pr}"
        );
    }
}

#[test]
fn ci_wait_names_a_failed_check_last_with_its_failed_log_above() {
    let mut project = project();
    project
        .on("pr view *", &format!("{HEAD}\n"))
        .on(
            "pr checks *",
            &[
                check("pass", "zaf / fmt", &link(101)),
                check("fail", "zaf / test", &link(102)),
            ]
            .concat(),
        )
        .on(
            "run view *",
            "zaf / test\tRun tests\tthread 'reads_the_csv' panicked: left == right failed\n",
        );

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 1, "{log}{said}");
    assert_eq!(
        handed_back(&log),
        format!("failed: zaf / test (fail) {}", link(102)),
        "{log}"
    );
    // The failed job's log lines come first, under a line naming the job; the check that passed
    // has none read.
    let header = log
        .find("--- failed log lines of zaf / test (job 102)")
        .expect(&log);
    let failure = log.find("panicked: left == right failed").expect(&log);
    let named = log.find("failed: zaf / test (fail)").expect(&log);
    assert!(header < failure && failure < named, "{log}");
    assert!(said.contains("exit 1"), "{said}");
    assert_eq!(
        project.gh_calls(),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            run_call(102)
        ]
    );

    // A failed log gh cannot read leaves the check named all the same.
    let mut project = self::project();
    project
        .on("pr view *", &format!("{HEAD}\n"))
        .on("pr checks *", &check("fail", "zaf / test", &link(102)))
        .fail_on("run view *", "HTTP 404: the log has expired\n");
    let (code, log, said) = wait(&project, &["7"]);
    assert_eq!(code, 1, "{log}{said}");
    assert_eq!(
        handed_back(&log),
        format!("failed: zaf / test (fail) {}", link(102)),
        "{log}"
    );
    assert!(log.contains("the log has expired"), "{log}");
}

#[test]
fn ci_wait_names_every_failed_and_cancelled_check() {
    let mut project = project();
    let elsewhere = "https://ci.example.com/builds/5";
    project
        .on("pr view *", &format!("{HEAD}\n"))
        .on(
            "pr checks *",
            &[
                check("fail", "zaf / test", &link(102)),
                check("pass", "zaf / fmt", &link(103)),
                check("cancel", "zaf / clippy", &link(104)),
                check("fail", "external", elsewhere),
            ]
            .concat(),
        )
        .on("run view *", "a failed line\n");

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 1, "{log}{said}");
    assert_eq!(
        last_lines(&log, 3),
        [
            format!("failed: zaf / test (fail) {}", link(102)),
            format!("failed: zaf / clippy (cancel) {}", link(104)),
            format!("failed: external (fail) {elsewhere}"),
        ],
        "{log}"
    );
    // Each Actions job's failed log is read, in order; a check that links elsewhere has no job
    // to read.
    assert_eq!(
        project.gh_calls(),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            run_call(102),
            run_call(104),
        ]
    );
}

#[test]
fn ci_wait_still_running_at_the_deadline_exits_3() {
    let mut project = project();
    project.on("pr view *", &format!("{HEAD}\n")).on(
        "pr checks *",
        &[
            check("pass", "zaf / fmt", &link(101)),
            check("pending", "zaf / test", &link(102)),
            check("pending", "zaf / clippy", &link(103)),
        ]
        .concat(),
    );

    // `--minutes` wins over `CI_WAIT_MINUTES`, which the project sets to 20.
    let (code, log, said) = wait(&project, &["7", "--minutes", "0"]);

    assert_eq!(code, 3, "{log}{said}");
    assert_eq!(
        last_lines(&log, 3),
        [
            "still running: zaf / test",
            "still running: zaf / clippy",
            HEAD
        ],
        "{log}"
    );
    // One read of each: the deadline had passed by the first.
    assert_eq!(project.gh_calls(), [view_call("7"), checks_call("7")]);
}

#[test]
fn ci_wait_takes_its_minutes_from_the_environment() {
    let mut project = project();
    project
        .env("CI_WAIT_MINUTES", "0")
        .on("pr view *", &format!("{HEAD}\n"))
        .on("pr checks *", &check("pending", "zaf / test", &link(102)));

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 3, "{log}{said}");
    assert_eq!(
        last_lines(&log, 2),
        ["still running: zaf / test", HEAD],
        "{log}"
    );
    assert_eq!(project.gh_calls(), [view_call("7"), checks_call("7")]);

    // A value that is no whole number is refused before gh is called.
    for minutes in ["soon", "-1", "1.5"] {
        let mut project = self::project();
        project.env("CI_WAIT_MINUTES", minutes);
        let (code, log, said) = wait(&project, &["7"]);
        assert_eq!(code, 1, "{minutes}: {log}{said}");
        assert!(log.contains("CI_WAIT_MINUTES"), "{log}");
        assert_eq!(project.gh_calls(), Vec::<String>::new(), "{minutes}");
    }
}

#[test]
fn ci_wait_no_checks_after_the_grace_passes() {
    let mut project = project();
    project
        .env("CI_WAIT_GRACE", "0")
        .on("pr view *", &format!("{HEAD}\n"))
        .fail_on(
            "pr checks *",
            "no checks reported on the 'feature/v1-1-2' branch\n",
        );

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), HEAD, "{log}");
    assert!(
        log.contains(&format!("pull request #7 reports no checks at {HEAD}")),
        "{log}"
    );
    assert_eq!(project.gh_calls(), [view_call("7"), checks_call("7")]);
}

#[test]
fn ci_wait_refuses_arguments_it_cannot_use() {
    let project = project();

    for args in [
        &[][..],
        &["seven"][..],
        &[""][..],
        &["#"][..],
        &["-7"][..],
        &["7;1"][..],
        &["7", "8"][..],
        &["7", "--fast"][..],
        &["7", "--minutes"][..],
        &["7", "--minutes", ""][..],
        &["7", "--minutes", "ten"][..],
        &["7", "--minutes", "-1"][..],
        &["--minutes", "0", "7"][..],
    ] {
        let (code, log, said) = wait(&project, args);
        assert_eq!(code, 1, "{args:?}: {log}{said}");
    }

    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn ci_wait_fails_with_2_when_gh_fails_or_prints_no_head() {
    let mut project = project();
    project.fail_on("pr view *", "HTTP 502");
    let (code, log, said) = wait(&project, &["7"]);
    assert_eq!(code, 2, "{log}{said}");
    assert!(log.contains("HTTP 502"), "{log}");
    assert_eq!(project.gh_calls(), [view_call("7")]);

    // The checks gh cannot read, for any reason but there being none.
    let mut project = self::project();
    project
        .on("pr view *", &format!("{HEAD}\n"))
        .fail_on("pr checks *", "HTTP 502");
    let (code, log, said) = wait(&project, &["7"]);
    assert_eq!(code, 2, "{log}{said}");
    assert!(log.contains("HTTP 502"), "{log}");
    assert_eq!(project.gh_calls(), [view_call("7"), checks_call("7")]);

    let mut project = self::project();
    project.on("pr view *", "");
    let (code, log, said) = wait(&project, &["7"]);
    assert_eq!(code, 2, "{log}{said}");
    assert!(log.contains("no head commit"), "{log}");
    assert_eq!(project.gh_calls(), [view_call("7")]);
}

/// One answer of the changing gh's `pr checks`: its exit code, stdout and stderr.
type Answer<'a> = (u8, &'a str, &'a str);

/// A directory holding a `gh` whose answers change between calls, which the fixtures' scripted gh
/// cannot do: the n-th `pr view` prints the n-th of `heads`, the n-th `pr checks` gives the n-th
/// of `checks`, and once a kind's answers run out its last one repeats. `run view` prints a failed
/// line. Every call is recorded in `calls`, one per line.
fn bin(dir: &Path, heads: &[&str], checks: &[Answer<'_>]) {
    for (index, head) in heads.iter().enumerate() {
        write(dir, &format!("head-{}", index + 1), &format!("{head}\n"));
    }
    for (index, (code, stdout, stderr)) in checks.iter().enumerate() {
        let answer = format!("checks-{}", index + 1);
        write(dir, &answer, stdout);
        write(dir, &format!("{answer}.err"), stderr);
        write(dir, &format!("{answer}.code"), &code.to_string());
    }
    let gh = format!(
        r#"#!/bin/sh
here=$(dirname "$0")
printf '%s\n' "$*" >> "$here/calls"
next() {{
    count=$(cat "$here/$1.count" 2>/dev/null || echo 0)
    count=$((count + 1))
    echo "$count" > "$here/$1.count"
    if [ "$count" -gt "$2" ]; then count=$2; fi
    echo "$here/$1-$count"
}}
case "$1 $2" in
    "pr view") cat "$(next head {heads})" ;;
    "pr checks")
        answer=$(next checks {checks})
        cat "$answer"
        cat "$answer.err" >&2
        exit "$(cat "$answer.code")"
        ;;
    "run view") echo "zaf / test: Run tests: error: 1 test failed" ;;
    *) echo "unexpected: $*" >&2; exit 97 ;;
esac
"#,
        heads = heads.len(),
        checks = checks.len(),
    );
    write(dir, "gh", &gh);
    fs::set_permissions(dir.join("gh"), fs::Permissions::from_mode(0o755)).expect("mode");
}

/// The calls the changing gh recorded, in order.
fn recorded(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A project whose `PATH` starts with the changing gh's directory, which it returns, then the
/// built zaf.
fn project_with_bin(heads: &[&str], checks: &[Answer<'_>]) -> (Project, PathBuf) {
    let mut project = project();
    let dir = project.temp.path().join("bin");
    bin(&dir, heads, checks);
    project.env("PATH", &path_with_zaf(&[&dir]));
    (project, dir)
}

#[test]
fn ci_wait_polls_until_the_checks_settle() {
    // gh exits 8 while a check is pending; the words decide, not the code.
    let pending = check("pending", "zaf / test", &link(102));
    let passed = check("pass", "zaf / test", &link(102));
    let (project, dir) = project_with_bin(&[HEAD], &[(8, &pending, ""), (0, &passed, "")]);

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), HEAD, "{log}");
    assert_eq!(
        recorded(&dir),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            checks_call("7"),
            view_call("7"),
        ]
    );
}

#[test]
fn ci_wait_reads_failed_checks_from_a_gh_that_exits_non_zero() {
    // gh exits 1 when a check failed, its list printed all the same.
    let failed = check("fail", "zaf / test", &link(102));
    let (project, dir) = project_with_bin(&[HEAD], &[(1, &failed, "")]);

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 1, "{log}{said}");
    assert_eq!(
        handed_back(&log),
        format!("failed: zaf / test (fail) {}", link(102)),
        "{log}"
    );
    assert!(log.contains("error: 1 test failed"), "{log}");
    assert_eq!(
        recorded(&dir),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            run_call(102)
        ]
    );
}

#[test]
fn ci_wait_starts_over_when_the_head_moves() {
    // The checks settled, but a push moved the head meanwhile: they were another commit's.
    let passed = check("pass", "zaf / test", &link(102));
    let (project, dir) = project_with_bin(&[OLD_HEAD, HEAD], &[(0, &passed, "")]);

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), HEAD, "{log}");
    assert!(log.contains(&format!("moved to {HEAD}")), "{log}");
    assert_eq!(
        recorded(&dir),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            view_call("7"),
            checks_call("7"),
            view_call("7"),
        ]
    );
}

#[test]
fn ci_wait_waits_within_the_grace_for_checks_to_register() {
    // A fresh push shows no checks for a moment; within the grace (90 seconds) that is no pass.
    let none = "no checks reported on the 'feature/v1-1-2' branch\n";
    let passed = check("pass", "zaf / test", &link(102));
    let (project, dir) = project_with_bin(&[HEAD], &[(1, "", none), (0, &passed, "")]);

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 0, "{log}{said}");
    assert_eq!(handed_back(&log), HEAD, "{log}");
    assert!(!log.contains("reports no checks"), "{log}");
    assert_eq!(
        recorded(&dir),
        [
            view_call("7"),
            checks_call("7"),
            view_call("7"),
            checks_call("7"),
            view_call("7"),
        ]
    );
}

#[test]
fn ci_wait_on_a_forgejo_forge_names_a_failed_check_with_its_link() {
    let forgejo = MANIFEST.replace(
        "[forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n",
        "[forge]\nkind = \"forgejo\"\nrepo = \"example/tracker\"\nurl = \"http://forgejo.test\"\n\
         token_env = \"ZAF_TEST_FORGE_TOKEN\"\n",
    );
    let mut project = project();
    fs::write(project.root.join(".zaf.toml"), forgejo).expect("manifest");
    let api = "http://forgejo.test/api/v1/repos/example/tracker";
    let mut curl = Curl::new(project.temp.path());
    curl.on(
        &format!("GET {api}/pulls/7"),
        200,
        &serde_json::json!({
            "number": 7,
            "state": "open",
            "merged": false,
            "head": { "ref": "feature/v1-1-2", "sha": HEAD, "repo_id": 9 },
        })
        .to_string(),
    )
    .on(
        &format!("GET {api}/commits/{HEAD}/statuses*"),
        200,
        &serde_json::json!([
            { "id": 1, "status": "success", "context": "ci / fmt (pull_request)",
              "target_url": "/example/tracker/actions/runs/4/jobs/0",
              "created_at": "2026-11-12T10:00:00Z" },
            { "id": 2, "status": "failure", "context": "ci / test (pull_request)",
              "target_url": "/example/tracker/actions/runs/4/jobs/1",
              "created_at": "2026-11-12T10:05:00Z" },
        ])
        .to_string(),
    );
    project
        .env(
            "PATH",
            &path_with_zaf(&[&fixtures().join("forgejo"), &fixtures()]),
        )
        .env("FAKE_CURL_DIR", &curl.dir.to_string_lossy())
        .env("ZAF_TEST_FORGE_TOKEN", "test-token");

    let (code, log, said) = wait(&project, &["7"]);

    assert_eq!(code, 1, "{log}{said}");
    let link = "http://forgejo.test/example/tracker/actions/runs/4/jobs/1";
    assert_eq!(
        handed_back(&log),
        format!("failed: ci / test (pull_request) (fail) {link}"),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "--- ci / test (pull_request): the forge has no API for a job's log; read it at {link}"
        )),
        "{log}"
    );
    // The head, the checks, the head again; no gh at all.
    assert_eq!(
        curl.asked(),
        [
            format!("GET {api}/pulls/7"),
            format!("GET {api}/pulls/7"),
            format!("GET {api}/commits/{HEAD}/statuses?page=1&limit=50"),
            format!("GET {api}/pulls/7"),
        ]
    );
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}
