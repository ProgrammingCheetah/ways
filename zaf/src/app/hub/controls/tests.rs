//! `POST /api/stop`, `POST /api/tick`, `POST /api/drop` and `POST /api/answer` over the
//! dashboard's socket: the guard, the project and what the control does, end to end. The stop runs
//! on a fixture with a live instance for the task, the answer on one with a task waiting on a
//! question; the tick on a fake ticker that records what it was asked and answers what it is
//! given.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::*;
use crate::app::actions::PHASES_DIR;
use crate::app::board::question as raise;
use crate::app::hub::guard::{Guard, KEY_HEADER, Key};
use crate::app::hub::{Dashboard, Page, StateReader};
use crate::app::projects::Projects;
use crate::app::spawn::stop::{self, GRACE, Processes, Signal};
use crate::domain::{Instance, InstanceNumber, RoleName, Ticket, TicketState};
use crate::fakes::FakeBoard;
use crate::ports::{Board, Holder, NewQuestion};

const TASK: u64 = 17;
const PID: u32 = 700;
const INSTANCE: &str = "implement instance 3 on #1";

fn holder() -> Holder {
    Holder {
        pid: PID,
        start: "1000".to_owned(),
    }
}

/// The process table: the pids alive; every process ends on `SIGTERM`.
#[derive(Default)]
struct Table {
    alive: Mutex<BTreeSet<u32>>,
}

impl Processes for Table {
    fn alive(&self, holder: &Holder) -> bool {
        self.alive.lock().unwrap().contains(&holder.pid)
    }

    fn group_of(&self, _: &Holder) -> Option<u32> {
        None
    }

    fn signal(&self, holder: &Holder, _: Option<u32>, _: Signal) {
        self.alive.lock().unwrap().remove(&holder.pid);
    }

    fn pause(&self, _: Duration) {}
}

/// A machine with one listed project, its state folder holding a run of the task when `running`,
/// and a board that has the task claimed by that run's instance.
struct Fixture {
    _home: tempfile::TempDir,
    _project: tempfile::TempDir,
    _state: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    board: Arc<FakeBoard>,
    table: Arc<Table>,
    /// The roots the stop was asked to run from.
    asked: Arc<Mutex<Vec<PathBuf>>>,
    stops: Arc<AtomicUsize>,
    stopper: Arc<Stopper>,
}

fn fixture(running: bool) -> Fixture {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let root = std::fs::canonicalize(project.path()).expect("root");

    let state = tempfile::tempdir().expect("state");
    let table = Arc::new(Table::default());
    if running {
        let run = state.path().join(PHASES_DIR).join("implement-3-task-17");
        std::fs::create_dir_all(&run).expect("run folder");
        std::fs::write(run.join("task"), format!("{TASK}\n")).expect("task");
        std::fs::write(run.join("instance"), INSTANCE).expect("instance");
        std::fs::write(run.join("pid"), holder().to_string()).expect("pid");
        table.alive.lock().unwrap().insert(PID);
    }
    let board = Arc::new(FakeBoard::default());
    board.add_ticket(Ticket {
        id: TicketId::from(TASK),
        epic: Some(TicketId::from(1)),
        title: "A task".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    board
        .claim(
            TicketId::from(TASK),
            &Instance {
                role: RoleName::from("implement"),
                epic: TicketId::from(1),
                number: InstanceNumber::from(3),
            },
        )
        .expect("claim");

    let asked = Arc::new(Mutex::new(Vec::new()));
    let stops = Arc::new(AtomicUsize::new(0));
    let stopper: Arc<Stopper> = {
        let (board, table, asked, stops) = (
            Arc::clone(&board),
            Arc::clone(&table),
            Arc::clone(&asked),
            Arc::clone(&stops),
        );
        let state = state.path().to_owned();
        Arc::new(move |root, task| {
            stops.fetch_add(1, Ordering::SeqCst);
            asked.lock().unwrap().push(root.to_owned());
            stop::stop(task, &state, board.as_ref(), table.as_ref(), GRACE).map_err(Failure::from)
        })
    };
    Fixture {
        home: home.path().to_owned(),
        _home: home,
        _project: project,
        _state: state,
        root,
        board,
        table,
        asked,
        stops,
        stopper,
    }
}

/// A server with the fixture's controls, and the key it admits.
fn serve(fixture: &Fixture) -> (u16, String) {
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let key = Key::generate().expect("a key");
    let secret = key.reveal().to_owned();
    let controls =
        Controls::new(Some(fixture.home.clone())).with_stop(Arc::clone(&fixture.stopper));
    let state: Arc<StateReader> = Arc::new(|| Err("not read in this test".to_owned()));
    let _page = dashboard.start_with("frame\n", state, Guard::new(key, false), controls);
    (port, secret)
}

/// `POST /api/stop` with `body`, carrying `key` when given; the status line and the JSON body.
fn post_stop(port: u16, key: Option<&str>, body: &str) -> (String, serde_json::Value) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let key = key.map_or_else(String::new, |key| format!("{KEY_HEADER}: {key}\r\n"));
    let request = format!(
        "POST /api/stop HTTP/1.1\r\nHost: zaf\r\n{key}Content-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");
    let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    let status = head.lines().next().expect("a status line").to_owned();
    (status, serde_json::from_str(body).expect("JSON"))
}

fn stop_body(fixture: &Fixture) -> String {
    serde_json::json!({ "project": fixture.root, "task": TASK }).to_string()
}

fn ready(board: &FakeBoard) -> Vec<TicketId> {
    board
        .ready()
        .expect("ready")
        .iter()
        .map(|ticket| ticket.id)
        .collect()
}

#[test]
fn stop_ends_instance() {
    let fixture = fixture(true);
    let (port, key) = serve(&fixture);
    assert!(ready(&fixture.board).is_empty(), "claimed, so not ready");

    let (status, body) = post_stop(port, Some(&key), &stop_body(&fixture));

    assert_eq!(status, "HTTP/1.1 200 OK", "{body}");
    assert_eq!(body["task"], TASK);
    assert_eq!(body["stopped"][0]["pid"], PID);
    assert_eq!(body["stopped"][0]["instance"], INSTANCE);
    assert_eq!(body["released"], serde_json::json!([INSTANCE]));
    assert!(!fixture.table.alive(&holder()), "the instance is gone");
    assert_eq!(
        ready(&fixture.board),
        vec![TicketId::from(TASK)],
        "the task reads released"
    );
    assert_eq!(
        *fixture.asked.lock().unwrap(),
        vec![fixture.root.clone()],
        "the stop ran from the project's root"
    );
}

#[test]
fn stop_without_instance_refused() {
    let fixture = fixture(false);
    let (port, key) = serve(&fixture);

    let (status, body) = post_stop(port, Some(&key), &stop_body(&fixture));

    assert_eq!(status, "HTTP/1.1 409 Conflict");
    let refused = body["refused"].as_str().expect("a refusal with its reason");
    assert!(refused.contains("no instance is running"), "{refused}");
    assert_eq!(body.as_object().expect("an object").len(), 1, "{body}");
    assert!(
        ready(&fixture.board).is_empty(),
        "the claim is kept: nothing changed"
    );
}

#[test]
fn stop_needs_key() {
    let fixture = fixture(true);
    let (port, key) = serve(&fixture);
    let wrong = format!("{key}0");

    for presented in [None, Some(wrong.as_str()), Some("")] {
        let (status, body) = post_stop(port, presented, &stop_body(&fixture));
        assert_eq!(status, "HTTP/1.1 403 Forbidden", "{presented:?}");
        assert!(body["refused"].is_string(), "{body}");
    }

    assert_eq!(fixture.stops.load(Ordering::SeqCst), 0, "no stop was asked");
    assert!(fixture.table.alive(&holder()), "the instance still runs");
    assert!(ready(&fixture.board).is_empty(), "the claim is kept");
}

/// A ticker that answers `answer` and keeps each tick it was asked for.
struct Fake {
    answer: Ticked,
    asked: Mutex<Vec<(PathBuf, String)>>,
}

impl Fake {
    fn answering(answer: Ticked) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<(PathBuf, String)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Ticker for Fake {
    fn tick(&self, root: &Path, version: &str) -> Ticked {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((root.to_owned(), version.to_owned()));
        self.answer.clone()
    }
}

/// A zaf home whose list holds one project, with a manifest, and a hub serving the controls over
/// it. Everything is dropped with the value.
struct Hub {
    home: tempfile::TempDir,
    project: tempfile::TempDir,
    ticker: Arc<Fake>,
    port: u16,
    key: String,
    _page: Page,
}

fn hub(answer: Ticked) -> Hub {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(
        project.path().join(".zaf.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let ticker = Fake::answering(answer);
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://zaf/").expect("a link");
    let key = link
        .rsplit_once("key=")
        .expect("a key in the link")
        .1
        .to_owned();
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let controls = Controls::new(Some(home.path().to_owned())).with_tick(ticker.clone());
    let page = dashboard.start_with(
        "frame\n",
        Arc::new(|| Err("not read in this test".to_owned())),
        guard,
        controls,
    );
    Hub {
        home,
        project,
        ticker,
        port,
        key,
        _page: page,
    }
}

impl Hub {
    /// A `POST /api/tick` with `body`, carrying the hub's key when `keyed`: the whole answer.
    fn post(&self, keyed: bool, body: &str) -> String {
        let header = if keyed {
            format!("{KEY_HEADER}: {}\r\n", self.key)
        } else {
            String::new()
        };
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        let request = format!(
            "POST /api/tick HTTP/1.1\r\nHost: zaf\r\n{header}Content-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("writes");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("reads");
        answer
    }

    /// The tick of the listed project, with `version` when there is one.
    fn tick(&self, version: Option<&str>) -> String {
        let root = self.root();
        let body = match version {
            Some(version) => serde_json::json!({ "project": root, "version": version }),
            None => serde_json::json!({ "project": root }),
        };
        self.post(true, &body.to_string())
    }

    fn root(&self) -> PathBuf {
        std::fs::canonicalize(self.project.path()).expect("root")
    }

    fn remembered(&self) -> Option<String> {
        Projects::read(self.home.path())
            .into_iter()
            .next()
            .expect("the project")
            .version
    }
}

/// The JSON body of an answer.
fn body_of(answer: &str) -> serde_json::Value {
    let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    serde_json::from_str(body).expect("JSON")
}

#[test]
fn tick_needs_key() {
    let hub = hub(Ticked::Ran(vec!["started implement #7".to_owned()]));
    let body = serde_json::json!({ "project": hub.root(), "version": "0.4.0" }).to_string();

    let answer = hub.post(false, &body);

    assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
    assert!(hub.ticker.asked().is_empty(), "nothing was ticked");
    assert_eq!(hub.remembered(), None, "nothing was remembered");
}

#[test]
fn a_wrong_key_changes_nothing() {
    let hub = hub(Ticked::Ran(Vec::new()));
    let body = serde_json::json!({ "project": hub.root(), "version": "0.4.0" }).to_string();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, hub.port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    write!(
        stream,
        "POST /api/tick HTTP/1.1\r\nHost: zaf\r\n{KEY_HEADER}: wrong\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");

    assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
    assert!(hub.ticker.asked().is_empty());
    assert_eq!(hub.remembered(), None);
}

#[test]
fn a_refusal_is_not_lost_to_a_body_left_unread() {
    let hub = hub(Ticked::Ran(Vec::new()));
    // A head that nearly fills the hub's first read and a body that runs past it, sent as one: the
    // hub refuses with part of the body still unread. Closing then would reset the connection,
    // and the read below would fail.
    let body = "x".repeat(2000);
    let request = format!(
        "POST /api/tick HTTP/1.1\r\nHost: zaf\r\n{KEY_HEADER}: wrong\r\nX-Padding: {}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        "p".repeat(800),
        body.len()
    );
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, hub.port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");

    assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
    assert!(hub.ticker.asked().is_empty());
}

#[test]
fn tick_needs_version_first() {
    let hub = hub(Ticked::Ran(Vec::new()));

    let answer = hub.tick(None);

    assert!(answer.starts_with("HTTP/1.1 400 Bad Request"), "{answer}");
    let refused = body_of(&answer)["refused"]
        .as_str()
        .expect("a refusal")
        .to_owned();
    assert!(refused.contains("a version is needed"), "{refused}");
    assert!(hub.ticker.asked().is_empty(), "no tick ran");
    assert_eq!(hub.remembered(), None);
}

#[test]
fn tick_remembers_version() {
    let hub = hub(Ticked::Ran(vec!["nothing to do".to_owned()]));

    let first = hub.tick(Some("0.4.0"));
    assert!(first.starts_with("HTTP/1.1 200 OK"), "{first}");
    assert_eq!(hub.remembered().as_deref(), Some("0.4.0"));

    // The second names no version, and runs with the one remembered.
    let second = hub.tick(None);
    assert!(second.starts_with("HTTP/1.1 200 OK"), "{second}");
    assert_eq!(body_of(&second)["version"], "0.4.0");
    let root = hub.root();
    assert_eq!(
        hub.ticker.asked(),
        [
            (root.clone(), "0.4.0".to_owned()),
            (root, "0.4.0".to_owned())
        ]
    );

    // A version given again replaces it.
    let third = hub.tick(Some("0.5.0"));
    assert!(third.starts_with("HTTP/1.1 200 OK"), "{third}");
    assert_eq!(hub.remembered().as_deref(), Some("0.5.0"));
    assert_eq!(hub.ticker.asked()[2].1, "0.5.0");
}

#[test]
fn tick_starts_ready_work() {
    // The tick's own words, as the verb prints them for one ready task it launched.
    let hub = hub(Ticked::Ran(vec![
        "tick 14:32".to_owned(),
        "started implement #7: 1 units".to_owned(),
    ]));

    let answer = hub.tick(Some("0.4.0"));

    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    let body = body_of(&answer);
    assert_eq!(body["running"], false);
    let said: Vec<&str> = body["tick"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|line| line.as_str().expect("a line"))
        .collect();
    assert!(said.contains(&"started implement #7: 1 units"), "{said:?}");
    assert_eq!(hub.ticker.asked(), [(hub.root(), "0.4.0".to_owned())]);
}

#[test]
fn a_route_that_is_no_control_is_unknown() {
    let hub = hub(Ticked::Ran(Vec::new()));
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, hub.port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    write!(
        stream,
        "POST /api/nothing HTTP/1.1\r\nHost: zaf\r\n{KEY_HEADER}: {}\r\nContent-Length: 2\r\n\
         Connection: close\r\n\r\n{{}}",
        hub.key
    )
    .expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");

    assert!(answer.starts_with("HTTP/1.1 404 Not Found"), "{answer}");
}

#[test]
fn a_body_that_is_too_large_is_refused_unread() {
    let hub = hub(Ticked::Ran(Vec::new()));
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, hub.port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    write!(
        stream,
        "POST /api/tick HTTP/1.1\r\nHost: zaf\r\n{KEY_HEADER}: {}\r\nContent-Length: 1000000\r\n\
         Connection: close\r\n\r\n",
        hub.key
    )
    .expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");

    assert!(answer.starts_with("HTTP/1.1 413 "), "{answer}");
    assert!(hub.ticker.asked().is_empty());
}

const HELD: u64 = 12;
const IDLE: u64 = 13;

/// A machine with one listed project, a board with a task, `#12`, waiting on a question and an
/// idle ready task, `#13`; the answer control rules on that board through [`rule`].
struct Asked {
    _home: tempfile::TempDir,
    _project: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    board: Arc<FakeBoard>,
    question: TicketId,
    /// The roots the ruling was asked to run from.
    roots: Arc<Mutex<Vec<PathBuf>>>,
    ruler: Arc<Ruler>,
}

fn task(number: u64, state: TicketState) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: Some(TicketId::from(1)),
        title: format!("Task {number}").into(),
        body: "".into(),
        state: Some(state),
        url: "".into(),
        kind: None,
    }
}

fn asked() -> Asked {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let root = std::fs::canonicalize(project.path()).expect("root");

    let board = Arc::new(FakeBoard::default());
    board.add_ticket(task(HELD, TicketState::Claimed));
    board.add_ticket(task(IDLE, TicketState::Ready));
    let question = raise(
        board.as_ref(),
        &NewQuestion {
            on: TicketId::from(HELD),
            text: "Which columns?".to_owned(),
            door: false,
        },
    )
    .expect("raised");

    let roots = Arc::new(Mutex::new(Vec::new()));
    let ruler: Arc<Ruler> = {
        let (board, roots) = (Arc::clone(&board), Arc::clone(&roots));
        Arc::new(move |root, question, ruling| {
            roots.lock().unwrap().push(root.to_owned());
            rule(board.as_ref(), question, ruling)
        })
    };
    Asked {
        home: home.path().to_owned(),
        _home: home,
        _project: project,
        root,
        board,
        question,
        roots,
        ruler,
    }
}

impl Asked {
    /// A server with the answer control built on the fixture, and the key it admits.
    fn serve(&self) -> (u16, String) {
        let dashboard = Dashboard::bind(0).expect("binds");
        let port = dashboard.port();
        let key = Key::generate().expect("a key");
        let secret = key.reveal().to_owned();
        let controls = Controls::new(Some(self.home.clone())).with_answer(Arc::clone(&self.ruler));
        let state: Arc<StateReader> = Arc::new(|| Err("not read in this test".to_owned()));
        let _page = dashboard.start_with("frame\n", state, Guard::new(key, false), controls);
        (port, secret)
    }

    fn body(&self, issue: u64, ruling: &str) -> String {
        serde_json::json!({ "project": self.root, "issue": issue, "ruling": ruling }).to_string()
    }

    fn state_of(&self, task: u64) -> Option<TicketState> {
        self.board.task(TicketId::from(task)).expect("task").state
    }
}

/// `POST /api/answer` with `body`, carrying `key` when given; the status line and the JSON body.
fn post_answer(port: u16, key: Option<&str>, body: &str) -> (String, serde_json::Value) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let key = key.map_or_else(String::new, |key| format!("{KEY_HEADER}: {key}\r\n"));
    let request = format!(
        "POST /api/answer HTTP/1.1\r\nHost: zaf\r\n{key}Content-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");
    let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    let status = head.lines().next().expect("a status line").to_owned();
    (status, serde_json::from_str(body).expect("JSON"))
}

#[test]
fn answer_rules_the_question() {
    let asked = asked();
    let (port, key) = asked.serve();

    let ask = asked.body(asked.question.get(), "Title only.");
    let (status, body) = post_answer(port, Some(&key), &ask);

    assert_eq!(status, "HTTP/1.1 200 OK", "{body}");
    assert_eq!(body["question"], asked.question.get());
    assert_eq!(
        body["answer"],
        format!("{} ruled; #{HELD} ready", asked.question)
    );
    assert_eq!(
        asked.board.question(asked.question),
        Some((false, Some("Title only.".to_owned()))),
        "recorded as `zaf board ruling` records it"
    );
    assert!(asked.board.is_closed(asked.question));
    assert_eq!(
        *asked.roots.lock().unwrap(),
        vec![asked.root.clone()],
        "the ruling ran from the project's root"
    );
}

#[test]
fn answer_unblocks_the_task() {
    let asked = asked();
    let (port, key) = asked.serve();
    assert_eq!(asked.state_of(HELD), Some(TicketState::Question));

    let ask = asked.body(asked.question.get(), "Title only.");
    let (_, body) = post_answer(port, Some(&key), &ask);

    assert_eq!(body["ready"], HELD);
    assert_eq!(asked.state_of(HELD), Some(TicketState::Ready));
}

#[test]
fn answer_on_non_question_refused() {
    let asked = asked();
    let (port, key) = asked.serve();
    // A ruling already made, so the question is no longer open.
    let ruled = asked.question.get();
    post_answer(port, Some(&key), &asked.body(ruled, "Title only."));

    // A task, an issue that does not exist and the question already ruled: none is open.
    for issue in [IDLE, 999, ruled] {
        let (status, body) = post_answer(port, Some(&key), &asked.body(issue, "Anything."));
        assert_eq!(status, "HTTP/1.1 409 Conflict", "#{issue}");
        let why = body["refused"].as_str().expect("the reason");
        assert!(why.contains("not an open question"), "#{issue}: {why}");
        assert_eq!(body.as_object().expect("an object").len(), 1, "{body}");
    }

    assert_eq!(asked.state_of(IDLE), Some(TicketState::Ready));
    assert_eq!(
        asked.board.question(asked.question),
        Some((false, Some("Title only.".to_owned()))),
        "the ruling already made stands"
    );
    assert!(asked.board.comments(TicketId::from(IDLE)).is_empty());
}

#[test]
fn answer_refuses_what_it_cannot_read_and_changes_nothing() {
    let asked = asked();
    let (port, key) = asked.serve();
    let number = asked.question.get();
    let outside = serde_json::json!({ "project": "/etc", "issue": number, "ruling": "x" });
    let asks = [
        ("not json".to_owned(), "400 Bad Request"),
        (r#"{"project":"x","issue":1}"#.to_owned(), "400 Bad Request"),
        (asked.body(0, "x"), "400 Bad Request"),
        (asked.body(number, "  \n"), "400 Bad Request"),
        (outside.to_string(), "404 Not Found"),
    ];
    for (ask, status) in asks {
        let (got, body) = post_answer(port, Some(&key), &ask);
        assert_eq!(got, format!("HTTP/1.1 {status}"), "{ask}");
        assert!(body["refused"].is_string(), "{ask}: {body}");
    }

    assert!(asked.roots.lock().unwrap().is_empty(), "no board was asked");
    assert!(!asked.board.is_closed(asked.question));
    assert_eq!(asked.state_of(HELD), Some(TicketState::Question));
}

#[test]
fn answer_needs_key() {
    let asked = asked();
    let (port, key) = asked.serve();
    let wrong = format!("{key}0");
    let ask = asked.body(asked.question.get(), "Title only.");

    for presented in [None, Some(wrong.as_str()), Some("")] {
        let (status, body) = post_answer(port, presented, &ask);
        assert_eq!(status, "HTTP/1.1 403 Forbidden", "{presented:?}");
        assert!(body["refused"].is_string(), "{body}");
    }

    assert!(
        asked.roots.lock().unwrap().is_empty(),
        "no ruling was asked"
    );
    assert!(!asked.board.is_closed(asked.question), "nothing was ruled");
    assert_eq!(asked.state_of(HELD), Some(TicketState::Question));
}

/// A hub serving over a zaf home whose list holds two projects: `here`, a folder with a manifest,
/// and `gone`, a folder that does not exist. `GET /api/state` reads that list.
struct DropHub {
    port: u16,
    key: String,
    home: tempfile::TempDir,
    _page: crate::app::hub::Page,
}

fn drop_hub() -> DropHub {
    let home = tempfile::tempdir().expect("home");
    let here = home.path().join("here");
    std::fs::create_dir_all(&here).expect("folder");
    std::fs::write(here.join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), &here).expect("listed");
    Projects::add(home.path(), &home.path().join("gone")).expect("listed");
    let root = home.path().to_owned();
    let state: Arc<StateReader> = Arc::new(move || {
        Ok(crate::app::hub::state::gather_machine(
            &root,
            std::time::SystemTime::now(),
        ))
    });
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let made = Key::generate().expect("a key");
    let key = made.reveal().to_owned();
    let controls = Controls::new(Some(home.path().to_owned()));
    let page = dashboard.start_with("frame\n", state, Guard::new(made, false), controls);
    DropHub {
        port,
        key,
        home,
        _page: page,
    }
}

impl DropHub {
    fn ask(&self, request: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        stream.write_all(request.as_bytes()).expect("writes");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("reads");
        answer
    }

    /// `POST /api/drop` with `body`, carrying `key` when given.
    fn post(&self, key: Option<&str>, body: &str) -> String {
        let key = key.map_or_else(String::new, |key| format!("{KEY_HEADER}: {key}\r\n"));
        self.ask(&format!(
            "POST /api/drop HTTP/1.1\r\nHost: zaf\r\n{key}Content-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        ))
    }

    fn drop(&self, folder: &Path, key: Option<&str>) -> String {
        self.post(key, &serde_json::json!({ "project": folder }).to_string())
    }

    /// The roots `GET /api/state` lists.
    fn listed(&self) -> Vec<String> {
        let answer = self.ask("GET /api/state HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n");
        let state = json_of(&answer);
        state["projects"]
            .as_array()
            .expect("projects")
            .iter()
            .map(|project| project["root"].as_str().expect("a root").to_owned())
            .collect()
    }

    fn folders(&self) -> Vec<PathBuf> {
        Projects::read(self.home.path())
            .into_iter()
            .map(|entry| entry.folder)
            .collect()
    }
}

fn status_of(answer: &str) -> &str {
    answer.lines().next().expect("a status line")
}

fn json_of(answer: &str) -> serde_json::Value {
    let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    serde_json::from_str(body).expect("JSON")
}

#[test]
fn drop_removes_missing() {
    let hub = drop_hub();
    let gone = hub.home.path().join("gone");
    assert!(hub.listed().contains(&gone.display().to_string()));

    let answer = hub.drop(&gone, Some(&hub.key));
    assert_eq!(status_of(&answer), "HTTP/1.1 200 OK", "{answer}");

    assert!(!hub.folders().contains(&gone), "gone from the list");
    assert!(
        !hub.listed().contains(&gone.display().to_string()),
        "gone from the next GET /api/state"
    );
    assert_eq!(hub.folders().len(), 1, "the present project stays");
}

#[test]
fn drop_refuses_present() {
    let hub = drop_hub();
    let here = hub.home.path().join("here");
    let answer = hub.drop(&here, Some(&hub.key));
    assert_eq!(status_of(&answer), "HTTP/1.1 409 Conflict", "{answer}");
    let refused = json_of(&answer)["refused"]
        .as_str()
        .expect("a reason")
        .to_owned();
    assert!(refused.contains("present"), "{refused}");
    assert!(hub.folders().contains(&here), "it stays");
    assert!(here.is_dir(), "its folder is untouched");

    // A folder not on the list is not found, and a body that is not a project is a bad request.
    let other = hub.drop(&hub.home.path().join("other"), Some(&hub.key));
    assert_eq!(status_of(&other), "HTTP/1.1 404 Not Found", "{other}");
    let bad = hub.post(Some(&hub.key), "{}");
    assert_eq!(status_of(&bad), "HTTP/1.1 400 Bad Request", "{bad}");
    assert_eq!(hub.folders().len(), 2, "nothing was dropped");
}

#[test]
fn drop_needs_key() {
    let hub = drop_hub();
    let gone = hub.home.path().join("gone");
    for key in [None, Some(""), Some("wrong")] {
        let answer = hub.drop(&gone, key);
        assert_eq!(status_of(&answer), "HTTP/1.1 403 Forbidden", "{key:?}");
    }
    assert!(hub.folders().contains(&gone), "still on the list");
}

// ---- POST /api/watch: a keyed start and stop of the project's watch, inside the hub ----

use crate::app::schedule::scheduler::Asked as TickAsked;
use crate::app::spawn::record::watch_running;
use crate::app::spawn::watch::{Tick, WatchHandle, Watches};

/// A tick that only counts itself: what the hub runs is the verb, which these tests do not need.
#[derive(Default)]
struct Counted(AtomicUsize);

impl Tick for Counted {
    fn tick(&self, _: &TickAsked) -> u8 {
        self.0.fetch_add(1, Ordering::SeqCst);
        0
    }

    fn say(&self, _: &str) {}
}

/// The watcher the hub's server is given in these tests: the real registry over a project's state
/// folder, each tick counted by `ticks`.
struct Counting {
    handle: WatchHandle,
    ticks: Arc<Counted>,
}

impl Watcher for Counting {
    fn start(&self, root: &Path, version: &str) -> Result<WatchStarted, Failure> {
        self.handle
            .start(
                &root.join(".zaf"),
                version,
                Duration::from_secs(86_400),
                self.ticks.clone(),
            )
            .map_err(Failure::from)
    }

    fn stop(&self, root: &Path) -> Result<WatchStopped, Failure> {
        self.handle.stop(&root.join(".zaf")).map_err(Failure::from)
    }
}

/// A hub over one listed project with the watch control built; `watches` is the hub's owner.
struct WatchHub {
    _home: tempfile::TempDir,
    _project: tempfile::TempDir,
    root: PathBuf,
    port: u16,
    key: String,
    ticks: Arc<Counted>,
    watches: Option<Watches>,
    _page: crate::app::hub::Page,
}

fn watch_hub() -> WatchHub {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let root = std::fs::canonicalize(project.path()).expect("root");
    let watches = Watches::new();
    let ticks = Arc::new(Counted::default());
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let made = Key::generate().expect("a key");
    let key = made.reveal().to_owned();
    let controls = Controls::new(Some(home.path().to_owned())).with_watch(Arc::new(Counting {
        handle: watches.handle(),
        ticks: Arc::clone(&ticks),
    }));
    let state: Arc<StateReader> = Arc::new(|| Err("not read in this test".to_owned()));
    let page = dashboard.start_with("frame\n", state, Guard::new(made, false), controls);
    WatchHub {
        _home: home,
        _project: project,
        root,
        port,
        key,
        ticks,
        watches: Some(watches),
        _page: page,
    }
}

impl WatchHub {
    fn state_dir(&self) -> PathBuf {
        self.root.join(".zaf")
    }

    /// `POST /api/watch` with `body`, carrying `key` when given.
    fn post(&self, key: Option<&str>, body: &serde_json::Value) -> (String, serde_json::Value) {
        let body = body.to_string();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("timeout");
        let key = key.map_or_else(String::new, |key| format!("{KEY_HEADER}: {key}\r\n"));
        let request = format!(
            "POST /api/watch HTTP/1.1\r\nHost: zaf\r\n{key}Content-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("writes");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("reads");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        let status = head.lines().next().expect("a status line").to_owned();
        (status, serde_json::from_str(body).expect("JSON"))
    }

    fn act(&self, action: &str) -> (String, serde_json::Value) {
        self.post(
            Some(&self.key),
            &serde_json::json!({ "project": self.root, "action": action, "version": "0.4.0" }),
        )
    }

    fn ticked(&self) -> usize {
        self.ticks.0.load(Ordering::SeqCst)
    }

    fn wait_for_a_tick(&self) {
        let limit = std::time::Instant::now() + Duration::from_secs(20);
        while self.ticked() == 0 {
            assert!(std::time::Instant::now() < limit, "no tick ran");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn watch_starts_and_stops() {
    let hub = watch_hub();
    assert!(!watch_running(&hub.state_dir()));

    let (status, body) = hub.act("start");

    assert_eq!(status, "HTTP/1.1 200 OK", "{body}");
    assert_eq!(body["watching"], true);
    assert_eq!(body["version"], "0.4.0");
    assert!(
        watch_running(&hub.state_dir()),
        "the watch reads as running for the project"
    );
    hub.wait_for_a_tick();
    assert_eq!(hub.ticked(), 1, "a tick ran at once, and no other");

    let (status, body) = hub.act("stop");

    assert_eq!(status, "HTTP/1.1 200 OK", "{body}");
    assert_eq!(body["watching"], false);
    assert!(
        !watch_running(&hub.state_dir()),
        "stopped, it reads as not running"
    );
    // And it can start again.
    let (status, _) = hub.act("start");
    assert_eq!(status, "HTTP/1.1 200 OK");
}

#[test]
fn second_watch_refused() {
    let hub = watch_hub();
    let (status, _) = hub.act("start");
    assert_eq!(status, "HTTP/1.1 200 OK");
    hub.wait_for_a_tick();

    let (status, body) = hub.act("start");

    assert_eq!(status, "HTTP/1.1 409 Conflict", "{body}");
    let why = body["refused"].as_str().expect("a refusal with its reason");
    assert!(why.contains("already runs"), "{why}");
    assert_eq!(body.as_object().expect("an object").len(), 1, "{body}");
    assert!(watch_running(&hub.state_dir()), "the first still runs");
    assert_eq!(hub.ticked(), 1, "the refused start ticked nothing");
}

#[test]
fn second_watch_refused_beside_a_scheduler_in_a_terminal() {
    let hub = watch_hub();
    // `zaf spawn --watch` in a terminal: its lock, held by a live process.
    let me = crate::adapters::process::this().expect("this process");
    let (terminal, _) = crate::app::schedule::lock::SchedulerLock::take(
        &hub.state_dir(),
        &me,
        "0.3.1",
        "0.4.0",
        std::time::SystemTime::now(),
    )
    .expect("taken");

    let (status, body) = hub.act("start");

    assert_eq!(status, "HTTP/1.1 409 Conflict", "{body}");
    assert!(
        body["refused"]
            .as_str()
            .expect("the reason")
            .contains("a scheduler already runs here"),
        "{body}"
    );
    let (status, body) = hub.act("stop");
    assert_eq!(status, "HTTP/1.1 409 Conflict", "{body}");
    assert!(watch_running(&hub.state_dir()), "the terminal's runs on");
    assert_eq!(hub.ticked(), 0);
    terminal.release();
}

#[test]
fn watch_ends_with_hub() {
    let mut hub = watch_hub();
    let (status, _) = hub.act("start");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(watch_running(&hub.state_dir()));

    // The hub ends: its owner of the watches is dropped, as the process's end drops it.
    drop(hub.watches.take());

    assert!(
        !watch_running(&hub.state_dir()),
        "the watch ended with the hub"
    );
    // The server's threads outlive the owner here, and start nothing once the hub has ended.
    let (status, body) = hub.act("start");
    assert_eq!(status, "HTTP/1.1 409 Conflict", "{body}");
    assert!(!watch_running(&hub.state_dir()));
}

#[test]
fn watch_needs_key() {
    let hub = watch_hub();
    let wrong = format!("{}0", hub.key);
    let start = serde_json::json!({ "project": hub.root, "action": "start", "version": "0.4.0" });

    for presented in [None, Some(wrong.as_str()), Some("")] {
        let (status, body) = hub.post(presented, &start);
        assert_eq!(status, "HTTP/1.1 403 Forbidden", "{presented:?}");
        assert!(body["refused"].is_string(), "{body}");
    }

    assert!(!watch_running(&hub.state_dir()), "nothing was started");
    assert_eq!(hub.ticked(), 0);

    // A stop needs it too: with a watch running, a keyless stop leaves it running.
    let (status, _) = hub.act("start");
    assert_eq!(status, "HTTP/1.1 200 OK");
    let stop = serde_json::json!({ "project": hub.root, "action": "stop" });
    let (status, _) = hub.post(None, &stop);
    assert_eq!(status, "HTTP/1.1 403 Forbidden");
    assert!(watch_running(&hub.state_dir()), "the watch runs on");
}
