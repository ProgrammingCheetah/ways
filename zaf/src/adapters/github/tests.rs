use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::notes::{
    LeaseNote, SIGNATURE_TEXT, marked, read_signature, report_marker, signature_marker,
};
use super::*;
use crate::domain::{Instance, InstanceNumber, Location, RoleName, Ticket, TicketId, TicketState};
use crate::manifest::RepoName;
use crate::ports::{
    Author, Board, Check, CheckName, CheckState, ClaimOutcome, ClaimRecord, Forge, NewQuestion,
    PortError, PrNumber, QUESTION_TITLE_LIMIT, ReviewComment, RuleId, ScanningAlert,
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
        epic: TicketId::from(1),
        number: InstanceNumber::from(number),
    }
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// An issue as `gh issue view --json` prints it; comments get ids 100, 101, ….
fn issue(number: u64, labels: &[&str], comments: &[String], state: &str) -> String {
    issue_with_body(number, labels, comments, state, "Goal: export the list")
}

/// A question ticket as `gh issue view --json` prints it, raised on `task`.
fn question_issue(number: u64, comments: &[String], state: &str, task: u64) -> String {
    let body = NewQuestion {
        on: TicketId::from(task),
        text: String::new(),
        door: false,
    }
    .body();
    issue_with_body(number, &["question"], comments, state, &body)
}

fn issue_with_body(
    number: u64,
    labels: &[&str],
    comments: &[String],
    state: &str,
    body: &str,
) -> String {
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
        "body": body,
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
    let numbers: Vec<TicketId> = tickets.iter().map(|ticket| ticket.id).collect();
    assert_eq!(numbers, [TicketId::from(2), TicketId::from(4)]);
    assert_eq!(tickets[0].epic, Some(TicketId::from(1)));
    assert_eq!(tickets[0].state, Some(TicketState::Ready));
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
        id: TicketId::from(3),
        epic: Some(TicketId::from(1)),
        title: "task 3".into(),
        body: "Goal: export the list".into(),
        state: Some(TicketState::Blocked),
        url: format!("https://github.com/{REPO}/issues/3").into(),
    };
    assert_eq!(fake.board().task(TicketId::from(3)), Ok(expected_ticket));
    let missing_issue_error = PortError::NotFound(
        "gh issue view: GraphQL: Could not resolve to an Issue with the number of 9.".to_owned(),
    );
    assert_eq!(
        fake.board().task(TicketId::from(9)),
        Err(missing_issue_error)
    );
    let failed_call_error = PortError::Backend("gh issue view: HTTP 502: bad gateway".to_owned());
    assert_eq!(
        fake.board().task(TicketId::from(10)),
        Err(failed_call_error)
    );
    assert_eq!(fake.calls(), [view("3"), view("9"), view("10")]);
}

/// An issue as `gh issue view --json number,state,labels,url` prints it.
fn standing(number: u64, labels: &[&str], state: &str, kind: &str) -> String {
    let labels: Vec<_> = labels
        .iter()
        .map(|name| serde_json::json!({ "name": name }))
        .collect();
    serde_json::json!({
        "number": number,
        "state": state,
        "stateReason": if state == "CLOSED" { serde_json::json!("COMPLETED") } else { serde_json::Value::Null },
        "labels": labels,
        "url": format!("https://github.com/{REPO}/{kind}/{number}"),
    })
    .to_string()
}

/// An issue closed for `reason`, read only for where it stands.
fn closed_for(number: u64, reason: &str) -> String {
    serde_json::json!({
        "number": number,
        "state": "CLOSED",
        "stateReason": reason,
        "labels": [{ "name": "task" }],
        "url": format!("https://github.com/{REPO}/issues/{number}"),
    })
    .to_string()
}

#[test]
fn issue_state_tells_open_done_closed_and_missing_apart() {
    use crate::domain::Standing;

    let fake = FakeGh::new();
    fake.on(
        "issue view 3 *",
        &standing(3, &["task", "blocked"], "OPEN", "issues"),
    )
    .on(
        "issue view 4 *",
        &standing(4, &["task", "done"], "CLOSED", "issues"),
    )
    .on(
        "issue view 5 *",
        &standing(5, &["task", "done"], "OPEN", "issues"),
    )
    .on("issue view 6 *", &closed_for(6, "NOT_PLANNED"))
    .on("issue view 8 *", &closed_for(8, "COMPLETED"))
    .on("issue view 11 *", &closed_for(11, "DUPLICATE"))
    .on("issue view 7 *", &standing(7, &[], "OPEN", "pull"))
    .fail_on(
        "issue view 9 *",
        "GraphQL: Could not resolve to an Issue with the number of 9.",
    )
    .fail_on(
        "issue view 12 *",
        "GraphQL: Could not resolve to an issue or pull request with the number of 12. (repository.issue)",
    )
    .fail_on("issue view 10 *", "HTTP 502: bad gateway");
    let board = fake.board();
    let state = |number: u64| board.standing(TicketId::from(number));
    assert_eq!(
        state(12),
        Ok(Standing::Missing),
        "gh 2.102 says `an issue or pull request`"
    );

    assert_eq!(state(3), Ok(Standing::Open));
    assert_eq!(state(4), Ok(Standing::Done));
    assert_eq!(state(5), Ok(Standing::Done));
    assert_eq!(state(6), Ok(Standing::ClosedNotPlanned));
    assert_eq!(
        state(8),
        Ok(Standing::Done),
        "closed as completed is done without the label"
    );
    assert_eq!(state(11), Ok(Standing::ClosedNotPlanned));
    assert_eq!(
        state(7),
        Ok(Standing::Missing),
        "a pull request is no issue"
    );
    assert_eq!(state(9), Ok(Standing::Missing));
    assert_eq!(
        state(10),
        Err(PortError::Backend(
            "gh issue view: HTTP 502: bad gateway".to_owned()
        ))
    );
    let standing_view = |number: &str| {
        argv(&[
            "issue",
            "view",
            number,
            "--json",
            "number,state,stateReason,labels,url",
            "--repo",
            REPO,
        ])
    };
    assert_eq!(
        fake.calls(),
        ["12", "3", "4", "5", "6", "8", "11", "7", "9", "10"].map(standing_view)
    );
}

#[test]
fn every_call_on_a_missing_issue_is_not_found() {
    let fake = FakeGh::new();
    let gone = "GraphQL: Could not resolve to an Issue with the number of 9.";
    fake.fail_on("issue *", gone);
    let board = fake.board();
    let missing = TicketId::from(9);
    let results = [
        board.task(missing).map(drop),
        board.claim(missing, &instance(1)).map(drop),
        board.heartbeat(missing, &instance(1)),
        board.release(missing, &instance(1)),
        board.set_state(missing, TicketState::Done),
        board.comment(missing, "Blocked."),
        board.report(missing, &instance(1), "working"),
        board.record_ruling(missing, "Title only.").map(drop),
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
        board.claim(TicketId::from(2), &instance(1)),
        Ok(ClaimOutcome::Claimed)
    );
    assert!(matches!(
        board.claim(TicketId::from(3), &instance(1)),
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
        board.claim(TicketId::from(2), &instance(1)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    assert!(matches!(
        board.claim(TicketId::from(2), &instance(2)),
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
        .heartbeat(TicketId::from(2), &instance(1))
        .expect("heartbeat");
    assert!(matches!(
        board.heartbeat(TicketId::from(2), &instance(2)),
        Err(PortError::Conflict(_))
    ));
    let expected_claims = vec![ClaimRecord {
        task: TicketId::from(2),
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
fn release_returns_a_held_task_to_ready_and_comments() {
    let fake = FakeGh::new();
    let comments = [claim_line(1, 1_800_000_000)];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &comments, "OPEN"),
    )
    .on("issue edit *", "")
    .on("issue comment *", "");
    fake.board()
        .release(TicketId::from(2), &instance(1))
        .expect("release");
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
    let calls = fake.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], view("2"));
    assert_eq!(calls[1], expected_edit);
    assert_eq!(calls[2][..4], argv(&["issue", "comment", "2", "--body"]));
    assert!(
        calls[2][4].starts_with("released from programmer instance 1 on #1 at "),
        "{:?}",
        calls[2]
    );
}

#[test]
fn a_release_keeps_question_and_done_and_adds_no_ready() {
    let fake = FakeGh::new();
    let comments = [claim_line(1, 1_800_000_000)];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed", "question"], &comments, "OPEN"),
    )
    .on(
        "issue view 3 *",
        &issue(3, &["task", "claimed", "done"], &comments, "OPEN"),
    )
    .on("issue edit *", "")
    .on("issue comment *", "");
    let board = fake.board();
    board
        .release(TicketId::from(2), &instance(1))
        .expect("release");
    board
        .release(TicketId::from(3), &instance(1))
        .expect("release");
    let removes_claimed_only = |number: &str| {
        argv(&[
            "issue",
            "edit",
            number,
            "--remove-label",
            "claimed",
            "--repo",
            REPO,
        ])
    };
    let calls = fake.calls();
    assert_eq!(calls[1], removes_claimed_only("2"));
    assert_eq!(calls[4], removes_claimed_only("3"));
}

#[test]
fn a_release_by_a_non_holder_is_quiet_and_changes_nothing() {
    let fake = FakeGh::new();
    let comments = [claim_line(1, 1_800_000_000)];
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &comments, "OPEN"),
    )
    .on("issue view 4 *", &issue(4, &["task", "ready"], &[], "OPEN"));
    let board = fake.board();
    board
        .release(TicketId::from(2), &instance(2))
        .expect("another instance holds it");
    board
        .release(TicketId::from(4), &instance(1))
        .expect("nobody holds it");
    assert_eq!(fake.calls(), [view("2"), view("4")]);
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
        .set_state(TicketId::from(2), TicketState::Done)
        .expect("label");
    board
        .set_state(TicketId::from(4), TicketState::Done)
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
        .comment(TicketId::from(2), "Blocked on the schema.")
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
        .report(TicketId::from(2), &instance(1), "working")
        .expect("first report");
    board
        .report(TicketId::from(3), &instance(1), "done")
        .expect("second report");
    board
        .report(TicketId::from(4), &instance(1), "done")
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
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: true,
    };
    assert_eq!(fake.board().open_question(&question), Ok(TicketId::from(7)));
    let expected_call = argv(&[
        "issue",
        "create",
        "--title",
        "Which columns does the export carry?",
        "--body",
        "Raised on #2.\n\nWhich columns does the export carry?",
        "--label",
        "question",
        "--label",
        "door",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

/// A question of `words` words, `word` repeated, raised on #2.
fn wordy_question(word: &str, words: usize) -> NewQuestion {
    NewQuestion {
        on: TicketId::from(2),
        text: vec![word; words].join(" "),
        door: false,
    }
}

/// The `--title` and `--body` the one `issue create` call carried.
fn created_title_and_body(fake: &FakeGh) -> (String, String) {
    let calls = fake.calls();
    let [call] = calls.as_slice() else {
        panic!("one call expected, got {calls:?}");
    };
    let value = |flag: &str| {
        let at = call.iter().position(|arg| arg == flag).expect("flag");
        call[at + 1].clone()
    };
    (value("--title"), value("--body"))
}

#[test]
fn long_question_is_filed() {
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/7\n"),
    );
    let question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns? ".repeat(70).trim().to_owned(),
        door: false,
    };
    assert!(question.text.chars().count() >= 1000);
    assert_eq!(fake.board().open_question(&question), Ok(TicketId::from(7)));
    let (title, _) = created_title_and_body(&fake);
    assert!(title.chars().count() <= 200, "GitHub refuses a long title");
}

#[test]
fn long_question_title_is_cut_at_a_word() {
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/7\n"),
    );
    let question = wordy_question("columns", 150);
    fake.board().open_question(&question).expect("filed");
    let (title, _) = created_title_and_body(&fake);
    assert!(title.chars().count() <= 200);
    assert!(title.chars().count() > 180, "the cut keeps what fits");
    assert!(question.text.starts_with(&title));
    assert!(
        question.text[title.len()..].starts_with(' '),
        "the cut falls between words, not inside `columns`"
    );
    assert!(!title.ends_with(' '));
}

#[test]
fn short_question_title_is_unchanged() {
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/7\n"),
    );
    let exact = NewQuestion {
        on: TicketId::from(2),
        text: "a".repeat(QUESTION_TITLE_LIMIT),
        door: false,
    };
    fake.board().open_question(&exact).expect("filed");
    let (title, _) = created_title_and_body(&fake);
    assert_eq!(title, exact.text);
    assert_eq!(
        wordy_question("x", 3).title(),
        "x x x",
        "a short question is its own title"
    );
}

#[test]
fn long_question_body_holds_the_whole_text() {
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/7\n"),
    );
    let question = wordy_question("columns", 150);
    fake.board().open_question(&question).expect("filed");
    let (_, body) = created_title_and_body(&fake);
    assert!(body.starts_with("Raised on #2."));
    assert!(body.ends_with(&question.text));
    let listed = serde_json::json!([{
        "number": 7,
        "title": question.title(),
        "body": body,
        "labels": [{ "name": "question" }],
    }]);
    let listing = FakeGh::new();
    listing.on("issue list *", &listed.to_string());
    let raised = listing
        .board()
        .questions_on(TicketId::from(2))
        .expect("questions on the task");
    assert_eq!(raised.len(), 1, "questions_on still finds it");
    assert_eq!(NewQuestion::text_of(&raised[0].0), question.text);
    assert!(question.is_asked_by(&raised[0].0));
}

#[test]
fn questions_lists_open_question_issues_and_skips_tasks_waiting_on_one() {
    let fake = FakeGh::new();
    let listed = format!(
        "[{},{},{}]",
        issue(9, &["question", "door"], &[], "OPEN"),
        issue(2, &["task", "question"], &[], "OPEN"),
        issue(7, &["question"], &[], "OPEN")
    );
    fake.on("issue list *", &listed);
    let tickets = fake.board().questions().expect("questions");
    let numbers: Vec<TicketId> = tickets.iter().map(|ticket| ticket.id).collect();
    assert_eq!(numbers, [TicketId::from(7), TicketId::from(9)]);
    assert_eq!(tickets[0].state, Some(TicketState::Question));
    let expected_call = argv(&[
        "issue",
        "list",
        "--label",
        "question",
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
fn questions_on_lists_ruled_and_open_questions_raised_on_the_task_oldest_first() {
    let fake = FakeGh::new();
    let ruled = [
        "chatter".to_owned(),
        "<!-- zaf:ruling -->\nTitle and author.\n".to_owned(),
    ];
    let listed = format!(
        "[{},{},{},{},{}]",
        question_issue(9, &[], "OPEN", 2),
        question_issue(8, &[], "OPEN", 3),
        issue(2, &["task", "question"], &[], "OPEN"),
        question_issue(7, &ruled, "CLOSED", 2),
        issue(5, &["question"], &[], "OPEN"),
    );
    fake.on("issue list *", &listed);

    let raised = fake
        .board()
        .questions_on(TicketId::from(2))
        .expect("questions on the task");

    let seen: Vec<(TicketId, Option<&str>)> = raised
        .iter()
        .map(|(ticket, ruling)| (ticket.id, ruling.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            (TicketId::from(7), Some("Title and author.")),
            (TicketId::from(9), None)
        ]
    );
    let expected_call = argv(&[
        "issue",
        "list",
        "--label",
        "question",
        "--state",
        "all",
        "--limit",
        "1000",
        "--json",
        ISSUE_FIELDS,
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn a_listed_question_is_the_one_that_opened_it() {
    let fake = FakeGh::new();
    let question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let listed = serde_json::json!([{
        "number": 7,
        "title": question.text,
        "body": question.body(),
        "labels": [{ "name": "question" }],
    }]);
    fake.on("issue list *", &listed.to_string());
    let tickets = fake.board().questions().expect("questions");
    assert!(question.is_asked_by(&tickets[0]));
    let elsewhere = NewQuestion {
        on: TicketId::from(3),
        ..question
    };
    assert!(!elsewhere.is_asked_by(&tickets[0]));
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
        .record_ruling(TicketId::from(7), "Title and author.")
        .expect("ruling");
    board
        .record_ruling(TicketId::from(8), "Title and author.")
        .expect("the same ruling is quiet");
    assert!(matches!(
        board.record_ruling(TicketId::from(8), "Title only."),
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

#[test]
fn a_ruling_returns_the_waiting_task_to_ready() {
    let fake = FakeGh::new();
    fake.on("issue view 7 *", &question_issue(7, &[], "OPEN", 2))
        .on(
            "issue view 2 *",
            &issue(2, &["task", "question"], &[], "OPEN"),
        )
        .on("issue comment *", "")
        .on("issue close *", "")
        .on("issue edit *", "");
    let returned = fake
        .board()
        .record_ruling(TicketId::from(7), "Title and author.");
    assert_eq!(returned, Ok(Some(TicketId::from(2))));
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
        view("2"),
        argv(&[
            "issue",
            "edit",
            "2",
            "--add-label",
            "ready",
            "--remove-label",
            "question",
            "--repo",
            REPO,
        ]),
    ];
    assert_eq!(fake.calls(), expected_calls);
}

#[test]
fn a_second_identical_ruling_changes_nothing_and_a_done_task_keeps_its_labels() {
    let ruled = ["<!-- zaf:ruling -->\nTitle and author.".to_owned()];
    let fake = FakeGh::new();
    fake.on("issue view 8 *", &question_issue(8, &ruled, "CLOSED", 2))
        .on("issue view 2 *", &issue(2, &["task", "ready"], &[], "OPEN"))
        .on("issue view 9 *", &question_issue(9, &[], "OPEN", 4))
        .on(
            "issue view 4 *",
            &issue(4, &["task", "done", "question"], &[], "OPEN"),
        )
        .on("issue comment *", "")
        .on("issue close *", "");
    let board = fake.board();
    assert_eq!(
        board.record_ruling(TicketId::from(8), "Title and author."),
        Ok(None),
        "the task already returned"
    );
    assert_eq!(
        board.record_ruling(TicketId::from(9), "Title and author."),
        Ok(None),
        "a done task is left alone"
    );
    let calls = fake.calls();
    assert!(
        calls.iter().all(|call| call[1] != "edit"),
        "no label changes: {calls:?}"
    );
    assert_eq!(calls[..2], [view("8"), view("2")]);
}

fn edits(fake: &FakeGh) -> Vec<Vec<String>> {
    fake.calls()
        .into_iter()
        .filter(|call| call[1] == "edit")
        .collect()
}

#[test]
fn a_question_replaces_claimed_or_ready_with_question() {
    let fake = FakeGh::new();
    fake.on(
        "issue view 2 *",
        &issue(2, &["task", "claimed"], &[], "OPEN"),
    )
    .on("issue view 4 *", &issue(4, &["task", "ready"], &[], "OPEN"))
    .on("issue edit *", "");
    let board = fake.board();
    board
        .set_state(TicketId::from(2), TicketState::Question)
        .expect("claimed task");
    board
        .set_state(TicketId::from(4), TicketState::Question)
        .expect("ready task");
    let edit = |number: &str, removed: &str| {
        argv(&[
            "issue",
            "edit",
            number,
            "--add-label",
            "question",
            "--remove-label",
            removed,
            "--repo",
            REPO,
        ])
    };
    assert_eq!(edits(&fake), [edit("2", "claimed"), edit("4", "ready")]);
}

#[test]
fn a_question_on_a_task_that_is_neither_claimed_nor_ready_leaves_its_labels_alone() {
    let fake = FakeGh::new();
    fake.on("issue view 2 *", &issue(2, &["task"], &[], "OPEN"))
        .on("issue view 4 *", &issue(4, &["task", "done"], &[], "OPEN"))
        .on("issue edit *", "");
    let board = fake.board();
    board
        .set_state(TicketId::from(2), TicketState::Question)
        .expect("unlabelled task");
    board
        .set_state(TicketId::from(4), TicketState::Question)
        .expect("done task");
    assert_eq!(edits(&fake), Vec::<Vec<String>>::new());
}

#[test]
fn a_question_on_an_epic_never_adds_a_state_label() {
    let fake = FakeGh::new();
    fake.on("issue view 3 *", &issue(3, &["epic"], &[], "OPEN"))
        .on("issue view 5 *", &issue(5, &["epic", "ready"], &[], "OPEN"))
        .on("issue edit *", "");
    let board = fake.board();
    board
        .set_state(TicketId::from(3), TicketState::Question)
        .expect("epic");
    board
        .set_state(TicketId::from(5), TicketState::Question)
        .expect("epic carrying a state label");
    assert_eq!(edits(&fake), Vec::<Vec<String>>::new());
}

#[test]
fn a_ruling_on_a_question_raised_on_an_epic_edits_no_labels() {
    let fake = FakeGh::new();
    fake.on("issue view 7 *", &question_issue(7, &[], "OPEN", 3))
        .on(
            "issue view 3 *",
            &issue(3, &["epic", "question"], &[], "OPEN"),
        )
        .on("issue comment *", "")
        .on("issue close *", "")
        .on("issue edit *", "");
    let returned = fake
        .board()
        .record_ruling(TicketId::from(7), "Title and author.");
    assert_eq!(returned, Ok(None), "an epic is never returned to ready");
    assert_eq!(edits(&fake), Vec::<Vec<String>>::new());
}

/// Zuri's 2026-11-01 sequence on zurfur-design: questions on epic #71 and on tasks
/// #72 and #73 that were never ready, then their rulings #78 to #80.
#[test]
fn questions_and_rulings_on_an_epic_and_unsigned_tasks_change_no_label() {
    let fake = FakeGh::new();
    fake.on("issue list *", "[]")
        .on(
            "issue create *",
            &format!("https://github.com/{REPO}/issues/78\n"),
        )
        .on("issue view 71 *", &issue(71, &["epic"], &[], "OPEN"))
        .on("issue view 72 *", &issue(72, &["task"], &[], "OPEN"))
        .on("issue view 73 *", &issue(73, &["task"], &[], "OPEN"))
        .on("issue view 78 *", &question_issue(78, &[], "OPEN", 71))
        .on("issue view 79 *", &question_issue(79, &[], "OPEN", 72))
        .on("issue view 80 *", &question_issue(80, &[], "OPEN", 73))
        .on("issue comment *", "")
        .on("issue close *", "")
        .on("issue edit *", "");
    let board = fake.board();
    for on in [71, 72, 73] {
        let command = NewQuestion {
            on: TicketId::from(on),
            text: "Which columns does the export carry?".to_owned(),
            door: false,
        };
        crate::app::board::question(&board, &command).expect("question");
    }
    for question in [78, 79, 80] {
        assert_eq!(
            board.record_ruling(TicketId::from(question), "Title and author."),
            Ok(None),
            "ruling #{question} returns nothing to ready"
        );
    }
    assert_eq!(edits(&fake), Vec::<Vec<String>>::new());
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
        {"user":{"login":"reviewer"},"body":"Name this binding.","path":"src/export.rs","line":12,"created_at":"2027-01-15T08:10:00Z","commit_id":"abc123"}
    ],[
        {"user":null,"body":"Outdated.","path":"src/old.rs","line":null,"created_at":"2027-01-15T08:00:00Z"}
    ]]"#;
    let reviews = r#"[[
        {"user":{"login":"reviewer"},"body":"Close; two nits.","submitted_at":"2027-01-15T08:05:00Z","commit_id":"def456"},
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
            head: None,
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Close; two nits.".to_owned(),
            location: None,
            head: Some("def456".to_owned()),
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Name this binding.".to_owned(),
            location: Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(12),
            }),
            head: Some("abc123".to_owned()),
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
fn last_changed_file_reads_the_diff() {
    let fake = FakeGh::new();
    let files = r#"[[
        {"filename":"README.md","status":"modified"},
        {"filename":"src/export.rs","status":"added"}
    ],[
        {"filename":"src/z/last.rs","status":"renamed","previous_filename":"src/a.rs"}
    ]]"#;
    let endpoint = format!("repos/{REPO}/pulls/7/files");
    fake.on(&format!("api {endpoint} *"), files)
        .on(&format!("api repos/{REPO}/pulls/8/files *"), "[[]]");
    let forge = fake.forge();
    assert_eq!(
        forge.last_changed_file(PrNumber::from(7)),
        Ok(Some("src/z/last.rs".to_owned()))
    );
    assert_eq!(forge.last_changed_file(PrNumber::from(8)), Ok(None));
    let expected_calls = vec![
        argv(&["api", &endpoint, "--paginate", "--slurp"]),
        argv(&[
            "api",
            &format!("repos/{REPO}/pulls/8/files"),
            "--paginate",
            "--slurp",
        ]),
    ];
    assert_eq!(fake.calls(), expected_calls);

    let broken = FakeGh::new();
    broken.fail_on("api *", "HTTP 404: Not Found");
    assert!(matches!(
        broken.forge().last_changed_file(PrNumber::from(7)),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn merged_is_true_only_for_a_merged_pr_and_changes_nothing() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", r#"{"state":"OPEN"}"#)
        .on("pr view 8 *", r#"{"state":"MERGED"}"#)
        .on("pr view 9 *", r#"{"state":"CLOSED"}"#);
    let forge = fake.forge();
    assert_eq!(forge.merged(PrNumber::from(7)), Ok(false));
    assert_eq!(forge.merged(PrNumber::from(8)), Ok(true));
    assert_eq!(forge.merged(PrNumber::from(9)), Ok(false));
    let expected_calls = vec![
        argv(&["pr", "view", "7", "--json", "state", "--repo", REPO]),
        argv(&["pr", "view", "8", "--json", "state", "--repo", REPO]),
        argv(&["pr", "view", "9", "--json", "state", "--repo", REPO]),
    ];
    assert_eq!(fake.calls(), expected_calls);

    let broken = FakeGh::new();
    broken.fail_on("pr view *", "HTTP 502");
    assert!(matches!(
        broken.forge().merged(PrNumber::from(7)),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn head_reads_the_pull_requests_head_commit() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", r#"{"headRefOid":"abc123"}"#)
        .on("pr view 8 *", r#"{"headRefOid":""}"#)
        .on("pr view 9 *", "{}");
    let forge = fake.forge();
    assert_eq!(forge.head(PrNumber::from(7)), Ok(Some("abc123".to_owned())));
    assert_eq!(
        forge.head(PrNumber::from(8)),
        Ok(None),
        "an empty id is none"
    );
    assert_eq!(forge.head(PrNumber::from(9)), Ok(None));
    let expected_call = argv(&["pr", "view", "7", "--json", "headRefOid", "--repo", REPO]);
    assert_eq!(fake.calls()[0], expected_call);
}

#[test]
fn a_missing_gh_is_unavailable_and_bad_json_a_backend_error() {
    let empty = tempfile::tempdir().expect("empty directory");
    let missing = GhCommand::from(RepoName::from(REPO)).env("PATH", empty.path());
    assert!(matches!(
        GithubBoard::from(missing).task(TicketId::from(2)),
        Err(PortError::Unavailable(_))
    ));
    let fake = FakeGh::new();
    fake.on("issue view *", "not json");
    assert!(matches!(
        fake.board().task(TicketId::from(2)),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn releases_list_stable_tags_and_download_by_pattern() {
    use crate::ports::Releases;

    let fake = FakeGh::new();
    fake.on(
        "release list *",
        r#"[{"tagName":"zaf-v0.2.1","isDraft":false,"isPrerelease":false},
            {"tagName":"zaf-v0.3.0","isDraft":true,"isPrerelease":false},
            {"tagName":"zaf-v0.3.0-rc.1","isDraft":false,"isPrerelease":true},
            {"tagName":"zaf-v0.2.0","isDraft":false,"isPrerelease":false}]"#,
    )
    .on("release download *", "");
    let releases = super::GithubReleases::from(fake.gh());

    assert_eq!(
        releases.tags().expect("lists"),
        ["zaf-v0.2.1", "zaf-v0.2.0"]
    );
    let dir = Path::new("/tmp/zaf-update");
    releases
        .download(
            "zaf-v0.2.1",
            &["a.tar.gz".to_owned(), "a.tar.gz.sha256".to_owned()],
            dir,
        )
        .expect("downloads");
    assert_eq!(
        fake.calls()[1],
        argv(&[
            "release",
            "download",
            "zaf-v0.2.1",
            "--dir",
            "/tmp/zaf-update",
            "--clobber",
            "--pattern",
            "a.tar.gz",
            "--pattern",
            "a.tar.gz.sha256",
            "--repo",
            REPO,
        ])
    );
}

#[test]
fn epics_read_the_latest_signature_and_a_challengers_report() {
    use crate::domain::{BodyHash, EpicStage, Signature, Standing};

    let body = "Goal: one ticket shape.";
    let old = Signature::of(&"Goal: an older body.".into(), at(1_000));
    let current = Signature::of(&body.into(), at(2_000));
    let challenger = Instance {
        role: RoleName::from("challenger"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(9),
    };
    let comments = [
        marked(&signature_marker(&old), SIGNATURE_TEXT),
        marked(&report_marker(&instance(4)), "a programmer's report"),
        marked(&signature_marker(&current), SIGNATURE_TEXT),
        marked(&report_marker(&challenger), "every task is exact"),
    ];
    let epics = serde_json::json!([{
        "number": 1,
        "title": "tickets",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": comments.iter().map(|text| serde_json::json!({"body": text})).collect::<Vec<_>>(),
    }]);
    let tasks = serde_json::json!([
        {"number": 2, "title": "t", "labels": [{"name": "task"}, {"name": "ready"}],
         "parent": {"number": 1}, "state": "OPEN"},
        {"number": 3, "title": "t", "labels": [{"name": "task"}],
         "parent": {"number": 1}, "state": "CLOSED", "stateReason": "NOT_PLANNED"},
        {"number": 4, "title": "t", "labels": [{"name": "task"}],
         "parent": {"number": 1}, "state": "CLOSED", "stateReason": "COMPLETED"},
    ]);
    let fake = FakeGh::new();
    fake.on("issue list --label epic *", &epics.to_string())
        .on("issue list --label task *", &tasks.to_string());

    let read = fake.board().epics().expect("epics read");
    assert_eq!(read.len(), 1);
    let epic = &read[0];
    assert_eq!(
        epic.signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&body.into()))
    );
    assert!(epic.challenged);
    let standings: Vec<Standing> = epic.tasks.iter().map(|task| task.standing).collect();
    assert_eq!(
        standings,
        [Standing::Open, Standing::ClosedNotPlanned, Standing::Done]
    );
    assert_eq!(epic.stage(), EpicStage::Challenged);
    let task_list = &fake.calls()[1];
    assert!(
        task_list.iter().any(|word| word.contains("stateReason")),
        "the task list reads how each task closed: {task_list:?}"
    );
}

#[test]
fn a_signature_is_one_marked_comment_on_the_epic() {
    use crate::domain::Signature;

    let fake = FakeGh::new();
    fake.on(
        "issue comment *",
        "https://github.com/example/tracker/issues/1#issuecomment-1",
    );
    let signature = Signature::of(&"Goal: g".into(), at(1_000));
    fake.board()
        .sign(TicketId::from(1), &signature)
        .expect("signs");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let comment = calls[0]
        .iter()
        .find(|word| word.starts_with("<!-- zaf:signature "))
        .expect("the comment body");
    assert_eq!(
        read_signature(comment),
        Some(Signature {
            hash: signature.hash.clone(),
            at: at(1_000),
        })
    );
}
