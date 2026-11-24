use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::*;
use crate::adapters::notes::{
    LeaseNote, SIGNATURE_TEXT, marked, read_signature, report_marker, signature_marker,
};
use crate::domain::{
    Instance, InstanceNumber, Location, RoleName, Ticket, TicketId, TicketState, TicketType,
};
use crate::manifest::RepoName;
use crate::ports::{
    Author, Board, BranchName, Check, CheckName, CheckState, ClaimOutcome, ClaimRecord, CommitId,
    Forge, NewQuestion, PortError, PrNumber, QUESTION_TITLE_LIMIT, ReviewComment, RuleId,
    ScanningAlert,
};

const REPO: &str = "example/tracker";
const ISSUE_FIELDS: &str = "number,title,body,labels,parent,state,comments,issueType";

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
        "number,title,body,labels,parent,issueType",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn blocked_lists_open_blocked_tasks_with_their_bodies_in_one_call() {
    let fake = FakeGh::new();
    let listed = format!(
        "[{},{}]",
        issue_with_body(9, &["task", "blocked"], &[], "OPEN", "Blocked by: #5, #6"),
        issue(3, &["task", "blocked"], &[], "OPEN")
    );
    fake.on("issue list *", &listed);
    let tickets = fake.board().blocked().expect("blocked");
    let numbers: Vec<TicketId> = tickets.iter().map(|ticket| ticket.id).collect();
    assert_eq!(numbers, [TicketId::from(3), TicketId::from(9)]);
    assert_eq!(tickets[1].state, Some(TicketState::Blocked));
    assert_eq!(tickets[1].body.as_str(), "Blocked by: #5, #6");
    let expected_call = argv(&[
        "issue",
        "list",
        "--label",
        "task",
        "--label",
        "blocked",
        "--state",
        "open",
        "--limit",
        "1000",
        "--json",
        "number,title,body,labels,parent,issueType",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn open_tickets_lists_every_open_task_but_a_done_one_in_one_call() {
    let fake = FakeGh::new();
    let listed = format!(
        "[{},{},{},{},{}]",
        issue(9, &["task", "question"], &[], "OPEN"),
        issue_with_body(3, &["task", "blocked"], &[], "OPEN", "Blocked by: #5"),
        issue(4, &["task", "done"], &[], "OPEN"),
        issue(5, &["task"], &[], "OPEN"),
        issue(6, &["task", "claimed"], &[], "OPEN")
    );
    fake.on("issue list *", &listed);
    let tickets =
        crate::domain::OpenTicket::tickets(fake.board().open_tickets().expect("open tickets"));
    let numbers: Vec<u64> = tickets.iter().map(|ticket| ticket.id.get()).collect();
    assert_eq!(numbers, [3, 5, 6, 9]);
    let states: Vec<Option<TicketState>> = tickets.iter().map(|ticket| ticket.state).collect();
    assert_eq!(
        states,
        [
            Some(TicketState::Blocked),
            None,
            Some(TicketState::Claimed),
            Some(TicketState::Question),
        ]
    );
    assert_eq!(tickets[0].body.as_str(), "Blocked by: #5");
    let expected_call = argv(&[
        "issue",
        "list",
        "--label",
        "task",
        "--state",
        "open",
        "--limit",
        "1000",
        "--json",
        "number,title,body,labels,parent,issueType,createdAt",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn open_tickets_reads_the_priority_label_and_when_each_opened() {
    let fake = FakeGh::new();
    let opened = |text: String, at: &str| {
        text.replace("\"labels\"", &format!("\"createdAt\":\"{at}\",\"labels\""))
    };
    let listed = format!(
        "[{},{},{},{}]",
        opened(
            issue(3, &["task", "priority/1"], &[], "OPEN"),
            "2026-11-12T08:00:00Z"
        ),
        issue(5, &["task", "priority/7"], &[], "OPEN"),
        issue(6, &["task", "priority/2", "priority/3"], &[], "OPEN"),
        issue(7, &["task", "ready"], &[], "OPEN"),
    );
    fake.on("issue list *", &listed);
    let open = fake.board().open_tickets().expect("open tickets");
    let read: Vec<(u64, Result<Option<u8>, String>)> = open
        .iter()
        .map(|open| {
            (
                open.ticket.id.get(),
                open.priority
                    .clone()
                    .map(|priority| priority.map(crate::domain::schedule::Priority::level))
                    .map_err(|ignored| ignored.to_string()),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (3, Ok(Some(1))),
            (
                5,
                Err(
                    "the label `priority/7` is not priority/1, priority/2 or priority/3".to_owned()
                )
            ),
            (
                6,
                Err("it carries more than one priority label (priority/2, priority/3)".to_owned())
            ),
            (7, Ok(None)),
        ]
    );
    assert_eq!(
        open[0].opened_at,
        humantime::parse_rfc3339("2026-11-12T08:00:00Z").ok()
    );
    assert_eq!(open[1].opened_at, None, "no createdAt, no time");
}

#[test]
fn a_ticket_reads_its_issue_type() {
    let fake = FakeGh::new();
    let typed = issue(3, &["task", "ready"], &[], "OPEN").replace(
        "\"labels\"",
        "\"issueType\":{\"name\":\"Bug\",\"id\":\"IT_1\"},\"labels\"",
    );
    fake.on("issue view 3 *", &typed);
    let ticket = fake.board().task(TicketId::from(3)).expect("task");
    assert_eq!(ticket.kind, Some(TicketType::from("Bug")));
    assert_eq!(
        ticket.summary().kind.as_ref().map(TicketType::as_str),
        Some("Bug")
    );
}

#[test]
fn an_untyped_ticket_has_no_type() {
    let fake = FakeGh::new();
    let untyped = issue(3, &["task", "ready"], &[], "OPEN")
        .replace("\"labels\"", "\"issueType\":null,\"labels\"");
    fake.on("issue view 3 *", &untyped);
    let ticket = fake.board().task(TicketId::from(3)).expect("task");
    assert_eq!(ticket.kind, None);
    let absent = FakeGh::new();
    absent.on("issue view 3 *", &issue(3, &["task", "ready"], &[], "OPEN"));
    let ticket = absent.board().task(TicketId::from(3)).expect("task");
    assert_eq!(ticket.kind, None);
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
        kind: None,
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
fn a_closed_issue_says_when_it_closed() {
    use crate::domain::Standing;

    let fake = FakeGh::new();
    let closed = closed_for(8, "COMPLETED").replace(
        "\"labels\"",
        "\"closedAt\":\"2026-11-12T09:30:00Z\",\"labels\"",
    );
    fake.on("issue view 8 *", &closed)
        .on("issue view 3 *", &standing(3, &["task"], "OPEN", "issues"));
    let board = fake.board();
    let done = board.standing_at(TicketId::from(8)).expect("reads");
    assert_eq!(done.standing, Standing::Done);
    assert_eq!(
        done.closed_at,
        humantime::parse_rfc3339("2026-11-12T09:30:00Z").ok()
    );
    let open = board.standing_at(TicketId::from(3)).expect("reads");
    assert_eq!((open.standing, open.closed_at), (Standing::Open, None));
}

#[test]
fn board_opened_at_github() {
    let fake = FakeGh::new();
    let created = |number: u64, kind: &str, state: &str| {
        serde_json::json!({
            "number": number,
            "state": state,
            "url": format!("https://github.com/{REPO}/{kind}/{number}"),
            "createdAt": "2026-11-12T08:00:00Z",
        })
        .to_string()
    };
    fake.on("issue view 8 *", &created(8, "issues", "CLOSED"))
        .on("issue view 4 *", &created(4, "pull", "OPEN"))
        .on("issue view 3 *", &standing(3, &["task"], "OPEN", "issues"));
    let board = fake.board();
    assert_eq!(
        board.opened_at(TicketId::from(8)),
        Ok(humantime::parse_rfc3339("2026-11-12T08:00:00Z").ok()),
        "a closed issue says when it opened"
    );
    assert_eq!(board.opened_at(TicketId::from(3)), Ok(None), "no createdAt");
    assert_eq!(
        board.opened_at(TicketId::from(4)),
        Ok(None),
        "a pull request"
    );
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
            "number,state,stateReason,labels,url,closedAt",
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
    .on("issue edit *", "")
    .on("issue close *", "");
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
    // Done closes each open issue as completed, the one already labelled so too.
    let close = |number: &str| {
        argv(&[
            "issue",
            "close",
            number,
            "--reason",
            "completed",
            "--repo",
            REPO,
        ])
    };
    assert_eq!(
        fake.calls(),
        [view("2"), expected_edit, close("2"), view("4"), close("4")]
    );
}

#[test]
fn github_drop_task_request() {
    let fake = FakeGh::new();
    fake.on("issue view 2 *", &issue(2, &["task", "ready"], &[], "OPEN"))
        .on(
            "issue view 4 *",
            &issue(4, &["task", "done"], &[], "CLOSED"),
        )
        .on("issue close *", "");
    let board = fake.board();
    board.drop_task(TicketId::from(2)).expect("drop");
    board.drop_task(TicketId::from(4)).expect("already closed");
    let close = argv(&[
        "issue",
        "close",
        "2",
        "--reason",
        "not planned",
        "--repo",
        REPO,
    ]);
    // The closed task is read and nothing is sent for it.
    assert_eq!(fake.calls(), [view("2"), close, view("4")]);
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
        "number,title,body,labels,parent,issueType",
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
        {"name":"ci","bucket":"pass","completedAt":"2027-01-15T08:00:00Z"},
        {"name":"lint","bucket":"fail","completedAt":"2027-01-15T08:01:00Z"},
        {"name":"scan","bucket":"pending","completedAt":"0001-01-01T00:00:00Z"},
        {"name":"docs","bucket":"skipping","completedAt":"2027-01-15T08:02:00Z"},
        {"name":"bench","bucket":"cancel","completedAt":"2027-01-15T08:03:00Z"}
    ]"#;
    fake.on("pr checks 7 *", checks).fail_on(
        "pr checks 8 *",
        "no checks reported on the 'feature/v1-export-2' branch",
    );
    let check = |name: &str, state: CheckState, finished_at: Option<&str>| Check {
        name: CheckName::from(name),
        state,
        finished_at: finished_at.map(time),
    };
    let expected_checks = vec![
        check("ci", CheckState::Passed, Some("2027-01-15T08:00:00Z")),
        check("lint", CheckState::Failed, Some("2027-01-15T08:01:00Z")),
        check("scan", CheckState::Pending, None),
        check("docs", CheckState::Passed, Some("2027-01-15T08:02:00Z")),
        check("bench", CheckState::Failed, Some("2027-01-15T08:03:00Z")),
    ];
    let forge = fake.forge();
    assert_eq!(forge.checks(PrNumber::from(7)), Ok(expected_checks));
    assert_eq!(forge.checks(PrNumber::from(8)), Ok(Vec::new()));
    let expected_call = argv(&[
        "pr",
        "checks",
        "7",
        "--json",
        "name,bucket,completedAt",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls()[0], expected_call);
}

#[test]
fn a_branchs_checks_are_the_check_runs_of_the_commit_it_is_at() {
    let fake = FakeGh::new();
    let runs = r#"[{"total_count":6,"check_runs":[
        {"name":"ci","status":"completed","conclusion":"success","completed_at":"2027-01-15T08:00:00Z"},
        {"name":"lint","status":"completed","conclusion":"failure","completed_at":"2027-01-15T08:01:00Z"},
        {"name":"scan","status":"in_progress","conclusion":null,"completed_at":null},
        {"name":"docs","status":"completed","conclusion":"skipped","completed_at":"2027-01-15T08:02:00Z"}
    ]},{"total_count":6,"check_runs":[
        {"name":"bench","status":"completed","conclusion":"cancelled","completed_at":"2027-01-15T08:03:00Z"},
        {"name":"deploy","status":"queued","conclusion":null,"completed_at":null}
    ]}]"#;
    fake.on(
        &format!("api repos/{REPO}/branches/feature/v1"),
        r#"{"name":"feature/v1","commit":{"sha":"abc123","commit":{"committer":{"date":"2027-01-15T07:30:00Z"}}}}"#,
    )
    .on(
        &format!("api repos/{REPO}/commits/abc123/check-runs *"),
        runs,
    );
    let check = |name: &str, state: CheckState, finished_at: Option<&str>| Check {
        name: CheckName::from(name),
        state,
        finished_at: finished_at.map(time),
    };
    let read = fake
        .forge()
        .branch_checks(&BranchName::from("feature/v1"))
        .expect("the branch reads");
    assert_eq!(read.head, Some(CommitId::from("abc123")));
    assert_eq!(read.reached_at, Some(time("2027-01-15T07:30:00Z")));
    assert_eq!(
        read.checks,
        [
            check("ci", CheckState::Passed, Some("2027-01-15T08:00:00Z")),
            check("lint", CheckState::Failed, Some("2027-01-15T08:01:00Z")),
            check("scan", CheckState::Pending, None),
            check("docs", CheckState::Passed, Some("2027-01-15T08:02:00Z")),
            check("bench", CheckState::Failed, Some("2027-01-15T08:03:00Z")),
            check("deploy", CheckState::Pending, None),
        ]
    );
    assert_eq!(
        fake.calls(),
        [
            argv(&["api", &format!("repos/{REPO}/branches/feature/v1")]),
            argv(&[
                "api",
                &format!("repos/{REPO}/commits/abc123/check-runs"),
                "--paginate",
                "--slurp",
            ]),
        ]
    );
}

#[test]
fn a_branch_github_does_not_have_is_no_head_and_one_with_no_run_no_check() {
    let fake = FakeGh::new();
    fake.fail_on(
        &format!("api repos/{REPO}/branches/feature/v2"),
        "gh: Branch not found (HTTP 404)",
    )
    .on(
        &format!("api repos/{REPO}/branches/feature/v1"),
        r#"{"commit":{"sha":"abc123"}}"#,
    )
    .on(
        &format!("api repos/{REPO}/commits/abc123/check-runs *"),
        r#"[{"total_count":0,"check_runs":[]}]"#,
    )
    .fail_on(&format!("api repos/{REPO}/branches/feature/v3"), "HTTP 502");
    let forge = fake.forge();
    let missing = forge
        .branch_checks(&BranchName::from("feature/v2"))
        .expect("no branch is no error");
    assert_eq!((missing.head, missing.checks), (None, Vec::new()));
    let quiet = forge
        .branch_checks(&BranchName::from("feature/v1"))
        .expect("the branch reads");
    assert_eq!(
        (quiet.head, quiet.checks),
        (Some(CommitId::from("abc123")), Vec::new())
    );
    let error = forge
        .branch_checks(&BranchName::from("feature/v3"))
        .expect_err("a failed read");
    assert!(matches!(error, PortError::Backend(_)), "{error:?}");
}

#[test]
fn a_finished_branch_check_with_no_time_is_an_error_naming_the_branch() {
    let fake = FakeGh::new();
    fake.on(
        &format!("api repos/{REPO}/branches/feature/v1"),
        r#"{"commit":{"sha":"abc123"}}"#,
    )
    .on(
        &format!("api repos/{REPO}/commits/abc123/check-runs *"),
        r#"[{"check_runs":[{"name":"ci","status":"completed","conclusion":"failure"}]}]"#,
    );
    let error = fake
        .forge()
        .branch_checks(&BranchName::from("feature/v1"))
        .expect_err("no time");
    assert!(
        matches!(&error, PortError::Backend(message)
            if message.contains("branch feature/v1") && message.contains("check `ci`")),
        "{error:?}"
    );
}

/// A time as the forge prints it.
fn time(text: &str) -> SystemTime {
    humantime::parse_rfc3339(text).expect("a time")
}

#[test]
fn a_finished_check_has_its_time() {
    let fake = FakeGh::new();
    let checks = r#"[
        {"name":"ci","bucket":"pass","completedAt":"2027-01-15T08:00:00Z"},
        {"name":"lint","bucket":"fail","completedAt":"2027-01-15T08:04:30Z"}
    ]"#;
    fake.on("pr checks 7 *", checks);
    let read = fake.forge().checks(PrNumber::from(7)).expect("checks");
    assert_eq!(read[0].finished_at, Some(time("2027-01-15T08:00:00Z")));
    assert_eq!(read[1].finished_at, Some(time("2027-01-15T08:04:30Z")));
}

#[test]
fn a_running_check_has_none() {
    let fake = FakeGh::new();
    let checks = r#"[
        {"name":"ci","bucket":"pending","completedAt":"0001-01-01T00:00:00Z"},
        {"name":"scan","bucket":"pending"}
    ]"#;
    fake.on("pr checks 7 *", checks);
    let read = fake.forge().checks(PrNumber::from(7)).expect("checks");
    assert_eq!(read.len(), 2);
    assert!(read.iter().all(|check| check.finished_at.is_none()));
}

#[test]
fn a_review_comment_has_its_time_and_head() {
    let fake = FakeGh::new();
    let lines = r#"[[
        {"user":{"login":"reviewer"},"body":"Name this.","path":"src/a.rs","line":3,"created_at":"2027-01-15T08:10:00Z","commit_id":"abc123"}
    ]]"#;
    let reviews = r#"[[
        {"user":{"login":"reviewer"},"body":"Two nits.","submitted_at":"2027-01-15T08:05:00Z","commit_id":"def456"}
    ]]"#;
    fake.on(&format!("api repos/{REPO}/pulls/7/comments *"), lines)
        .on(&format!("api repos/{REPO}/pulls/7/reviews *"), reviews);
    let read = fake
        .forge()
        .review_comments(PrNumber::from(7))
        .expect("comments");
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].posted_at, time("2027-01-15T08:05:00Z"));
    assert_eq!(read[0].head, Some(CommitId::from("def456")));
    assert_eq!(read[1].posted_at, time("2027-01-15T08:10:00Z"));
    assert_eq!(read[1].head, Some(CommitId::from("abc123")));
}

/// A `gh pr view` answer for an open pull request whose `mergeable` is `word`.
fn open_pr_json(word: &str) -> String {
    format!(
        r#"{{"state":"OPEN","createdAt":"2027-01-16T08:00:00Z","mergedAt":null,"mergeable":{word}}}"#
    )
}

#[test]
fn github_pull_reads_mergeable() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", &open_pr_json(r#""MERGEABLE""#))
        .on("pr view 8 *", &open_pr_json(r#""CONFLICTING""#));
    let forge = fake.forge();
    let read = |number: u64| forge.pull_request(PrNumber::from(number)).expect("read");
    assert_eq!(read(7).mergeable, Some(true));
    assert_eq!(read(8).mergeable, Some(false));
}

#[test]
fn github_pull_reads_head_and_merged_in_one_call() {
    let fake = FakeGh::new();
    fake.on(
        "pr view 7 *",
        r#"{"state":"MERGED","createdAt":"2027-01-15T08:00:00Z","mergedAt":"2027-01-15T09:30:00Z","headRefOid":"abc123"}"#,
    )
    .on(
        "pr view 8 *",
        r#"{"state":"OPEN","createdAt":"2027-01-16T08:00:00Z","mergedAt":null,"headRefOid":""}"#,
    );
    let forge = fake.forge();
    let merged = forge.pull_request(PrNumber::from(7)).expect("merged");
    assert_eq!(merged.head, Some("abc123".to_owned()));
    assert!(merged.merged);
    let open = forge.pull_request(PrNumber::from(8)).expect("open");
    assert_eq!(open.head, None);
    assert!(!open.merged);
    assert_eq!(fake.calls().len(), 2, "one call for each read");
}

#[test]
fn github_unknown_mergeable_is_none() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", &open_pr_json(r#""UNKNOWN""#))
        .on("pr view 8 *", &open_pr_json(r#""SOMETHING_NEW""#))
        .on("pr view 9 *", &open_pr_json("null"))
        .on(
            "pr view 10 *",
            r#"{"state":"OPEN","createdAt":"2027-01-16T08:00:00Z","mergedAt":null}"#,
        );
    let forge = fake.forge();
    for number in 7..=10 {
        let read = forge.pull_request(PrNumber::from(number)).expect("read");
        assert_eq!(read.mergeable, None, "#{number}");
    }
}

#[test]
fn a_pr_has_its_open_and_merge_times() {
    let fake = FakeGh::new();
    fake.on(
        "pr view 7 *",
        r#"{"state":"MERGED","createdAt":"2027-01-15T08:00:00Z","mergedAt":"2027-01-15T09:30:00Z"}"#,
    )
    .on(
        "pr view 8 *",
        r#"{"state":"OPEN","createdAt":"2027-01-16T08:00:00Z","mergedAt":null}"#,
    )
    .on(
        "pr view 9 *",
        r#"{"state":"CLOSED","createdAt":"2027-01-17T08:00:00Z","mergedAt":null}"#,
    );
    let forge = fake.forge();
    let merged = forge.pull_request(PrNumber::from(7)).expect("merged");
    assert_eq!(merged.opened_at, time("2027-01-15T08:00:00Z"));
    assert_eq!(merged.merged_at, Some(time("2027-01-15T09:30:00Z")));
    let open = forge.pull_request(PrNumber::from(8)).expect("open");
    assert_eq!(open.opened_at, time("2027-01-16T08:00:00Z"));
    assert_eq!(open.merged_at, None);
    let closed = forge.pull_request(PrNumber::from(9)).expect("closed");
    assert_eq!(closed.merged_at, None);
    let expected_call = argv(&[
        "pr",
        "view",
        "7",
        "--json",
        "state,createdAt,mergedAt,mergeable,headRefOid",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls()[0], expected_call);
}

#[test]
fn a_missing_time_is_an_error() {
    let fake = FakeGh::new();
    fake.on(
        "pr checks 7 *",
        r#"[{"name":"ci","bucket":"pass","completedAt":"0001-01-01T00:00:00Z"}]"#,
    )
    .on("pr checks 8 *", r#"[{"name":"ci","bucket":"fail"}]"#)
    .on(
        &format!("api repos/{REPO}/pulls/9/comments *"),
        r#"[[{"user":null,"body":"Late.","path":"src/a.rs","line":1}]]"#,
    )
    .on(&format!("api repos/{REPO}/pulls/9/reviews *"), "[[]]")
    .on("pr view 10 *", r#"{"state":"OPEN","createdAt":null}"#)
    .on(
        "pr view 11 *",
        r#"{"state":"MERGED","createdAt":"2027-01-15T08:00:00Z","mergedAt":null}"#,
    );
    let forge = fake.forge();
    let errors = [
        (7, forge.checks(PrNumber::from(7)).expect_err("zero time")),
        (8, forge.checks(PrNumber::from(8)).expect_err("no time")),
        (
            9,
            forge
                .review_comments(PrNumber::from(9))
                .expect_err("no post time"),
        ),
        (
            10,
            forge
                .pull_request(PrNumber::from(10))
                .expect_err("no opening"),
        ),
        (
            11,
            forge
                .pull_request(PrNumber::from(11))
                .expect_err("no merge time"),
        ),
    ];
    for (number, error) in errors {
        let PortError::Backend(message) = &error else {
            panic!("pull request #{number}: {error:?} is not a backend error");
        };
        assert!(
            message.contains(&format!("pull request #{number}")),
            "{message}"
        );
    }
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
            posted_at: time("2027-01-15T08:00:00Z"),
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Close; two nits.".to_owned(),
            location: None,
            head: Some(CommitId::from("def456")),
            posted_at: time("2027-01-15T08:05:00Z"),
        },
        ReviewComment {
            author: Author::from("reviewer"),
            body: "Name this binding.".to_owned(),
            location: Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(12),
            }),
            head: Some(CommitId::from("abc123")),
            posted_at: time("2027-01-15T08:10:00Z"),
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
fn changed_files_lists_a_rename_under_both_names() {
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
        forge.changed_files(PrNumber::from(7)),
        Ok(crate::domain::ChangedFiles::of(&[
            "README.md",
            "src/export.rs",
            "src/a.rs",
            "src/z/last.rs"
        ]))
    );
    assert_eq!(
        forge.changed_files(PrNumber::from(8)),
        Ok(crate::domain::ChangedFiles::default())
    );
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
        broken.forge().changed_files(PrNumber::from(7)),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn changed_files_are_cut_short_at_githubs_cap() {
    let rows = (0..3000)
        .map(|at| format!(r#"{{"filename":"f{at}.rs"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let fake = FakeGh::new();
    fake.on(
        &format!("api repos/{REPO}/pulls/7/files *"),
        &format!("[[{rows}]]"),
    )
    .on(
        &format!("api repos/{REPO}/pulls/8/files *"),
        r#"[[{"filename":"a.rs"}]]"#,
    );
    let forge = fake.forge();
    let capped = forge
        .changed_files(PrNumber::from(7))
        .expect("the files read");
    assert_eq!(capped.paths.len(), 3000);
    assert!(capped.cut_short);
    let short = forge
        .changed_files(PrNumber::from(8))
        .expect("the files read");
    assert!(!short.cut_short);
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
    // gh prints the id itself through `--jq`: an empty line, or jq's `null`, is no head.
    let fake = FakeGh::new();
    fake.on("pr view 7 *", "abc123\n")
        .on("pr view 8 *", "\n")
        .on("pr view 9 *", "null\n");
    let forge = fake.forge();
    assert_eq!(forge.head(PrNumber::from(7)), Ok(Some("abc123".to_owned())));
    assert_eq!(
        forge.head(PrNumber::from(8)),
        Ok(None),
        "an empty id is none"
    );
    assert_eq!(forge.head(PrNumber::from(9)), Ok(None));
    let expected_call = argv(&[
        "pr",
        "view",
        "7",
        "--json",
        "headRefOid",
        "--jq",
        ".headRefOid",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls()[0], expected_call);
}

const HANDOFF: &str = "<!-- zaf:handoff -->";

/// The pages `gh api --paginate --slurp` prints for a pull request's conversation: one page,
/// its comments numbered 50, 51, ….
fn conversation(bodies: &[&str]) -> String {
    let comments: Vec<serde_json::Value> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| serde_json::json!({ "id": 50 + index, "body": body }))
        .collect();
    serde_json::json!([comments]).to_string()
}

fn conversation_call() -> Vec<String> {
    let endpoint = format!("repos/{REPO}/issues/7/comments");
    argv(&["api", &endpoint, "--paginate", "--slurp"])
}

#[test]
fn a_second_handoff_replaces_the_first() {
    let pr = PrNumber::from(7);
    // The first hand-off finds no comment of its own and posts one.
    let first = FakeGh::new();
    first
        .on("api *", &conversation(&["LGTM"]))
        .on("pr comment *", "");
    first
        .forge()
        .set_handoff(pr, "Ran the tests.")
        .expect("first");
    let posted = format!("{HANDOFF}\nRan the tests.");
    let post = argv(&["pr", "comment", "7", "--body", &posted, "--repo", REPO]);
    assert_eq!(first.calls(), [conversation_call(), post]);

    // The second finds it and edits it in place: no second comment.
    let second = FakeGh::new();
    second
        .on(
            "api *issues/7/comments --paginate*",
            &conversation(&["LGTM", &posted]),
        )
        .on("api *issues/comments/51 *", "{}");
    second
        .forge()
        .set_handoff(pr, "Fixed the CI.")
        .expect("second");
    let field = format!("body={HANDOFF}\nFixed the CI.");
    let endpoint = format!("repos/{REPO}/issues/comments/51");
    let edit = argv(&["api", &endpoint, "--method", "PATCH", "-f", &field]);
    assert_eq!(second.calls(), [conversation_call(), edit]);

    // The same text again changes nothing.
    let third = FakeGh::new();
    third.on(
        "api *",
        &conversation(&[&format!("{HANDOFF}\nFixed the CI.")]),
    );
    third
        .forge()
        .set_handoff(pr, "Fixed the CI.")
        .expect("third");
    assert_eq!(third.calls(), [conversation_call()]);
}

#[test]
fn the_github_handoff_reads_the_latest_marked_comment_and_none_without() {
    let fake = FakeGh::new();
    fake.on(
        "api *issues/7/comments*",
        &conversation(&[
            "LGTM",
            &format!("{HANDOFF}\nold"),
            "thanks",
            &format!("{HANDOFF}\nnew\nover two lines"),
        ]),
    )
    .on(
        "api *issues/8/comments*",
        &conversation(&["LGTM", "mentions <!-- zaf:handoff --> inline"]),
    )
    .on("api *issues/9/comments*", "[]");
    let forge = fake.forge();
    assert_eq!(
        forge.handoff(PrNumber::from(7)),
        Ok(Some("new\nover two lines".to_owned()))
    );
    assert_eq!(forge.handoff(PrNumber::from(8)), Ok(None));
    assert_eq!(forge.handoff(PrNumber::from(9)), Ok(None));
    assert_eq!(fake.calls().len(), 3, "reads only: nothing was written");

    let broken = FakeGh::new();
    broken.fail_on("api *", "HTTP 502");
    assert!(matches!(
        broken.forge().handoff(PrNumber::from(7)),
        Err(PortError::Backend(_))
    ));
    assert!(matches!(
        broken.forge().set_handoff(PrNumber::from(7), "x"),
        Err(PortError::Backend(_))
    ));
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
        "comments": comments
            .iter()
            .map(|text| serde_json::json!({"body": text, "author": {"login": "zaf-bot"}}))
            .collect::<Vec<_>>(),
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
        .on("issue list --label task *", &tasks.to_string())
        .on("api user", r#"{"login":"zaf-bot","id":5}"#);

    let read = fake.board().epics().expect("epics read");
    assert_eq!(read.len(), 1);
    let epic = &read[0];
    assert_eq!(
        epic.signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&body.into()))
    );
    assert!(epic.challenged());
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
fn an_epic_is_planned_when_the_latest_planning_exit_was_posted() {
    let planning = |number: u32| Instance {
        role: RoleName::from("planning"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(number),
    };
    let comment =
        |text: String, created: &str| serde_json::json!({"body": text, "createdAt": created});
    let epics = serde_json::json!([{
        "number": 1,
        "title": "tickets",
        "body": "Goal: one.",
        "labels": [{"name": "epic"}],
        "comments": [
            comment(format!("<!-- zaf:exit {} done -->\nfirst", planning(1)), "2026-11-01T10:00:00Z"),
            comment(format!("<!-- zaf:exit {} done -->\nlast", planning(2)), "2026-11-02T11:30:00Z"),
            // Not a planning exit: another role's exit and a plain comment.
            comment(format!("<!-- zaf:exit {} done -->", instance(4)), "2026-11-03T00:00:00Z"),
            comment("planning exit, said in words".to_owned(), "2026-11-04T00:00:00Z"),
        ],
    }]);
    let fake = FakeGh::new();
    fake.on("issue list --label epic *", &epics.to_string())
        .on("issue list --label task *", "[]");

    let read = fake.board().epics().expect("epics read");
    assert_eq!(
        read[0].planned_at,
        Some(humantime::parse_rfc3339("2026-11-02T11:30:00Z").expect("a time"))
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

#[test]
fn github_file_epic_opens_an_epic_labelled_issue() {
    use crate::domain::NewEpic;

    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/12\n"),
    );
    let epic = NewEpic {
        title: "tickets".into(),
        body: "Release: 0.5.0\nGoal: one ticket shape.".into(),
    };
    assert_eq!(fake.board().file_epic(&epic), Ok(TicketId::from(12)));
    let expected_call = argv(&[
        "issue",
        "create",
        "--title",
        "tickets",
        "--body",
        "Release: 0.5.0\nGoal: one ticket shape.",
        "--label",
        "epic",
        "--repo",
        REPO,
    ]);
    assert_eq!(fake.calls(), [expected_call]);
}

#[test]
fn github_filed_epic_is_listed_by_epics() {
    use crate::domain::NewEpic;

    let body = "Release: 0.5.0\nGoal: one ticket shape.";
    let fake = FakeGh::new();
    fake.on(
        "issue create *",
        &format!("https://github.com/{REPO}/issues/12\n"),
    );
    let epics = serde_json::json!([{
        "number": 12,
        "title": "tickets",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": [],
    }]);
    fake.on("issue list --label epic *", &epics.to_string())
        .on("issue list --label task *", "[]");
    let epic = NewEpic {
        title: "tickets".into(),
        body: body.into(),
    };
    let board = fake.board();
    let id = board.file_epic(&epic).expect("epic filed");
    let listed = board.epics().expect("epics read");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].body.as_str(), body);
    assert!(listed[0].tasks.is_empty());
}

#[test]
fn github_file_epic_maps_a_gh_failure() {
    use crate::domain::NewEpic;

    let fake = FakeGh::new();
    fake.fail_on("issue create *", "HTTP 502: bad gateway");
    let epic = NewEpic {
        title: "tickets".into(),
        body: "Goal: one ticket shape.".into(),
    };
    let error = fake.board().file_epic(&epic).expect_err("gh failed");
    assert!(
        matches!(&error, PortError::Backend(message) if message.contains("bad gateway")),
        "{error:?}"
    );
}

#[test]
fn github_epic_task_carries_body_and_url() {
    let epics = serde_json::json!([{
        "number": 1, "title": "tickets", "body": "Goal.",
        "labels": [{"name": "epic"}], "comments": [],
    }]);
    let tasks = serde_json::json!([
        {"number": 2, "title": "t", "body": "Goal: the first",
         "url": "https://github.com/example/tracker/issues/2",
         "labels": [{"name": "task"}, {"name": "ready"}],
         "parent": {"number": 1}, "state": "OPEN"},
    ]);
    let fake = FakeGh::new();
    fake.on("issue list --label epic *", &epics.to_string())
        .on("issue list --label task *", &tasks.to_string());

    let read = fake.board().epics().expect("epics read");
    let task = &read[0].tasks[0];
    assert_eq!(task.body, "Goal: the first".into());
    assert_eq!(
        task.url,
        "https://github.com/example/tracker/issues/2".into()
    );
    let task_list = &fake.calls()[1];
    assert!(
        task_list
            .iter()
            .any(|word| word.contains("body") && word.contains("url")),
        "the task list asks for the body and the link: {task_list:?}"
    );
}

const WORKFLOWS: &str = ".github/workflows";

fn github_entry(name: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "path": format!("{WORKFLOWS}/{name}"), "type": kind })
}

#[test]
fn ci_filter_reads_github_workflows() {
    use crate::domain::{PullRequestFilter, RepoPath, WorkflowFilter};
    use crate::ports::CiFilter;

    let fake = FakeGh::new();
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS} --method GET -f ref=feature/v1"),
        &serde_json::json!([
            github_entry("b.yaml", "file"),
            github_entry("a.yml", "file"),
            github_entry("c.yml", "file"),
            github_entry("README.md", "file"),
            github_entry("nested.yml", "dir"),
        ])
        .to_string(),
    );
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS}/a.yml * -f ref=feature/v1"),
        "on: pull_request\njobs: {}\n",
    );
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS}/b.yaml * -f ref=feature/v1"),
        "on: push\njobs: {}\n",
    );
    fake.fail_on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS}/c.yml *"),
        "gh: Forbidden (HTTP 403)",
    );
    let found = GithubCiFilter::from(fake.gh())
        .workflows(&BranchName::from("feature/v1"))
        .expect("the workflows read");
    let at = |name: &str, filter| WorkflowFilter {
        path: RepoPath::from(format!("{WORKFLOWS}/{name}").as_str()),
        filter,
    };
    assert_eq!(
        found,
        [
            at("a.yml", PullRequestFilter::AnyChange),
            at("b.yaml", PullRequestFilter::AnyChange),
            at("c.yml", PullRequestFilter::Unsure),
        ]
    );
    // Every read names the base's ref, never a pull request's head.
    for call in fake.calls() {
        assert_eq!(call.last().map(String::as_str), Some("ref=feature/v1"));
    }
}

#[test]
fn ci_filter_has_no_workflows_in_a_github_base_with_no_folder_but_fails_on_a_missing_base() {
    use crate::ports::CiFilter;

    let fake = FakeGh::new();
    fake.fail_on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS} * ref=main"),
        "gh: Not Found (HTTP 404)",
    );
    fake.on(&format!("api repos/{REPO}/branches/main"), "{}");
    fake.fail_on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS} * ref=gone"),
        "gh: Not Found (HTTP 404)",
    );
    fake.fail_on(
        &format!("api repos/{REPO}/branches/gone"),
        "gh: Not Found (HTTP 404)",
    );
    let filter = GithubCiFilter::from(fake.gh());
    assert_eq!(filter.workflows(&BranchName::from("main")), Ok(Vec::new()));
    assert!(matches!(
        filter.workflows(&BranchName::from("gone")),
        Err(PortError::Backend(_))
    ));
    assert!(matches!(
        filter.workflows(&BranchName::from("-main")),
        Err(PortError::Rejected(_))
    ));
}

/// An issue as the batched GraphQL read returns it, its comments carrying `total` in all.
fn batch_issue(number: u64, comments: &[String], total: usize) -> serde_json::Value {
    let comments: Vec<_> = comments
        .iter()
        .enumerate()
        .map(|(index, body)| {
            let url = format!(
                "https://github.com/{REPO}/issues/{number}#issuecomment-{}",
                100 + index
            );
            serde_json::json!({ "body": body, "url": url, "createdAt": "2027-01-15T08:00:00Z" })
        })
        .collect();
    serde_json::json!({
        "number": number,
        "title": format!("task {number}"),
        "body": "Goal: export the list",
        "state": "OPEN",
        "labels": { "totalCount": 2, "nodes": [{ "name": "task" }, { "name": "claimed" }] },
        "parent": { "number": 1 },
        "issueType": null,
        "comments": { "totalCount": total, "nodes": comments },
    })
}

/// What `gh api graphql` prints for a batch: each issue under the alias `i<number>`.
fn batch_answer(issues: Vec<(u64, serde_json::Value)>) -> String {
    let repository: serde_json::Map<String, serde_json::Value> = issues
        .into_iter()
        .map(|(number, issue)| (format!("i{number}"), issue))
        .collect();
    serde_json::json!({ "data": { "repository": repository } }).to_string()
}

const CLAIMED: [u64; 5] = [3, 4, 5, 6, 7];

/// The comments of claimed task `number`: held by instance `number % 3 + 1`.
fn claimed_comments(number: u64) -> Vec<String> {
    let holder = u32::try_from(number % 3).expect("small") + 1;
    vec![
        claim_line(holder, 1_700_000_000 + number),
        heartbeat_line(holder, 1_800_000_000 + number),
    ]
}

fn claimed_list() -> String {
    let rows: Vec<_> = CLAIMED
        .iter()
        .map(|number| serde_json::json!({ "number": number }))
        .collect();
    serde_json::Value::Array(rows).to_string()
}

fn batch_of(numbers: &[u64]) -> Vec<(u64, serde_json::Value)> {
    numbers
        .iter()
        .map(|number| {
            let comments = claimed_comments(*number);
            (*number, batch_issue(*number, &comments, comments.len()))
        })
        .collect()
}

#[test]
fn github_claims_read_in_one_call() {
    let fake = FakeGh::new();
    fake.on("issue list --label claimed *", &claimed_list())
        .on("api graphql *", &batch_answer(batch_of(&CLAIMED)));
    let claims = fake.board().claims().expect("claims");
    assert_eq!(claims.len(), 5);
    let calls = fake.calls();
    assert_eq!(calls.len(), 2, "the list, then one read of the five");
    assert_eq!(calls[1][..2], argv(&["api", "graphql"]));
    assert!(
        calls
            .iter()
            .all(|call| call[..2] != argv(&["issue", "view"])),
        "no issue is read alone"
    );
}

#[test]
fn github_claims_unchanged() {
    let batched = FakeGh::new();
    batched
        .on("issue list --label claimed *", &claimed_list())
        .on("api graphql *", &batch_answer(batch_of(&CLAIMED)));
    // The per-issue reads: each task alone in the list, so `claims` reads it with `issue view`.
    let mut per_issue = Vec::new();
    for number in CLAIMED {
        let single = FakeGh::new();
        single
            .on(
                "issue list --label claimed *",
                &format!(r#"[{{"number":{number}}}]"#),
            )
            .on(
                &format!("issue view {number} *"),
                &issue(
                    number,
                    &["task", "claimed"],
                    &claimed_comments(number),
                    "OPEN",
                ),
            );
        per_issue.extend(single.board().claims().expect("claims"));
    }
    let claims = batched.board().claims().expect("claims");
    assert_eq!(claims.len(), 5);
    assert_eq!(claims, per_issue);
}

#[test]
fn github_batch_missing_number_errors() {
    let fake = FakeGh::new();
    let mut answer: serde_json::Value =
        serde_json::from_str(&batch_answer(batch_of(&[3, 4, 6, 7]))).expect("json");
    answer["data"]["repository"]["i5"] = serde_json::Value::Null;
    fake.on("issue list --label claimed *", &claimed_list())
        .on("api graphql *", &answer.to_string());
    let Err(PortError::NotFound(message)) = fake.board().claims() else {
        panic!("a number the answer lacks is an error");
    };
    assert!(message.contains("#5"), "{message}");
}

#[test]
fn github_batch_cut_comments_are_read_whole() {
    let fake = FakeGh::new();
    let comments = claimed_comments(3);
    let mut issues = batch_of(&[4]);
    issues.insert(0, (3, batch_issue(3, &comments[..1], 150)));
    fake.on(
        "issue list --label claimed *",
        r#"[{"number":3},{"number":4}]"#,
    )
    .on("api graphql *", &batch_answer(issues))
    .on(
        "issue view 3 *",
        &issue(3, &["task", "claimed"], &comments, "OPEN"),
    );
    let claims = fake.board().claims().expect("claims");
    assert_eq!(claims.len(), 2);
    assert_eq!(fake.calls().len(), 3);
}

#[test]
fn fake_gh_concurrent_calls_get_distinct_numbers() {
    let fake = FakeGh::new();
    fake.on("issue *", "{}");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gh");
    for round in 0..5 {
        let before = fake.calls().len();
        let children: Vec<_> = (0..8)
            .map(|call| {
                std::process::Command::new(&fixture)
                    .args(["issue", "view", &format!("{round}-{call}")])
                    .env("FAKE_GH_DIR", fake.scene.path())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("fake gh")
            })
            .collect();
        for child in children {
            let ended = child.wait_with_output().expect("fake gh ends");
            assert!(
                ended.status.success(),
                "fake gh: {:?}: {}",
                ended.status,
                String::from_utf8_lossy(&ended.stderr)
            );
        }
        let calls = fake.calls();
        assert_eq!(calls.len(), before + 8, "two calls wrote one file");
        let mut seen: Vec<_> = calls.iter().map(|call| call[2].clone()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), calls.len(), "a call's record was overwritten");
    }
}

/// Whether a check is expected on a pull request from `branch` that changes a doc, for the one
/// workflow `text` at the base of a fake GitHub.
fn github_ci_expected(text: &str, branch: &str) -> bool {
    use crate::domain::{RepoPath, checks_expected};
    use crate::ports::CiFilter;

    let fake = FakeGh::new();
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS} --method GET -f ref=main"),
        &serde_json::json!([github_entry("a.yml", "file")]).to_string(),
    );
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS}/a.yml * -f ref=main"),
        text,
    );
    let workflows = GithubCiFilter::from(fake.gh())
        .workflows(&BranchName::from("main"))
        .expect("the workflows read");
    checks_expected(
        &workflows,
        &[RepoPath::from("docs/a.md")],
        false,
        Some(branch),
    )
}

#[test]
fn ci_filter_push_no_branches_expected_github() {
    assert!(github_ci_expected("on: push\n", "feature/x"));
    assert!(github_ci_expected("on:\n  push:\n", "feature/x"));
}

#[test]
fn ci_filter_push_matching_branch_expected_github() {
    let text = "on:\n  push:\n    branches: ['feature/**']\n";
    assert!(github_ci_expected(text, "feature/x"));
}

#[test]
fn ci_filter_push_other_branch_not_expected_github() {
    let text = "on:\n  push:\n    branches: [main]\n";
    assert!(!github_ci_expected(text, "feature/x"));
}

#[test]
fn ci_filter_pull_request_target_expected_github() {
    assert!(github_ci_expected("on: pull_request_target\n", "feature/x"));
}

#[test]
fn ci_filter_unreadable_unsure_github() {
    use crate::domain::PullRequestFilter;
    use crate::ports::CiFilter;

    assert!(github_ci_expected(
        "on:\n  push:\n    branches-ignore: [wip]\n",
        "feature/x"
    ));
    let fake = FakeGh::new();
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS} --method GET -f ref=main"),
        &serde_json::json!([github_entry("a.yml", "file")]).to_string(),
    );
    fake.on(
        &format!("api repos/{REPO}/contents/{WORKFLOWS}/a.yml * -f ref=main"),
        "on: [unclosed",
    );
    let found = GithubCiFilter::from(fake.gh())
        .workflows(&BranchName::from("main"))
        .expect("the workflows read");
    assert_eq!(found[0].filter, PullRequestFilter::Unsure);
}

mod forge_writes;
mod logins;
mod projects;
