use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::notes::LeaseNote;
use super::*;
use crate::domain::{Instance, InstanceNumber, IssueNumber, Location, RoleName, TaskLabel};
use crate::manifest::RepoName;
use crate::ports::{
    Author, BaseName, Board, Check, CheckName, CheckState, ClaimOutcome, ClaimRecord, Forge,
    NewQuestion, PortError, PrMerge, PrNumber, PrRequest, ReviewComment, RuleId, ScanningAlert,
    SliceName, Ticket,
};

const REPO: &str = "example/tracker";
const ISSUE_FIELDS: &str = "number,title,body,labels,parent,state,comments";

/// A scene for the fake `gh` in `tests/fixtures/`: its rules, responses and recorded calls.
struct FakeGh {
    scene: tempfile::TempDir,
    responses: Cell<usize>,
}

impl FakeGh {
    fn new() -> Self {
        let scene = tempfile::tempdir().expect("scene directory");
        fs::write(scene.path().join("rules"), "").expect("rules file");
        Self {
            scene,
            responses: Cell::new(0),
        }
    }

    /// Answers calls whose joined argv matches the glob `pattern` with `stdout`.
    fn on(&self, pattern: &str, stdout: &str) -> &Self {
        self.rule(pattern, 0, stdout)
    }

    /// Fails calls matching `pattern` with exit code 1 and `stderr`.
    fn fail_on(&self, pattern: &str, stderr: &str) -> &Self {
        self.rule(pattern, 1, stderr)
    }

    fn rule(&self, pattern: &str, status: u8, text: &str) -> &Self {
        let index = self.responses.get();
        self.responses.set(index + 1);
        let response = format!("response-{index}");
        fs::write(self.scene.path().join(&response), text).expect("response file");
        let mut rules = OpenOptions::new()
            .append(true)
            .open(self.scene.path().join("rules"))
            .expect("rules file");
        writeln!(rules, "{pattern}\t{status}\t{response}").expect("rule");
        self
    }

    fn gh(&self) -> GhCommand {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            std::iter::once(fixtures).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        GhCommand::from(RepoName::from(REPO))
            .env("PATH", search_path)
            .env("FAKE_GH_DIR", self.scene.path())
    }

    fn board(&self) -> GithubBoard {
        GithubBoard::from(self.gh())
    }

    fn forge(&self) -> GithubForge {
        GithubForge::from(self.gh())
    }

    /// Every call's argv, in order.
    fn calls(&self) -> Vec<Vec<String>> {
        let Ok(entries) = fs::read_dir(self.scene.path().join("calls")) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries.map(|entry| entry.expect("call").path()).collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                let recorded = fs::read_to_string(file).expect("recorded call");
                recorded.split_terminator('\0').map(str::to_owned).collect()
            })
            .collect()
    }
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn view(number: &str) -> Vec<String> {
    argv(&[
        "issue",
        "view",
        number,
        "--json",
        ISSUE_FIELDS,
        "--repo",
        REPO,
    ])
}

fn instance(number: u32) -> Instance {
    Instance {
        role: RoleName::from("programmer"),
        epic: IssueNumber::from(1),
        number: InstanceNumber::from(number),
    }
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// An issue as `gh issue view --json` prints it; comments get ids 100, 101, ….
fn issue(number: u64, labels: &[&str], comments: &[String], state: &str) -> String {
    let labels: Vec<_> = labels
        .iter()
        .map(|name| serde_json::json!({ "name": name }))
        .collect();
    let comments: Vec<_> = comments
        .iter()
        .enumerate()
        .map(|(index, body)| {
            let url = format!(
                "https://github.com/{REPO}/issues/{number}#issuecomment-{}",
                100 + index
            );
            serde_json::json!({ "body": body, "url": url })
        })
        .collect();
    let issue = serde_json::json!({
        "number": number,
        "title": format!("task {number}"),
        "body": "Goal: export the list",
        "labels": labels,
        "parent": { "number": 1 },
        "state": state,
        "comments": comments,
    });
    issue.to_string()
}

fn claim_line(holder: u32, seconds: u64) -> String {
    let note = LeaseNote::Claim {
        instance: instance(holder),
        at: at(seconds),
    };
    note.to_string()
}

fn heartbeat_line(holder: u32, seconds: u64) -> String {
    let note = LeaseNote::Heartbeat {
        instance: instance(holder),
        at: at(seconds),
    };
    note.to_string()
}

#[test]
fn lease_notes_read_back_what_they_write() {
    assert_eq!(
        claim_line(1, 1_800_000_000),
        "claimed by programmer instance 1 on #1 at 2027-01-15T08:00:00Z"
    );
    assert_eq!(
        heartbeat_line(2, 1_800_000_000),
        "heartbeat programmer instance 2 on #1 2027-01-15T08:00:00Z"
    );
    let expected_claim = LeaseNote::Claim {
        instance: instance(1),
        at: at(1_800_000_000),
    };
    assert_eq!(claim_line(1, 1_800_000_000).parse(), Ok(expected_claim));
    assert!("a plain comment".parse::<LeaseNote>().is_err());
    assert!("claimed by nobody at noon".parse::<LeaseNote>().is_err());
}

#[test]
fn ready_lists_ready_tasks_in_issue_order() {
    let fake = FakeGh::new();
    let listed = format!(
        "[{},{}]",
        issue(4, &["task", "ready"], &[], "OPEN"),
        issue(2, &["task", "ready"], &[], "OPEN")
    );
    fake.on("issue list *", &listed);
    let tickets = fake.board().ready().expect("ready");
    let numbers: Vec<IssueNumber> = tickets.iter().map(|ticket| ticket.number).collect();
    assert_eq!(numbers, [IssueNumber::from(2), IssueNumber::from(4)]);
    assert_eq!(tickets[0].epic, Some(IssueNumber::from(1)));
    assert_eq!(tickets[0].label, Some(TaskLabel::Ready));
    let expected_call = argv(&[
        "issue",
        "list",
        "--label",
        "task",
        "--label",
        "ready",
        "--state",
        "open",
        "--limit",
        "1000",
        "--json",
        "number,title,body,labels,parent",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn task_reads_one_issue_and_a_failure_carries_stderr() {
    let fake = FakeGh::new();
    fake.on(
        "issue view 3 *",
        &issue(3, &["task", "blocked"], &[], "OPEN"),
    )
    .fail_on(
        "issue view 9 *",
        "GraphQL: Could not resolve to an Issue with the number of 9.",
    )
    .fail_on("issue view 10 *", "HTTP 502: bad gateway");
    let expected_ticket = Ticket {
        number: IssueNumber::from(3),
        epic: Some(IssueNumber::from(1)),
        title: "task 3".to_owned(),
        body: "Goal: export the list".to_owned(),
        label: Some(TaskLabel::Blocked),
    };
    assert_eq!(fake.board().task(IssueNumber::from(3)), Ok(expected_ticket));
    let missing_issue_error = PortError::NotFound(
        "gh issue view: GraphQL: Could not resolve to an Issue with the number of 9.".to_owned(),
    );
    assert_eq!(
        fake.board().task(IssueNumber::from(9)),
        Err(missing_issue_error)
    );
    let failed_call_error = PortError::Backend("gh issue view: HTTP 502: bad gateway".to_owned());
    assert_eq!(
        fake.board().task(IssueNumber::from(10)),
        Err(failed_call_error)
    );
    assert_eq!(fake.calls(), [view("3"), view("9"), view("10")]);
}

#[test]
fn every_call_on_a_missing_issue_is_not_found() {
    let fake = FakeGh::new();
    let gone = "GraphQL: Could not resolve to an Issue with the number of 9.";
    fake.fail_on("issue *", gone);
    let board = fake.board();
    let missing = IssueNumber::from(9);
    let results = [
        board.task(missing).map(drop),
        board.claim(missing, &instance(1)).map(drop),
        board.heartbeat(missing, &instance(1)),
        board.release(missing),
        board.label(missing, TaskLabel::Done),
        board.comment(missing, "Blocked."),
        board.report(missing, &instance(1), "working"),
        board.record_ruling(missing, "Title only."),
    ];
    for result in results {
        assert!(matches!(result, Err(PortError::NotFound(_))), "{result:?}");
    }
}

#[test]
fn a_new_claim_comments_then_labels() {
    let fake = FakeGh::new();
    fake.on("issue view 2 *", &issue(2, &["task", "ready"], &[], "OPEN"))
        .on(
            "issue view 3 *",
            &issue(3, &["task", "blocked"], &[], "OPEN"),
        )
        .on("issue comment *", "")
        .on("issue edit *", "");
    let board = fake.board();
    assert_eq!(
        board.claim(IssueNumber::from(2), &instance(1)),
        Ok(ClaimOutcome::Claimed)
    );
    assert!(matches!(
        board.claim(IssueNumber::from(3), &instance(1)),
        Err(PortError::Rejected(_))
    ));
    let calls = fake.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0], view("2"));
    assert_eq!(calls[1][..4], argv(&["issue", "comment", "2", "--body"]));
    let posted = calls[1][4].parse::<LeaseNote>().expect("a claim line");
    assert!(matches!(posted, LeaseNote::Claim { instance: holder, .. } if holder == instance(1)));
    let expected_edit = argv(&[
        "issue",
        "edit",
        "2",
        "--add-label",
        "claimed",
        "--remove-label",
        "ready",
        "--repo",
        REPO,
    ]);
    assert_eq!(calls[2], expected_edit);
    assert_eq!(calls[3], view("3"));
}

#[test]
fn a_repeated_claim_is_quiet_for_the_holder_and_a_conflict_for_others() {
    let fake = FakeGh::new();
    let comments = [claim_line(1, 1_800_000_000)];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &comments, "OPEN"),
    );
    let board = fake.board();
    assert_eq!(
        board.claim(IssueNumber::from(2), &instance(1)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    assert!(matches!(
        board.claim(IssueNumber::from(2), &instance(2)),
        Err(PortError::Conflict(_))
    ));
    assert_eq!(fake.calls(), [view("2"), view("2")]);
}

#[test]
fn a_heartbeat_is_a_new_comment_and_claims_read_the_latest() {
    let fake = FakeGh::new();
    let comments = [
        claim_line(2, 1_700_000_000),
        claim_line(1, 1_800_000_000),
        heartbeat_line(1, 1_800_000_600),
        heartbeat_line(2, 1_800_000_900),
    ];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &comments, "OPEN"),
    )
    .on("issue list --label claimed *", r#"[{"number":2}]"#)
    .on("issue comment *", "");
    let board = fake.board();
    board
        .heartbeat(IssueNumber::from(2), &instance(1))
        .expect("heartbeat");
    assert!(matches!(
        board.heartbeat(IssueNumber::from(2), &instance(2)),
        Err(PortError::Conflict(_))
    ));
    let expected_claims = vec![ClaimRecord {
        task: IssueNumber::from(2),
        instance: instance(1),
        heartbeat: at(1_800_000_600),
    }];
    assert_eq!(board.claims(), Ok(expected_claims));
    let calls = fake.calls();
    assert_eq!(calls[1][..4], argv(&["issue", "comment", "2", "--body"]));
    let posted = calls[1][4].parse::<LeaseNote>().expect("a heartbeat line");
    assert!(
        matches!(posted, LeaseNote::Heartbeat { instance: holder, .. } if holder == instance(1))
    );
    let comment_calls = calls.iter().filter(|call| call[1] == "comment").count();
    assert_eq!(comment_calls, 1, "a refused heartbeat writes nothing");
}

#[test]
fn release_returns_a_claimed_task_to_ready_and_leaves_others() {
    let fake = FakeGh::new();
    let comments = [claim_line(1, 1_800_000_000)];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &comments, "OPEN"),
    )
    .on("issue view 4 *", &issue(4, &["task", "ready"], &[], "OPEN"))
    .on("issue edit *", "");
    let board = fake.board();
    board.release(IssueNumber::from(2)).expect("release");
    board
        .release(IssueNumber::from(4))
        .expect("a release without a claim is quiet");
    let expected_edit = argv(&[
        "issue",
        "edit",
        "2",
        "--add-label",
        "ready",
        "--remove-label",
        "claimed",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [view("2"), expected_edit, view("4")]);
}

#[test]
fn label_replaces_the_state_label_and_an_existing_one_is_quiet() {
    let fake = FakeGh::new();
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &[], "OPEN"),
    )
    .on("issue view 4 *", &issue(4, &["task", "done"], &[], "OPEN"))
    .on("issue edit *", "");
    let board = fake.board();
    board
        .label(IssueNumber::from(2), TaskLabel::Done)
        .expect("label");
    board
        .label(IssueNumber::from(4), TaskLabel::Done)
        .expect("the same label again");
    let expected_edit = argv(&[
        "issue",
        "edit",
        "2",
        "--add-label",
        "done",
        "--remove-label",
        "claimed",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [view("2"), expected_edit, view("4")]);
}

#[test]
fn comment_posts_the_body() {
    let fake = FakeGh::new();
    fake.on("issue comment *", "");
    fake.board()
        .comment(IssueNumber::from(2), "Blocked on the schema.")
        .expect("comment");
    let expected_call = argv(&[
        "issue",
        "comment",
        "2",
        "--body",
        "Blocked on the schema.",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn report_posts_once_then_edits_its_own_comment() {
    let marker = "<!-- zaf:report programmer instance 1 on #1 -->";
    let fake = FakeGh::new();
    let edited = ["a plain comment".to_owned(), format!("{marker}\nworking")];
    let unchanged = [format!("{marker}\ndone")];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &[], "OPEN"),
    )
    .on(
        "issue view 3 *",
        &issue(3, &["task", "claimed"], &edited, "OPEN"),
    )
    .on(
        "issue view 4 *",
        &issue(4, &["task", "claimed"], &unchanged, "OPEN"),
    )
    .on("issue comment *", "")
    .on("api *", "{}");
    let board = fake.board();
    board
        .report(IssueNumber::from(2), &instance(1), "working")
        .expect("first report");
    board
        .report(IssueNumber::from(3), &instance(1), "done")
        .expect("second report");
    board
        .report(IssueNumber::from(4), &instance(1), "done")
        .expect("the same report");
    let first_body = format!("{marker}\nworking");
    let edit_field = format!("body={marker}\ndone");
    let expected_calls = vec![
        view("2"),
        argv(&[
            "issue",
            "comment",
            "2",
            "--body",
            &first_body,
            "--repo",
            REPO,
        ]),
        view("3"),
        argv(&[
            "api",
            &format!("repos/{REPO}/issues/comments/101"),
            "--method",
            "PATCH",
            "-f",
            &edit_field,
        ]),
        view("4"),
    ];
    assert_eq!(fake.calls(), expected_calls);
}

#[test]
fn open_question_creates_a_labelled_issue() {
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/7\n"),
    );
    let question = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: true,
    };
    assert_eq!(
        fake.board().open_question(&question),
        Ok(IssueNumber::from(7))
    );
    let expected_call = argv(&[
        "issue",
        "create",
        "--title",
        "Which columns does the export carry?",
        "--body",
        "Raised on #2.",
        "--label",
        "question",
        "--label",
        "door",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn record_ruling_comments_and_closes_once() {
    let fake = FakeGh::new();
    let ruled = ["<!-- zaf:ruling -->\nTitle and author.".to_owned()];
    fake.on("issue view 7 *", &issue(7, &["question"], &[], "OPEN"))
        .on("issue view 8 *", &issue(8, &["question"], &ruled, "CLOSED"))
        .on("issue comment *", "")
        .on("issue close *", "");
    let board = fake.board();
    board
        .record_ruling(IssueNumber::from(7), "Title and author.")
        .expect("ruling");
    board
        .record_ruling(IssueNumber::from(8), "Title and author.")
        .expect("the same ruling is quiet");
    assert!(matches!(
        board.record_ruling(IssueNumber::from(8), "Title only."),
        Err(PortError::Conflict(_))
    ));
    let expected_calls = vec![
        view("7"),
        argv(&[
            "issue",
            "comment",
            "7",
            "--body",
            "<!-- zaf:ruling -->\nTitle and author.",
            "--repo",
            REPO,
        ]),
        argv(&["issue", "close", "7", "--repo", REPO]),
        view("8"),
        view("8"),
    ];
    assert_eq!(fake.calls(), expected_calls);
}

fn request() -> PrRequest {
    PrRequest {
        slice: SliceName::from("feature/v1-export-2"),
        base: BaseName::from("feature/v1"),
        title: "Add the CSV writer".to_owned(),
        body: "Closes #2".to_owned(),
    }
}

#[test]
fn open_pr_creates_one_or_returns_the_open_one() {
    let fake = FakeGh::new();
    fake.on("pr list *", "[]").on(
        "pr create *",
        &format!("https://github.com/{REPO}/pull/7\n"),
    );
    assert_eq!(fake.forge().open_pr(&request()), Ok(PrNumber::from(7)));
    let list_call = argv(&[
        "pr",
        "list",
        "--head",
        "feature/v1-export-2",
        "--state",
        "open",
        "--json",
        "number",
        "--repo",
        REPO,
    ]);
    let create_call = argv(&[
        "pr",
        "create",
        "--head",
        "feature/v1-export-2",
        "--base",
        "feature/v1",
        "--title",
        "Add the CSV writer",
        "--body",
        "Closes #2",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [list_call.clone(), create_call]);

    let existing = FakeGh::new();
    existing.on("pr list *", r#"[{"number":5}]"#);
    assert_eq!(existing.forge().open_pr(&request()), Ok(PrNumber::from(5)));
    assert_eq!(existing.calls(), [list_call]);
}

#[test]
fn checks_map_gh_buckets_and_none_reported_is_empty() {
    let fake = FakeGh::new();
    let checks = r#"[
        {"name":"ci","bucket":"pass"},
        {"name":"lint","bucket":"fail"},
        {"name":"scan","bucket":"pending"},
        {"name":"docs","bucket":"skipping"},
        {"name":"bench","bucket":"cancel"}
    ]"#;
    fake.on("pr checks 7 *", checks).fail_on(
        "pr checks 8 *",
        "no checks reported on the 'feature/v1-export-2' branch",
    );
    let check = |name: &str, state: CheckState| Check {
        name: CheckName::from(name),
        state,
    };
    let expected_checks = vec![
        check("ci", CheckState::Passed),
        check("lint", CheckState::Failed),
        check("scan", CheckState::Pending),
        check("docs", CheckState::Passed),
        check("bench", CheckState::Failed),
    ];
    let forge = fake.forge();
    assert_eq!(forge.checks(PrNumber::from(7)), Ok(expected_checks));
    assert_eq!(forge.checks(PrNumber::from(8)), Ok(Vec::new()));
    let expected_call = argv(&["pr", "checks", "7", "--json", "name,bucket", "--repo", REPO]);
    assert_eq!(fake.calls()[0], expected_call);
}

#[test]
fn review_comments_merge_reviews_and_line_comments_oldest_first() {
    let fake = FakeGh::new();
    let lines = r#"[[
        {"user":{"login":"reviewer"},"body":"Name this binding.","path":"src/export.rs","line":12,"created_at":"2027-01-15T08:10:00Z"}
    ],[
        {"user":null,"body":"Outdated.","path":"src/old.rs","line":null,"created_at":"2027-01-15T08:00:00Z"}
    ]]"#;
    let reviews = r#"[[
        {"user":{"login":"reviewer"},"body":"Close; two nits.","submitted_at":"2027-01-15T08:05:00Z"},
        {"user":{"login":"bot"},"body":"","submitted_at":"2027-01-15T08:06:00Z"},
        {"user":{"login":"reviewer"},"body":"Draft.","submitted_at":null}
    ]]"#;
    fake.on(&format!("api repos/{REPO}/pulls/7/comments *"), lines)
        .on(&format!("api repos/{REPO}/pulls/7/reviews *"), reviews);
    let expected_comments = vec![
        ReviewComment {
            author: Author::from("ghost"),
            body: "Outdated.".to_owned(),
            location: Some(Location {
                path: "src/old.rs".to_owned(),
                line: None,
            }),
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Close; two nits.".to_owned(),
            location: None,
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Name this binding.".to_owned(),
            location: Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(12),
            }),
        },
    ];
    assert_eq!(
        fake.forge().review_comments(PrNumber::from(7)),
        Ok(expected_comments)
    );
    let expected_calls = vec![
        argv(&[
            "api",
            &format!("repos/{REPO}/pulls/7/comments"),
            "--paginate",
            "--slurp",
        ]),
        argv(&[
            "api",
            &format!("repos/{REPO}/pulls/7/reviews"),
            "--paginate",
            "--slurp",
        ]),
    ];
    assert_eq!(fake.calls(), expected_calls);
}

#[test]
fn scanning_alerts_read_the_open_alerts_of_the_head_ref() {
    let fake = FakeGh::new();
    let alerts = r#"[[
        {"rule":{"id":"js/sql-injection"},"most_recent_instance":{"location":{"path":"src/export.rs","start_line":40}}}
    ],[
        {"rule":{"id":"js/unused-import"},"most_recent_instance":null},
        {"rule":{"id":"js/path-injection"},"most_recent_instance":{"location":{"path":"src/read.rs"}}}
    ]]"#;
    let endpoint = format!("repos/{REPO}/code-scanning/alerts?ref=refs/pull/7/head&state=open");
    fake.on(&format!("api {endpoint} *"), alerts);
    let alert = |rule: &str, location: Option<Location>| ScanningAlert {
        rule: RuleId::from(rule),
        location,
    };
    let expected_alerts = vec![
        alert(
            "js/sql-injection",
            Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(40),
            }),
        ),
        alert("js/unused-import", None),
        alert(
            "js/path-injection",
            Some(Location {
                path: "src/read.rs".to_owned(),
                line: None,
            }),
        ),
    ];
    assert_eq!(
        fake.forge().scanning_alerts(PrNumber::from(7)),
        Ok(expected_alerts)
    );
    let expected_calls = vec![argv(&["api", &endpoint, "--paginate", "--slurp"])];
    assert_eq!(fake.calls(), expected_calls);

    let disabled = FakeGh::new();
    disabled.fail_on("api *", "Code scanning is not enabled for this repository");
    assert!(matches!(
        disabled.forge().scanning_alerts(PrNumber::from(7)),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn merge_merges_once_and_a_merged_pr_is_quiet() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", r#"{"state":"OPEN"}"#)
        .on("pr view 8 *", r#"{"state":"MERGED"}"#)
        .on("pr merge *", "");
    let forge = fake.forge();
    assert_eq!(forge.merge(PrNumber::from(7)), Ok(PrMerge::Merged));
    assert_eq!(forge.merge(PrNumber::from(8)), Ok(PrMerge::AlreadyMerged));
    let expected_calls = vec![
        argv(&["pr", "view", "7", "--json", "state", "--repo", REPO]),
        argv(&["pr", "merge", "7", "--merge", "--repo", REPO]),
        argv(&["pr", "view", "8", "--json", "state", "--repo", REPO]),
    ];
    assert_eq!(fake.calls(), expected_calls);
}

#[test]
fn a_missing_gh_is_unavailable_and_bad_json_a_backend_error() {
    let empty = tempfile::tempdir().expect("empty directory");
    let missing = GhCommand::from(RepoName::from(REPO)).env("PATH", empty.path());
    assert!(matches!(
        GithubBoard::from(missing).task(IssueNumber::from(2)),
        Err(PortError::Unavailable(_))
    ));
    let fake = FakeGh::new();
    fake.on("issue view *", "not json");
    assert!(matches!(
        fake.board().task(IssueNumber::from(2)),
        Err(PortError::Backend(_))
    ));
}
