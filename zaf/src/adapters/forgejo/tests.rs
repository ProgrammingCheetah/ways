use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::rows::parse_time;
use super::*;
use crate::adapters::notes::{LeaseNote, marked, report_marker, signature_marker};
use crate::domain::{
    BodyHash, Instance, InstanceNumber, NewTask, RoleName, Signature, Standing, TicketId,
    TicketState, TicketType,
};
use crate::manifest::{EnvVarName, ForgeUrl, RepoName};
use crate::ports::{Board, ClaimOutcome, NewQuestion, PortError};

const URL: &str = "http://forgejo.test";
const REPO: &str = "example/tracker";
const TOKEN: &str = "secret-token-0123";
const API: &str = "http://forgejo.test/api/v1/repos/example/tracker";

/// The id each label the tests use has in the fake repository.
const LABEL_IDS: &[(&str, u64)] = &[
    ("task", 1),
    ("epic", 2),
    ("question", 3),
    ("door", 4),
    ("state/ready", 11),
    ("state/claimed", 12),
    ("state/blocked", 13),
    ("state/question", 14),
    ("state/done", 15),
    ("type/Task", 21),
    ("type/Bug", 22),
    ("not planned", 31),
    ("group:core", 41),
];

fn label_id(name: &str) -> u64 {
    LABEL_IDS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, id)| *id)
        .unwrap_or_else(|| panic!("no test id for label `{name}`"))
}

fn label(name: &str) -> Value {
    json!({ "id": label_id(name), "name": name, "exclusive": name.contains('/') })
}

fn labels(names: &[&str]) -> Value {
    Value::Array(names.iter().map(|name| label(name)).collect())
}

/// A scene for the fake `curl` in `tests/fixtures/forgejo/`: its rules, responses and calls.
struct FakeCurl {
    scene: tempfile::TempDir,
    responses: Cell<usize>,
}

/// One recorded call: its argv and what curl read on stdin.
struct Call {
    argv: Vec<String>,
    config: String,
}

impl Call {
    /// `METHOD URL`, as the rules match it.
    fn asked(&self) -> String {
        let method = self
            .argv
            .iter()
            .position(|arg| arg == "--request")
            .map_or("GET", |at| self.argv[at + 1].as_str());
        format!("{method} {}", self.argv.last().expect("a URL"))
    }
}

impl FakeCurl {
    /// A scene whose instance pages 50 items at a time.
    fn new() -> Self {
        let scene = tempfile::tempdir().expect("scene directory");
        fs::write(scene.path().join("rules"), "").expect("rules file");
        let fake = Self {
            scene,
            responses: Cell::new(0),
        };
        fake.on(
            "GET */api/v1/settings/api",
            200,
            &json!({ "max_response_items": 50 }),
        );
        fake
    }

    /// Answers calls whose `METHOD URL` matches the glob `pattern` with `status` and `body`.
    fn on(&self, pattern: &str, status: u16, body: &Value) -> &Self {
        let text = if body.is_null() {
            String::new()
        } else {
            body.to_string()
        };
        self.rule(pattern, &status.to_string(), &text)
    }

    /// Answers the first call matching `pattern` with `status` and `body`; later ones fall to
    /// the rules after it.
    fn once(&self, pattern: &str, status: u16, body: &Value) -> &Self {
        self.rule(pattern, &format!("{status}!"), &body.to_string())
    }

    /// Fails calls matching `pattern` the way curl fails when the instance does not answer.
    fn unreachable(&self, pattern: &str) -> &Self {
        self.rule(
            pattern,
            "curl:7",
            "curl: (7) Failed to connect to forgejo.test port 80: Connection refused",
        )
    }

    fn rule(&self, pattern: &str, answer: &str, text: &str) -> &Self {
        let index = self.responses.get();
        self.responses.set(index + 1);
        let response = format!("response-{index}");
        fs::write(self.scene.path().join(&response), text).expect("response file");
        let mut rules = OpenOptions::new()
            .append(true)
            .open(self.scene.path().join("rules"))
            .expect("rules file");
        writeln!(rules, "{pattern}\t{answer}\t{response}").expect("rule");
        self
    }

    fn api_with(&self, token: Option<&str>) -> ForgejoApi {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forgejo");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            std::iter::once(fixtures).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        ForgejoApi::new(
            ForgeUrl::from(URL),
            RepoName::from(REPO),
            EnvVarName::from("FORGEJO_TOKEN"),
            token.map(ForgejoToken::from),
        )
        .env("PATH", search_path)
        .env("FAKE_CURL_DIR", self.scene.path())
    }

    fn api(&self) -> ForgejoApi {
        self.api_with(Some(TOKEN))
    }

    fn board(&self) -> ForgejoBoard {
        ForgejoBoard::from(self.api())
    }

    /// Every call, in order.
    fn calls(&self) -> Vec<Call> {
        let Ok(entries) = fs::read_dir(self.scene.path().join("calls")) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries
            .map(|entry| entry.expect("call").path())
            .filter(|path| path.extension().is_none())
            .collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                let recorded = fs::read_to_string(file).expect("recorded call");
                let config = fs::read_to_string(file.with_extension("config")).unwrap_or_default();
                Call {
                    argv: recorded.split_terminator('\0').map(str::to_owned).collect(),
                    config,
                }
            })
            .collect()
    }

    /// Every call's `METHOD URL` but the page-size read, in order.
    fn asked(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(Call::asked)
            .filter(|asked| !asked.ends_with("/settings/api"))
            .collect()
    }

    /// The JSON bodies sent with `METHOD URL` calls matching `asked`, read back from curl's
    /// config the way curl reads it.
    fn bodies(&self, asked: &str) -> Vec<Value> {
        self.calls()
            .iter()
            .filter(|call| call.asked() == asked)
            .filter_map(|call| sent_body(&call.config))
            .collect()
    }
}

/// The body a curl config carries, unquoted as curl unquotes it.
fn sent_body(config: &str) -> Option<Value> {
    let line = config
        .lines()
        .find_map(|line| line.strip_prefix("data-raw = \""))?;
    let quoted = line.strip_suffix('"')?;
    let mut text = String::new();
    let mut chars = quoted.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            text.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => text.push('\n'),
            Some('r') => text.push('\r'),
            Some('t') => text.push('\t'),
            Some('v') => text.push('\u{0b}'),
            Some(other) => text.push(other),
            None => {}
        }
    }
    serde_json::from_str(&text).ok()
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

/// An issue as Forgejo prints it.
fn issue(number: u64, names: &[&str], state: &str) -> Value {
    issue_with_body(number, names, state, "Goal: export the list")
}

fn issue_with_body(number: u64, names: &[&str], state: &str, body: &str) -> Value {
    json!({
        "id": 1000 + number,
        "number": number,
        "title": format!("task {number}"),
        "body": body,
        "labels": labels(names),
        "state": state,
        "html_url": format!("{URL}/{REPO}/issues/{number}"),
        "pull_request": null,
    })
}

/// Comments as Forgejo prints them, with ids 100, 101, ….
fn comments(bodies: &[String]) -> Value {
    Value::Array(
        bodies
            .iter()
            .enumerate()
            .map(|(index, body)| {
                json!({
                    "id": 100 + index,
                    "body": body,
                    "created_at": "2026-11-12T03:00:00-07:00",
                })
            })
            .collect(),
    )
}

fn claim_line(holder: u32, seconds: u64) -> String {
    LeaseNote::Claim {
        instance: instance(holder),
        at: at(seconds),
    }
    .to_string()
}

/// Answers `issues/<number>/blocks` with `epic` as the one epic it blocks, or none.
fn blocks(fake: &FakeCurl, number: u64, epic: Option<u64>) {
    let blocked: Vec<Value> = epic
        .map(|epic| issue(epic, &["epic"], "open"))
        .into_iter()
        .collect();
    fake.on(
        &format!("GET {API}/issues/{number}/blocks?*"),
        200,
        &Value::Array(blocked),
    );
}

// The door.

#[test]
fn the_token_and_the_body_go_to_curl_on_stdin_never_on_its_command_line() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("POST {API}/issues/3/comments"),
        201,
        &json!({ "id": 9 }),
    );
    fake.board()
        .comment(TicketId::from(3), "a \"quoted\" line\\ and\nanother")
        .expect("the comment posts");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert!(
        call.argv.iter().all(|arg| !arg.contains(TOKEN)),
        "the token is on argv: {:?}",
        call.argv
    );
    assert!(
        call.config
            .contains(&format!("header = \"Authorization: token {TOKEN}\"")),
        "{}",
        call.config
    );
    assert_eq!(
        sent_body(&call.config),
        Some(json!({ "body": "a \"quoted\" line\\ and\nanother" }))
    );
    for flag in ["--config", "--max-time", "--connect-timeout", "--globoff"] {
        assert!(call.argv.iter().any(|arg| arg == flag), "no {flag}");
    }
}

#[test]
fn a_404_is_not_found_and_another_refusal_a_backend_error_with_forgejos_message() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/404"),
        404,
        &json!({ "message": "The target couldn't be found.", "errors": [] }),
    );
    fake.on(
        &format!("GET {API}/issues/403"),
        403,
        &json!({ "message": "token does not have at least one of required scope(s): [read:issue]" }),
    );
    let board = fake.board();
    let missing = board.task(TicketId::from(404));
    assert!(
        matches!(&missing, Err(PortError::NotFound(message)) if message.contains("couldn't be found")),
        "got {missing:?}"
    );
    let refused = board.task(TicketId::from(403)).expect_err("refused");
    let PortError::Backend(message) = refused else {
        panic!("got {refused:?}");
    };
    assert!(message.contains("403"), "{message}");
    assert!(message.contains("required scope"), "{message}");
}

#[test]
fn an_instance_that_does_not_answer_is_unavailable() {
    let fake = FakeCurl::new();
    fake.unreachable(&format!("GET {API}/issues/3"));
    let read = fake.board().task(TicketId::from(3));
    assert!(
        matches!(&read, Err(PortError::Unavailable(message)) if message.contains("Connection refused")),
        "got {read:?}"
    );
}

#[test]
fn a_missing_curl_is_unavailable() {
    let empty = tempfile::tempdir().expect("an empty PATH");
    let api = ForgejoApi::new(
        ForgeUrl::from(URL),
        RepoName::from(REPO),
        EnvVarName::from("FORGEJO_TOKEN"),
        Some(ForgejoToken::from(TOKEN)),
    )
    .env("PATH", empty.path());
    let read = ForgejoBoard::from(api).task(TicketId::from(3));
    assert!(
        matches!(&read, Err(PortError::Unavailable(message)) if message.contains("cannot run curl")),
        "got {read:?}"
    );
}

#[test]
fn a_repository_call_without_a_token_is_unavailable_and_runs_nothing() {
    let fake = FakeCurl::new();
    let board = ForgejoBoard::from(fake.api_with(None));
    let read = board.task(TicketId::from(3));
    assert!(
        matches!(&read, Err(PortError::Unavailable(message)) if message.contains("`FORGEJO_TOKEN` is not set")),
        "got {read:?}"
    );
    assert!(fake.calls().is_empty());
}

#[test]
fn a_reply_that_is_not_the_expected_json_is_a_backend_error() {
    let fake = FakeCurl::new();
    fake.rule(
        &format!("GET {API}/issues/3"),
        "200",
        "<html>a proxy</html>",
    );
    let read = fake.board().task(TicketId::from(3));
    assert!(
        matches!(&read, Err(PortError::Backend(message)) if message.contains("unreadable JSON")),
        "got {read:?}"
    );
}

#[test]
fn a_list_reads_pages_until_one_comes_back_short() {
    let fake = FakeCurl::new();
    // Rules match in order: this page size wins over the scene's 50.
    let scene_rules = fake.scene.path().join("rules");
    fs::write(&scene_rules, "").expect("rules reset");
    fake.on(
        "GET */api/v1/settings/api",
        200,
        &json!({ "max_response_items": 2 }),
    );
    let page = |items: &[&str]| -> Value {
        Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, name)| json!({ "id": index, "name": name }))
                .collect(),
        )
    };
    fake.on(
        &format!("GET {API}/labels?page=1&limit=2"),
        200,
        &page(&["a", "b"]),
    );
    fake.on(
        &format!("GET {API}/labels?page=2&limit=2"),
        200,
        &page(&["c"]),
    );
    let names: Vec<String> = fake
        .api()
        .list::<Value>("labels")
        .expect("the list reads")
        .iter()
        .map(|label| label["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert_eq!(fake.asked().len(), 2, "two pages, then stop");
}

#[test]
fn a_query_value_is_percent_encoded() {
    assert_eq!(query_value("state/ready"), "state%2Fready");
    assert_eq!(query_value("not planned"), "not%20planned");
    assert_eq!(query_value("group:core-1.x_~"), "group%3Acore-1.x_~");
}

#[test]
fn a_token_never_prints() {
    let token = ForgejoToken::from(TOKEN);
    assert!(!format!("{token:?}").contains(TOKEN));
}

#[test]
fn times_read_in_utc_and_with_the_instances_offset() {
    let utc = humantime::parse_rfc3339("2026-11-12T10:35:26Z").expect("a time");
    assert_eq!(parse_time("2026-11-12T10:35:26Z"), Some(utc));
    assert_eq!(parse_time("2026-11-12T03:35:26-07:00"), Some(utc));
    assert_eq!(parse_time("2026-11-12T12:35:26+02:00"), Some(utc));
    assert_eq!(parse_time("yesterday"), None);
}

// The board.

#[test]
fn ready_lists_open_ready_tasks_in_number_order_with_their_epics() {
    let fake = FakeCurl::new();
    let listed = json!([
        issue(4, &["task", "state/ready", "type/Bug"], "open"),
        issue(2, &["task", "state/ready"], "open"),
        // What Forgejo lists when a filter label is missing from the repository.
        issue(3, &["task", "state/blocked"], "open"),
    ]);
    fake.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=*"),
        200,
        &listed,
    );
    blocks(&fake, 2, Some(1));
    blocks(&fake, 4, None);
    let tickets = fake.board().ready().expect("ready");
    let numbers: Vec<TicketId> = tickets.iter().map(|ticket| ticket.id).collect();
    assert_eq!(numbers, [TicketId::from(2), TicketId::from(4)]);
    assert_eq!(tickets[0].epic, Some(TicketId::from(1)));
    assert_eq!(tickets[1].epic, None);
    assert_eq!(tickets[0].state, Some(TicketState::Ready));
    assert_eq!(tickets[0].url.as_str(), format!("{URL}/{REPO}/issues/2"));
    assert_eq!(tickets[1].kind, Some(TicketType::from("Bug")));
    assert_eq!(
        fake.asked()[0],
        format!(
            "GET {API}/issues?state=open&type=issues&labels=task,state%2Fready&page=1&limit=50"
        )
    );
}

#[test]
fn a_task_reads_its_state_type_epic_and_link() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/7"),
        200,
        &issue(
            7,
            &["task", "type/Task", "state/blocked", "group:core"],
            "open",
        ),
    );
    blocks(&fake, 7, Some(5));
    let ticket = fake
        .board()
        .task(TicketId::from(7))
        .expect("the task reads");
    assert_eq!(ticket.epic, Some(TicketId::from(5)));
    assert_eq!(ticket.state, Some(TicketState::Blocked));
    assert_eq!(ticket.kind, Some(TicketType::from("Task")));
    assert_eq!(ticket.body.as_str(), "Goal: export the list");
}

#[test]
fn a_new_claim_comments_then_puts_on_the_claimed_label() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/ready"], "open"),
    );
    fake.on(
        &format!("POST {API}/issues/3/comments"),
        201,
        &json!({ "id": 9 }),
    );
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["task", "state/ready", "state/claimed"]),
    );
    fake.on(
        &format!("POST {API}/issues/3/labels"),
        200,
        &labels(&["task", "state/claimed"]),
    );
    let outcome = fake.board().claim(TicketId::from(3), &instance(1));
    assert_eq!(outcome, Ok(ClaimOutcome::Claimed));
    assert_eq!(
        fake.asked(),
        [
            format!("GET {API}/issues/3"),
            format!("POST {API}/issues/3/comments"),
            format!("GET {API}/labels?page=1&limit=50"),
            format!("POST {API}/issues/3/labels"),
        ]
    );
    let comment = &fake.bodies(&format!("POST {API}/issues/3/comments"))[0];
    let line = comment["body"].as_str().expect("a body");
    assert!(
        line.starts_with("claimed by programmer instance 1 on #1 at "),
        "{line}"
    );
    assert_eq!(
        fake.bodies(&format!("POST {API}/issues/3/labels")),
        [json!({ "labels": [label_id("state/claimed")] })]
    );
}

#[test]
fn a_repeated_claim_is_quiet_for_the_holder_and_a_conflict_for_others() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/claimed"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/3/comments"),
        200,
        &comments(&[claim_line(1, 1_800_000_000)]),
    );
    let board = fake.board();
    assert_eq!(
        board.claim(TicketId::from(3), &instance(1)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    let other = board.claim(TicketId::from(3), &instance(2));
    assert!(
        matches!(&other, Err(PortError::Conflict(message)) if message.contains("is held by programmer instance 1")),
        "got {other:?}"
    );
    assert!(
        fake.asked().iter().all(|asked| asked.starts_with("GET")),
        "nothing written"
    );
}

#[test]
fn a_task_that_is_not_ready_is_not_claimed() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/blocked"], "open"),
    );
    let claimed = fake.board().claim(TicketId::from(3), &instance(1));
    assert!(
        matches!(claimed, Err(PortError::Rejected(_))),
        "got {claimed:?}"
    );
}

#[test]
fn claims_read_the_latest_claim_and_its_holders_heartbeats() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=state%2Fclaimed*"),
        200,
        &json!([issue(3, &["task", "state/claimed"], "open")]),
    );
    let heartbeat = LeaseNote::Heartbeat {
        instance: instance(1),
        at: at(1_800_000_600),
    }
    .to_string();
    fake.on(
        &format!("GET {API}/issues/3/comments"),
        200,
        &comments(&[claim_line(1, 1_800_000_000), heartbeat]),
    );
    let claims = fake.board().claims().expect("claims read");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].task, TicketId::from(3));
    assert_eq!(claims[0].instance, instance(1));
    assert_eq!(claims[0].heartbeat, at(1_800_000_600));
}

#[test]
fn release_returns_a_held_task_to_ready_and_comments() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/claimed"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/3/comments"),
        200,
        &comments(&[claim_line(1, 1_800_000_000)]),
    );
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["task", "state/ready", "state/claimed"]),
    );
    fake.on(
        &format!("POST {API}/issues/3/labels"),
        200,
        &labels(&["task", "state/ready"]),
    );
    fake.on(
        &format!("POST {API}/issues/3/comments"),
        201,
        &json!({ "id": 9 }),
    );
    fake.board()
        .release(TicketId::from(3), &instance(1))
        .expect("the release");
    assert_eq!(
        fake.bodies(&format!("POST {API}/issues/3/labels")),
        [json!({ "labels": [label_id("state/ready")] })]
    );
    let comment = &fake.bodies(&format!("POST {API}/issues/3/comments"))[0];
    assert!(
        comment["body"]
            .as_str()
            .is_some_and(|line| line.starts_with("released from programmer instance 1 on #1 at ")),
        "{comment}"
    );
}

#[test]
fn a_release_by_a_non_holder_changes_nothing() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/claimed"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/3/comments"),
        200,
        &comments(&[claim_line(1, 1_800_000_000)]),
    );
    fake.board()
        .release(TicketId::from(3), &instance(2))
        .expect("quiet");
    assert!(fake.asked().iter().all(|asked| asked.starts_with("GET")));
}

#[test]
fn a_state_left_on_beside_the_new_one_is_taken_off() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/ready"], "open"),
    );
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["state/ready", "state/blocked"]),
    );
    // A `state/ready` made by hand, not exclusive: Forgejo keeps it beside the new state.
    fake.on(
        &format!("POST {API}/issues/3/labels"),
        200,
        &labels(&["task", "state/ready", "state/blocked"]),
    );
    fake.on(
        &format!("DELETE {API}/issues/3/labels/*"),
        204,
        &Value::Null,
    );
    fake.board()
        .set_state(TicketId::from(3), TicketState::Blocked)
        .expect("the state is set");
    assert_eq!(
        fake.asked().last().map(String::as_str),
        Some(format!("DELETE {API}/issues/3/labels/{}", label_id("state/ready")).as_str())
    );
}

#[test]
fn standing_reads_open_done_dropped_closed_and_missing() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/1"),
        200,
        &issue(1, &["task", "state/ready"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/2"),
        200,
        &issue(2, &["task", "state/done"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "not planned"], "closed"),
    );
    fake.on(
        &format!("GET {API}/issues/4"),
        200,
        &issue(4, &["task"], "closed"),
    );
    fake.on(
        &format!("GET {API}/issues/5"),
        404,
        &json!({ "message": "The target couldn't be found." }),
    );
    let mut pull = issue(6, &[], "open");
    pull["pull_request"] = json!({ "merged": false });
    fake.on(&format!("GET {API}/issues/6"), 200, &pull);
    let board = fake.board();
    let standing = |number: u64| board.standing(TicketId::from(number));
    assert_eq!(standing(1), Ok(Standing::Open));
    assert_eq!(standing(2), Ok(Standing::Done));
    assert_eq!(standing(3), Ok(Standing::ClosedNotPlanned));
    assert_eq!(standing(4), Ok(Standing::Done));
    assert_eq!(standing(5), Ok(Standing::Missing));
    assert_eq!(standing(6), Ok(Standing::Missing));
}

#[test]
fn a_filed_task_creates_the_labels_it_lacks_and_its_epic_depends_on_it() {
    let fake = FakeCurl::new();
    // The repository has `task` and the state; the type and the group are created.
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["task", "state/blocked"]),
    );
    fake.on(&format!("POST {API}/labels"), 201, &label("type/Bug"));
    fake.on(&format!("POST {API}/issues"), 201, &issue(9, &[], "open"));
    fake.on(
        &format!("POST {API}/issues/5/dependencies"),
        201,
        &issue(5, &[], "open"),
    );
    let filed = fake
        .board()
        .file_task(&NewTask {
            epic: TicketId::from(5),
            title: "export".into(),
            body: "Goal: export".into(),
            state: TicketState::Blocked,
            group: Some("core".into()),
            kind: TicketType::from("Bug"),
        })
        .expect("the task is filed");
    assert_eq!(filed, TicketId::from(9));
    let created: Vec<String> = fake
        .bodies(&format!("POST {API}/labels"))
        .iter()
        .map(|body| format!("{} {}", body["name"], body["exclusive"]))
        .collect();
    assert_eq!(created, ["\"type/Bug\" true", "\"group:core\" false"]);
    let opened = &fake.bodies(&format!("POST {API}/issues"))[0];
    assert_eq!(opened["title"], "export");
    assert_eq!(opened["body"], "Goal: export");
    assert_eq!(opened["labels"][0], label_id("task"));
    assert_eq!(opened["labels"][1], label_id("state/blocked"));
    assert_eq!(
        fake.bodies(&format!("POST {API}/issues/5/dependencies")),
        [json!({ "owner": "example", "repo": "tracker", "index": 9 })]
    );
}

#[test]
fn a_label_created_beside_an_older_one_of_its_name_is_deleted_and_the_older_used() {
    let fake = FakeCurl::new();
    // The first read lacks `state/ready`; by the read after the creation another writer's
    // older one is there too.
    fake.once(&format!("GET {API}/labels?*"), 200, &json!([]));
    fake.on(
        &format!("POST {API}/labels"),
        201,
        &json!({ "id": 77, "name": "state/ready", "exclusive": true }),
    );
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &json!([
            { "id": 11, "name": "state/ready", "exclusive": true },
            { "id": 77, "name": "state/ready", "exclusive": true },
        ]),
    );
    fake.on(&format!("DELETE {API}/labels/77"), 204, &Value::Null);
    let api = fake.api();
    let mut repo = labels::RepoLabels::read(&api).expect("labels read");
    let id = repo
        .ensure(&api, &labels::LabelSpec::state(TicketState::Ready))
        .expect("the label");
    assert_eq!(id, 11);
    assert_eq!(
        fake.asked(),
        [
            format!("GET {API}/labels?page=1&limit=50"),
            format!("POST {API}/labels"),
            format!("GET {API}/labels?page=1&limit=50"),
            format!("DELETE {API}/labels/77"),
        ]
    );
    let created = &fake.bodies(&format!("POST {API}/labels"))[0];
    assert_eq!(created["name"], "state/ready");
    assert_eq!(created["exclusive"], true);
    assert_eq!(repo.id("state/ready"), Some(11));
}

#[test]
fn issue_types_are_the_type_labels_and_task() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["task", "type/Bug", "state/ready"]),
    );
    let types = fake.board().issue_types().expect("types read");
    assert_eq!(types.repo, REPO);
    assert_eq!(
        types.names,
        [TicketType::from("Bug"), TicketType::from("Task")]
    );
}

#[test]
fn epics_read_tasks_from_dependencies_and_their_notes_from_comments() {
    let fake = FakeCurl::new();
    let epic_body = "An epic.";
    fake.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=epic*"),
        200,
        &json!([issue_with_body(1, &["epic"], "open", epic_body)]),
    );
    let signature = Signature::of(&epic_body.into(), at(1_800_000_000));
    let signed = marked(&signature_marker(&signature), "Signed.");
    let challenger: Instance = "challenger instance 3 on #1".parse().expect("an instance");
    let challenged = marked(&report_marker(&challenger), "Every task is exact.");
    let planned = "<!-- zaf:exit planning instance 2 on #1 done -->\nplanned".to_owned();
    fake.on(
        &format!("GET {API}/issues/1/comments"),
        200,
        &comments(&[signed, planned, challenged]),
    );
    let mut dropped = issue(4, &["task", "state/ready", "not planned"], "closed");
    dropped["title"] = json!("dropped");
    fake.on(
        &format!("GET {API}/issues/1/dependencies?*"),
        200,
        &json!([
            dropped,
            issue(
                2,
                &["task", "state/ready", "group:core", "type/Task"],
                "open"
            ),
            issue(3, &["task", "state/done"], "closed"),
            // A dependency that is not a task is not one of the epic's tasks.
            issue(8, &["epic"], "open"),
        ]),
    );
    let epics = fake.board().epics().expect("epics read");
    assert_eq!(epics.len(), 1);
    let epic = &epics[0];
    let tasks: Vec<(u64, Standing)> = epic
        .tasks
        .iter()
        .map(|task| (task.number.get(), task.standing))
        .collect();
    assert_eq!(
        tasks,
        [
            (2, Standing::Open),
            (3, Standing::Done),
            (4, Standing::ClosedNotPlanned)
        ]
    );
    assert_eq!(
        epic.tasks[0].group.as_ref().map(|g| g.as_str()),
        Some("core")
    );
    assert_eq!(epic.tasks[0].kind, Some(TicketType::from("Task")));
    assert_eq!(
        epic.signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&epic_body.into()))
    );
    assert!(epic.signed());
    assert!(epic.challenged);
    assert_eq!(epic.planned_at, parse_time("2026-11-12T10:00:00Z"));
}

#[test]
fn a_ticket_that_is_no_open_epic_is_not_one() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task"], "open"),
    );
    fake.on(
        &format!("GET {API}/issues/4"),
        200,
        &issue(4, &["epic"], "closed"),
    );
    let board = fake.board();
    for number in [3, 4] {
        let read = board.epic(TicketId::from(number));
        assert!(matches!(read, Err(PortError::NotFound(_))), "got {read:?}");
    }
}

#[test]
fn a_question_opens_with_its_labels_and_state() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["question", "door", "state/question"]),
    );
    fake.on(&format!("POST {API}/issues"), 201, &issue(12, &[], "open"));
    let asked = NewQuestion {
        on: TicketId::from(3),
        text: "Which way?".to_owned(),
        door: true,
    };
    let opened = fake
        .board()
        .open_question(&asked)
        .expect("the question opens");
    assert_eq!(opened, TicketId::from(12));
    let body = &fake.bodies(&format!("POST {API}/issues"))[0];
    assert_eq!(body["title"], "Which way?");
    assert_eq!(body["body"], "Raised on #3.\n\nWhich way?");
    assert_eq!(
        body["labels"],
        json!([
            label_id("question"),
            label_id("state/question"),
            label_id("door")
        ])
    );
}

#[test]
fn a_ruling_comments_closes_the_question_and_returns_the_waiting_task() {
    let fake = FakeCurl::new();
    let raised = NewQuestion {
        on: TicketId::from(3),
        text: String::new(),
        door: false,
    }
    .body();
    fake.on(
        &format!("GET {API}/issues/12"),
        200,
        &issue_with_body(12, &["question", "state/question"], "open", &raised),
    );
    fake.on(&format!("GET {API}/issues/12/comments"), 200, &json!([]));
    fake.on(
        &format!("POST {API}/issues/12/comments"),
        201,
        &json!({ "id": 9 }),
    );
    fake.on(
        &format!("PATCH {API}/issues/12"),
        201,
        &issue(12, &[], "closed"),
    );
    fake.on(
        &format!("GET {API}/issues/3"),
        200,
        &issue(3, &["task", "state/question"], "open"),
    );
    fake.on(
        &format!("GET {API}/labels?*"),
        200,
        &labels(&["state/ready", "state/question"]),
    );
    fake.on(
        &format!("POST {API}/issues/3/labels"),
        200,
        &labels(&["task", "state/ready"]),
    );
    let returned = fake
        .board()
        .record_ruling(TicketId::from(12), "this way")
        .expect("the ruling");
    assert_eq!(returned, Some(TicketId::from(3)));
    assert_eq!(
        fake.bodies(&format!("POST {API}/issues/12/comments")),
        [json!({ "body": "<!-- zaf:ruling -->\nthis way" })]
    );
    assert_eq!(
        fake.bodies(&format!("PATCH {API}/issues/12")),
        [json!({ "state": "closed" })]
    );
}

#[test]
fn a_different_ruling_is_a_conflict() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/12"),
        200,
        &issue(12, &["question"], "closed"),
    );
    fake.on(
        &format!("GET {API}/issues/12/comments"),
        200,
        &comments(&["<!-- zaf:ruling -->\nthis way".to_owned()]),
    );
    let changed = fake
        .board()
        .record_ruling(TicketId::from(12), "the other way");
    assert!(
        matches!(changed, Err(PortError::Conflict(_))),
        "got {changed:?}"
    );
}

#[test]
fn a_report_posts_once_then_edits_its_own_comment() {
    let fake = FakeCurl::new();
    let marker = report_marker(&instance(1));
    fake.on(
        &format!("GET {API}/issues/3/comments"),
        200,
        &comments(&["a plain comment".to_owned(), format!("{marker}\nfirst")]),
    );
    fake.on(
        &format!("PATCH {API}/issues/comments/101"),
        200,
        &json!({ "id": 101 }),
    );
    let board = fake.board();
    board
        .report(TicketId::from(3), &instance(1), "first")
        .expect("unchanged");
    assert!(fake.asked().iter().all(|asked| asked.starts_with("GET")));
    board
        .report(TicketId::from(3), &instance(1), "second")
        .expect("edited");
    assert_eq!(
        fake.bodies(&format!("PATCH {API}/issues/comments/101")),
        [json!({ "body": format!("{marker}\nsecond") })]
    );
}

#[test]
fn access_reports_the_version_and_a_token_that_reads_and_writes() {
    let fake = FakeCurl::new();
    fake.on(
        "GET http://forgejo.test/api/v1/version",
        200,
        &json!({ "version": "15.0.11+gitea-1.22.0" }),
    );
    fake.on(
        &format!("GET {API}"),
        200,
        &json!({ "permissions": { "admin": false, "push": true, "pull": true } }),
    );
    fake.on(
        &format!("POST {API}/issues"),
        422,
        &json!({ "message": "[Title]: Required" }),
    );
    let checks = fake.board().access();
    let lines: Vec<(String, bool, String)> = checks
        .into_iter()
        .map(|check| (check.what, check.ok, check.detail))
        .collect();
    assert_eq!(
        lines,
        [
            (
                "instance".to_owned(),
                true,
                "Forgejo 15.0.11+gitea-1.22.0 at http://forgejo.test".to_owned()
            ),
            (
                "token".to_owned(),
                true,
                "`FORGEJO_TOKEN` reads and writes example/tracker".to_owned()
            ),
        ]
    );
    assert_eq!(fake.bodies(&format!("POST {API}/issues")), [json!({})]);
}

#[test]
fn access_fails_a_token_whose_scope_cannot_write() {
    let fake = FakeCurl::new();
    fake.on(
        "GET http://forgejo.test/api/v1/version",
        200,
        &json!({ "version": "15.0.11" }),
    );
    fake.on(
        &format!("GET {API}"),
        200,
        &json!({ "permissions": { "push": true, "pull": true } }),
    );
    fake.on(
        &format!("POST {API}/issues"),
        403,
        &json!({ "message": "token does not have at least one of required scope(s): [write:issue]" }),
    );
    let checks = fake.board().access();
    assert!(checks[0].ok);
    assert!(!checks[1].ok);
    assert!(
        checks[1].detail.contains("cannot write"),
        "{}",
        checks[1].detail
    );
    assert!(checks[1].detail.contains("403"), "{}", checks[1].detail);
}

#[test]
fn access_fails_both_when_the_instance_does_not_answer_or_no_token_is_set() {
    let fake = FakeCurl::new();
    fake.unreachable("GET http://forgejo.test/api/v1/version");
    let checks = fake.board().access();
    assert!(!checks[0].ok && !checks[1].ok);
    assert!(
        checks[0]
            .detail
            .contains("no answer from http://forgejo.test"),
        "{}",
        checks[0].detail
    );

    let answering = FakeCurl::new();
    answering.on(
        "GET http://forgejo.test/api/v1/version",
        200,
        &json!({ "version": "15.0.11" }),
    );
    let checks = ForgejoBoard::from(answering.api_with(None)).access();
    assert!(checks[0].ok);
    assert!(!checks[1].ok);
    assert_eq!(
        checks[1].detail,
        "`FORGEJO_TOKEN` is not set: export a Forgejo access token in it, with read and write on the repository's issues and pull requests"
    );
}

mod forge;
