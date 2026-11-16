use std::io::{BufRead, BufReader};
use std::net::Ipv6Addr;
use std::time::SystemTime;

use super::super::state::gather_machine;
use super::*;
use crate::app::projects::Projects;

const FRAME: &str = "epic #103   [###---]  1 of 4\n  1 of 2 slots free <b>&</b>\n";

fn started(frame: &str) -> (Page, u16) {
    started_with(frame, Arc::new(|| Err("not read in this test".to_owned())))
}

fn started_with(frame: &str, state: Arc<StateReader>) -> (Page, u16) {
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    let guard = Guard::start(false).expect("a key");
    (dashboard.start(frame, state, guard), port)
}

/// A reader of a zaf home whose list holds one project, `gone`, whose folder was removed: it is
/// read as missing, and nothing of it is asked of a board. `reads` counts the reads made.
fn one_missing_project(reads: &Arc<AtomicUsize>) -> Arc<StateReader> {
    let home = tempfile::tempdir().expect("home");
    let folder = home.path().join("gone");
    std::fs::create_dir(&folder).expect("folder");
    Projects::add(home.path(), &folder).expect("listed");
    std::fs::remove_dir(&folder).expect("removed");
    let reads = Arc::clone(reads);
    Arc::new(move || {
        reads.fetch_add(1, Ordering::SeqCst);
        Ok(gather_machine(home.path(), SystemTime::now()))
    })
}

/// The JSON body of an answer.
fn json_of(answer: &str) -> serde_json::Value {
    let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    serde_json::from_str(body).expect("JSON")
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream
}

/// Sends `request` as it is and reads the whole answer.
fn ask(port: u16, request: &str) -> String {
    let mut stream = connect(port);
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");
    answer
}

fn get(port: u16, target: &str) -> String {
    ask(
        port,
        &format!("GET {target} HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n"),
    )
}

fn status_of(answer: &str) -> &str {
    answer.lines().next().expect("a status line")
}

/// The next line of a stream; the stream staying open is part of every test that reads one.
fn line_of(lines: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    let read = lines.read_line(&mut line).expect("reads");
    assert!(read > 0, "the stream stays open");
    line
}

/// Opens the event stream and waits until the server says it is open, so a frame published after
/// this is one the stream will carry.
fn open_stream(port: u16, target: &str) -> BufReader<TcpStream> {
    let mut stream = connect(port);
    write!(stream, "GET {target} HTTP/1.1\r\nHost: zaf\r\n\r\n").expect("writes");
    let mut lines = BufReader::new(stream);
    loop {
        if line_of(&mut lines).starts_with(": connected") {
            return lines;
        }
    }
}

/// The next event: its id and its data line.
fn next_event(lines: &mut BufReader<TcpStream>) -> (String, String) {
    let (mut id, mut data) = (String::new(), String::new());
    loop {
        let line = line_of(lines);
        let line = line.trim_end();
        if let Some(value) = line.strip_prefix("id: ") {
            value.clone_into(&mut id);
        } else if let Some(value) = line.strip_prefix("data: ") {
            value.clone_into(&mut data);
        } else if line.is_empty() && !data.is_empty() {
            return (id, data);
        }
    }
}

/// The paths the page's HTML loads its script and style from.
fn assets_of(index: &str) -> (String, String) {
    let value_after = |marker: &str| {
        let start = index
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} in {index}"));
        let rest = &index[start + marker.len()..];
        rest[..rest.find('"').expect("a closing quote")].to_owned()
    };
    (
        value_after("<script type=\"module\" crossorigin src=\""),
        value_after("rel=\"stylesheet\" crossorigin href=\""),
    )
}

#[test]
fn serves_index() {
    let (_page, port) = started(FRAME);
    let answer = get(port, "/");
    assert_eq!(status_of(&answer), "HTTP/1.1 200 OK");
    assert!(
        answer.contains("Content-Type: text/html; charset=utf-8"),
        "{answer}"
    );
    let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    assert!(
        body.contains("<div id=\"app\"></div>"),
        "the app's mount: {body}"
    );
    assert!(
        body.contains("name=\"viewport\" content=\"width=device-width, initial-scale=1\""),
        "it works at phone width: {body}"
    );
    assert!(
        body.contains("name=\"color-scheme\" content=\"light dark\""),
        "it follows the system's theme: {body}"
    );
    assert_eq!(
        answer.split_once("\r\n\r\n").map(|(_, body)| body),
        Some(String::from_utf8_lossy(assets::find("/").expect("embedded").body).as_ref()),
        "the body is the embedded build"
    );
}

#[test]
fn serves_assets_with_types() {
    let (_page, port) = started(FRAME);
    let index = get(port, "/");
    let (script, style) = assets_of(&index);
    for (path, content_type) in [
        (script, "Content-Type: text/javascript; charset=utf-8"),
        (style, "Content-Type: text/css; charset=utf-8"),
    ] {
        let answer = get(port, &path);
        assert_eq!(status_of(&answer), "HTTP/1.1 200 OK", "{path}");
        assert!(answer.contains(content_type), "{path}: {answer}");
        let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        assert!(!body.is_empty(), "{path} has a body");
    }
}

#[test]
fn the_page_loads_nothing_from_outside() {
    let (_page, port) = started(FRAME);
    let answer = get(port, "/");
    let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    assert!(head.contains("default-src 'none'"), "{head}");
    assert!(head.contains("script-src 'self'"), "{head}");
    assert!(!head.contains("unsafe-inline"), "{head}");
    // The document names no other host: what it loads is its own script and style.
    for forbidden in ["http://", "https://", "@import", "<style", "<script>"] {
        assert!(
            !body.contains(forbidden),
            "{forbidden:?} in the page: {body}"
        );
    }
}

#[test]
fn a_file_is_not_found_outside_the_build() {
    let (_page, port) = started(FRAME);
    for path in ["/assets/other.js", "/assets/", "/index", "/../Cargo.toml"] {
        assert_eq!(
            status_of(&get(port, path)),
            "HTTP/1.1 404 Not Found",
            "{path}"
        );
    }
}

#[test]
fn a_non_get_is_refused() {
    let (page, port) = started(FRAME);
    let before = get(port, "/");
    let mut stream = open_stream(port, "/events");
    for method in ["POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"] {
        for target in ["/", "/events", "/anything"] {
            let answer = ask(
                port,
                &format!(
                    "{method} {target} HTTP/1.1\r\nHost: zaf\r\nContent-Length: 4\r\n\r\nbody"
                ),
            );
            assert_eq!(
                status_of(&answer),
                "HTTP/1.1 405 Method Not Allowed",
                "{method} {target}"
            );
            assert!(answer.contains("Allow: GET"), "{answer}");
        }
    }
    assert_eq!(get(port, "/"), before, "nothing changed");
    // And no event went to the stream: the next one is the one published now.
    page.publish("after\n");
    let (id, _) = next_event(&mut stream);
    assert_eq!(id, "1", "the refusals published nothing");
}

#[test]
fn other_requests_are_refused_by_name() {
    let (_page, port) = started(FRAME);
    assert_eq!(status_of(&get(port, "/missing")), "HTTP/1.1 404 Not Found");
    assert_eq!(
        status_of(&ask(port, "nonsense\r\n\r\n")),
        "HTTP/1.1 400 Bad Request"
    );
    assert_eq!(
        status_of(&ask(port, "GET http://elsewhere/ HTTP/1.1\r\n\r\n")),
        "HTTP/1.1 400 Bad Request"
    );
    let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD + 1));
    assert_eq!(
        status_of(&ask(port, &long)),
        "HTTP/1.1 431 Request Header Fields Too Large"
    );
}

#[test]
fn a_publish_sends_one_event_to_each_stream() {
    let (page, port) = started("first\n");
    let mut one = open_stream(port, "/events");
    let mut two = open_stream(port, "/events");
    page.publish("second\nline\n");
    for stream in [&mut one, &mut two] {
        let (id, data) = next_event(stream);
        assert_eq!(id, "1");
        assert_eq!(data, "\"second\\nline\\n\"", "one line of JSON");
    }
    page.publish("third\n");
    let (id, data) = next_event(&mut one);
    assert_eq!((id.as_str(), data.as_str()), ("2", "\"third\\n\""));
}

#[test]
fn a_stream_behind_the_page_catches_up() {
    let (page, port) = started("first\n");
    // A client that has the first frame, version 0, and opens its stream after a report.
    let version = 0;
    page.publish("second\n");
    let mut stream = open_stream(port, &format!("/events?since={version}"));
    let (id, data) = next_event(&mut stream);
    assert_eq!((id.as_str(), data.as_str()), ("1", "\"second\\n\""));

    // A client that is current is told nothing until the next frame.
    let mut current = open_stream(port, "/events?since=1");
    page.publish("third\n");
    assert_eq!(next_event(&mut current).0, "2");
}

#[test]
fn a_reconnecting_stream_sends_the_last_event_id() {
    let (page, port) = started("first\n");
    page.publish("second\n");
    let mut stream = connect(port);
    write!(
        stream,
        "GET /events?since=0 HTTP/1.1\r\nLast-Event-ID: 1\r\n\r\n"
    )
    .expect("writes");
    let mut lines = BufReader::new(stream);
    loop {
        if line_of(&mut lines).starts_with(": connected") {
            break;
        }
    }
    page.publish("third\n");
    assert_eq!(
        next_event(&mut lines).0,
        "2",
        "the header wins over the query: frame 1 was not sent again"
    );
}

#[test]
fn an_announce_sends_a_named_change_event() {
    let (page, port) = started(FRAME);
    let mut changes = open_stream(port, "/api/events");
    let mut frames = open_stream(port, "/events");
    page.announce(&Change {
        project: "/work/ways".into(),
        why: super::super::Why::Started,
    });
    assert_eq!(
        line_of(&mut changes),
        "\n",
        "the end of the opening comment"
    );
    assert_eq!(line_of(&mut changes), "event: change\n");
    assert_eq!(
        line_of(&mut changes),
        "data: {\"project\":\"/work/ways\",\"why\":\"started\"}\n"
    );
    // The text frame's stream is its own: a change is not a frame.
    page.publish("next\n");
    let (_, data) = next_event(&mut frames);
    assert_eq!(data, "\"next\\n\"");
}

#[test]
fn a_closed_stream_is_dropped() {
    let (page, port) = started("first\n");
    drop(open_stream(port, "/events"));
    for _ in 0..50 {
        page.publish("frame\n");
        if page.shared.state().subscribers.is_empty() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the closed stream is still a subscriber");
}

#[test]
fn connections_are_capped() {
    let (page, port) = started("first\n");
    let held: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| open_stream(port, "/events"))
        .collect();
    assert_eq!(
        page.shared.connections.load(Ordering::SeqCst),
        MAX_CONNECTIONS
    );
    assert_eq!(
        status_of(&get(port, "/")),
        "HTTP/1.1 503 Service Unavailable"
    );
    drop(held);
}

#[test]
fn a_used_port_is_named() {
    let held = Dashboard::bind(0).expect("binds");
    let error = Dashboard::bind(held.port()).expect_err("in use");
    assert!(matches!(error, BindError::InUse { port } if port == held.port()));
    assert_eq!(
        error.to_string(),
        format!("port {} is already in use", held.port())
    );
}

#[test]
fn it_binds_every_interface() {
    let dashboard = Dashboard::bind(0).expect("binds");
    let addr = dashboard.listener.local_addr().expect("address");
    assert!(addr.ip().is_unspecified(), "{addr}");
    assert_ne!(dashboard.port(), 0, "a port 0 gets a free port");
}

#[test]
fn the_url_is_the_address_and_the_port() {
    assert_eq!(
        lan_url(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)), 8080),
        "http://192.168.1.20:8080/"
    );
    assert_eq!(
        lan_url(IpAddr::V6(Ipv6Addr::LOCALHOST), 80),
        "http://[::1]:80/"
    );
    assert!(!lan_address().is_unspecified());
}

#[test]
fn state_route_answers_json() {
    let reads = Arc::new(AtomicUsize::new(0));
    let (_page, port) = started_with("frame\n", one_missing_project(&reads));
    let answer = get(port, "/api/state");
    assert_eq!(status_of(&answer), "HTTP/1.1 200 OK");
    assert!(
        answer.contains("Content-Type: application/json\r\n"),
        "{answer}"
    );
    assert!(answer.contains("Cache-Control: no-store"), "{answer}");
    let json = json_of(&answer);
    let mut keys: Vec<_> = json.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["controls", "machine", "next", "projects"]);
    assert_eq!(json["controls"], true, "a hub that serves controls says so");
    assert_eq!(json["machine"]["projects"], 1);
    assert_eq!(json["projects"][0]["project"], "gone");
    assert_eq!(json["projects"][0]["missing"], true);
    // What waits on the Engineer comes from the same read: the project that cannot run.
    assert_eq!(json["next"][0]["kind"], "broken");
    assert_eq!(json["next"][0]["project"], "gone");
    // A query changes nothing about the route, and each request is read again.
    assert_eq!(status_of(&get(port, "/api/state?x=1")), "HTTP/1.1 200 OK");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[test]
fn state_route_refuses_post() {
    let reads = Arc::new(AtomicUsize::new(0));
    let (page, port) = started_with("frame\n", one_missing_project(&reads));
    let before = get(port, "/");
    let mut stream = open_stream(port, "/events");
    for method in ["POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"] {
        let answer = ask(
            port,
            &format!("{method} /api/state HTTP/1.1\r\nHost: zaf\r\nContent-Length: 4\r\n\r\nbody"),
        );
        assert_eq!(
            status_of(&answer),
            "HTTP/1.1 405 Method Not Allowed",
            "{method}"
        );
        assert!(answer.contains("Allow: GET"), "{answer}");
        assert!(
            answer.contains("Content-Type: application/json"),
            "{answer}"
        );
        assert_eq!(
            json_of(&answer),
            serde_json::json!({ "refused": "GET only" }),
            "the refusal is one field"
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0, "a refusal reads nothing");
    assert_eq!(get(port, "/"), before, "nothing changed");
    page.publish("after\n");
    assert_eq!(next_event(&mut stream).0, "1", "and nothing was published");
}

#[test]
fn a_state_that_cannot_be_read_is_a_refusal() {
    let (_page, port) = started("frame\n");
    let answer = get(port, "/api/state");
    assert_eq!(status_of(&answer), "HTTP/1.1 503 Service Unavailable");
    assert!(
        answer.contains("Content-Type: application/json"),
        "{answer}"
    );
    assert_eq!(
        json_of(&answer),
        serde_json::json!({ "refused": "not read in this test" })
    );
}

#[test]
fn an_unknown_api_route_is_a_json_refusal() {
    let (_page, port) = started("frame\n");
    for target in ["/api", "/api/", "/api/nothing", "/api/event"] {
        let answer = get(port, target);
        assert_eq!(status_of(&answer), "HTTP/1.1 404 Not Found", "{target}");
        assert_eq!(
            json_of(&answer),
            serde_json::json!({ "refused": "not found" }),
            "{target}"
        );
    }
}

#[test]
fn requests_that_wait_for_a_read_share_it() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let state: Arc<StateReader> = Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(300));
        Err("slow".to_owned())
    });
    let (_page, port) = started_with("frame\n", state);
    let askers: Vec<_> = (0..8)
        .map(|_| thread::spawn(move || get(port, "/api/state")))
        .collect();
    for asker in askers {
        let answer = asker.join().expect("answered");
        assert_eq!(status_of(&answer), "HTTP/1.1 503 Service Unavailable");
    }
    assert!(
        reads.load(Ordering::SeqCst) <= 2,
        "eight requests at once are not eight reads: {}",
        reads.load(Ordering::SeqCst)
    );
}
