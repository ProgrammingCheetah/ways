use std::cell::Cell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::app::hub::{EpicRow, Report, record};
use crate::cli::run;

fn snapshot(epic: u64) -> Snapshot {
    Snapshot {
        epics: vec![EpicRow {
            number: epic,
            ..EpicRow::default()
        }],
    }
}

fn report(state_dir: &Path) {
    record(
        state_dir,
        &Report {
            at: UNIX_EPOCH,
            task: None,
            worker: "team".to_owned(),
            what: "working".to_owned(),
        },
    );
}

/// A viewer that does nothing for `looks` looks.
struct Idle(usize);

impl Pace for Idle {
    fn now(&mut self) -> SystemTime {
        UNIX_EPOCH
    }

    fn width(&mut self) -> usize {
        100
    }

    fn wait(&mut self) -> bool {
        self.0 = self.0.saturating_sub(1);
        self.0 > 0
    }
}

/// A viewer with a page open on the stream: it makes a report at one look, and reads at the next
/// what the stream was sent, which is by then everything the report caused.
struct Watcher<'a> {
    state_dir: &'a Path,
    port: u16,
    stream: Option<BufReader<TcpStream>>,
    looks: usize,
    events: Vec<String>,
}

impl Watcher<'_> {
    fn open(&self) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(stream, "GET /events HTTP/1.1\r\nHost: zaf\r\n\r\n").expect("writes");
        let mut lines = BufReader::new(stream);
        loop {
            let mut line = String::new();
            assert!(lines.read_line(&mut line).expect("reads") > 0, "open");
            if line.starts_with(": connected") {
                return lines;
            }
        }
    }

    /// The next event's data line.
    fn event(&mut self) -> Option<String> {
        let stream = self.stream.as_mut()?;
        loop {
            let mut line = String::new();
            if stream.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if let Some(data) = line.trim_end().strip_prefix("data: ") {
                return Some(data.to_owned());
            }
        }
    }
}

impl Pace for Watcher<'_> {
    fn now(&mut self) -> SystemTime {
        UNIX_EPOCH
    }

    fn width(&mut self) -> usize {
        100
    }

    fn wait(&mut self) -> bool {
        self.looks += 1;
        match self.looks {
            1 => self.stream = Some(self.open()),
            2 => report(self.state_dir),
            3 => {
                let event = self.event().expect("the report's event");
                self.events.push(event);
                // From here the stream is waited on for a quarter of a second at a time.
                if let Some(stream) = &self.stream {
                    stream
                        .get_ref()
                        .set_read_timeout(Some(Duration::from_millis(300)))
                        .expect("timeout");
                }
            }
            4 | 5 => {}
            6 => {
                if let Some(event) = self.event() {
                    self.events.push(event);
                }
            }
            _ => return false,
        }
        true
    }
}

fn fetch(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");
    answer
}

#[test]
fn hub_port_prints_the_lan_url() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dashboard = dashboard_on_a_free_port();
    let port = dashboard.port();
    let address = IpAddr::V4(Ipv4Addr::new(192, 168, 7, 20));
    let mut out = Vec::new();
    serve(
        &mut out,
        dashboard,
        address,
        |_| Ok(snapshot(103)),
        dir.path(),
        &mut Idle(2),
    )
    .expect("serves");
    assert_eq!(
        String::from_utf8(out).expect("text"),
        format!("http://192.168.7.20:{port}/\n"),
        "port 0 prints the port it got"
    );
    assert_ne!(port, 0);
}

#[test]
fn the_first_frame_is_the_page() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dashboard = dashboard_on_a_free_port();
    let port = dashboard.port();
    let seen = Cell::new(String::new());
    let mut pace = Fetching {
        port,
        seen: &seen,
        looks: 0,
    };
    serve(
        &mut Vec::new(),
        dashboard,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        |_| Ok(snapshot(103)),
        dir.path(),
        &mut pace,
    )
    .expect("serves");
    let answer = seen.take();
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(answer.contains("epic #103"), "{answer}");
}

/// Gets the page at its second look.
struct Fetching<'a> {
    port: u16,
    seen: &'a Cell<String>,
    looks: usize,
}

impl Pace for Fetching<'_> {
    fn now(&mut self) -> SystemTime {
        UNIX_EPOCH
    }

    fn width(&mut self) -> usize {
        100
    }

    fn wait(&mut self) -> bool {
        self.looks += 1;
        if self.looks == 1 {
            self.seen.set(fetch(
                self.port,
                "GET / HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
            ));
        }
        self.looks < 2
    }
}

fn dashboard_on_a_free_port() -> Dashboard {
    Dashboard::bind(0).expect("binds")
}

#[test]
fn a_report_sends_one_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dashboard = dashboard_on_a_free_port();
    let calls = Cell::new(0_u64);
    let mut watcher = Watcher {
        state_dir: dir.path(),
        port: dashboard.port(),
        stream: None,
        looks: 0,
        events: Vec::new(),
    };
    serve(
        &mut Vec::new(),
        dashboard,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(100 + calls.get()))
        },
        dir.path(),
        &mut watcher,
    )
    .expect("serves");
    assert_eq!(calls.get(), 2, "the first frame, and the report's");
    assert_eq!(watcher.events.len(), 1, "one report, one event");
    let frame: String = serde_json::from_str(&watcher.events[0]).expect("a JSON string");
    assert_eq!(
        frame,
        draw(&snapshot(102), 100),
        "the board as it was after"
    );
}

#[test]
fn no_board_between_reports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    serve(
        &mut Vec::new(),
        dashboard_on_a_free_port(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(103))
        },
        dir.path(),
        &mut Idle(300),
    )
    .expect("serves");
    assert_eq!(calls.get(), 1, "only the first frame read the board");
}

#[test]
fn a_failing_board_keeps_the_last_frame_and_the_server() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    let dashboard = dashboard_on_a_free_port();
    let mut pace = Reporting {
        state_dir: dir.path(),
        looks: 0,
    };
    let outcome = serve(
        &mut Vec::new(),
        dashboard,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        |_| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Ok(snapshot(103))
            } else {
                Err(crate::app::hub::HubError::Board(
                    crate::ports::PortError::Unavailable("down".to_owned()),
                ))
            }
        },
        dir.path(),
        &mut pace,
    );
    assert!(outcome.is_ok(), "the server lives on");
    assert_eq!(
        calls.get(),
        2,
        "tried once for the report, not at every look"
    );
}

/// Makes a report at the first look and looks on until the fifth.
struct Reporting<'a> {
    state_dir: &'a Path,
    looks: usize,
}

impl Pace for Reporting<'_> {
    fn now(&mut self) -> SystemTime {
        UNIX_EPOCH
    }

    fn width(&mut self) -> usize {
        100
    }

    fn wait(&mut self) -> bool {
        self.looks += 1;
        if self.looks == 1 {
            report(self.state_dir);
        }
        self.looks < 5
    }
}

#[test]
fn a_used_port_exits_1() {
    let held = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("binds");
    let port = held.local_addr().expect("address").port().to_string();
    assert_eq!(run(["zaf", "hub", "--port", port.as_str()]), REFUSED);
}

#[test]
fn a_used_port_is_refused_not_failed() {
    let held = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("binds");
    let port = held.local_addr().expect("address").port();
    assert_eq!(bind(port, "hub").err(), Some(REFUSED));
}

#[test]
fn the_port_is_a_number_and_not_with_the_hook() {
    assert_eq!(run(["zaf", "hub", "--port", "http"]), 2);
    assert_eq!(run(["zaf", "hub", "--port", "70000"]), 2);
    assert_eq!(run(["zaf", "hub", "report", "--port", "0"]), 2);
}
