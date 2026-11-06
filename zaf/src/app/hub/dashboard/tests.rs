use std::io::{BufRead, BufReader};
use std::net::Ipv6Addr;

use super::*;

const FRAME: &str = "epic #103   [###---]  1 of 4\n  1 of 2 slots free <b>&</b>\n";

fn started(frame: &str) -> (Page, u16) {
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    (dashboard.start(frame), port)
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

#[test]
fn get_root_returns_the_frame() {
    let (_page, port) = started(FRAME);
    let answer = get(port, "/");
    assert_eq!(status_of(&answer), "HTTP/1.1 200 OK");
    assert!(answer.contains("Content-Type: text/html"), "{answer}");
    assert!(
        answer.contains("epic #103   [###---]  1 of 4\n"),
        "the frame, as text: {answer}"
    );
    assert!(
        answer.contains("&lt;b&gt;&amp;&lt;/b&gt;"),
        "the frame is text, never markup: {answer}"
    );
    assert!(!answer.contains("<b>"), "{answer}");
}

#[test]
fn the_page_shows_the_newest_frame() {
    let (page, port) = started("old\n");
    page.publish("new frame\n");
    let answer = get(port, "/");
    assert!(answer.contains("new frame"), "{answer}");
    assert!(!answer.contains("old"), "{answer}");
}

#[test]
fn the_page_loads_nothing_from_outside() {
    let (_page, port) = started(FRAME);
    let answer = get(port, "/");
    let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    assert!(head.contains("default-src 'none'"), "{head}");
    for forbidden in [
        "http://", "https://", "//", " src=", " href=", "@import", "url(",
    ] {
        assert!(
            !body.contains(forbidden),
            "{forbidden:?} in the page: {body}"
        );
    }
    assert!(body.contains("new EventSource('/events"), "{body}");
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
    let version = {
        let answer = get(port, "/");
        assert!(answer.contains("data-version=\"0\""), "{answer}");
        0
    };
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
