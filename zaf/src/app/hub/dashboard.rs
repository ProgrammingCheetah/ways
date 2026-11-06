//! The hub's dashboard mode (docs/zaf-actions.md §5, ruling 43): the frame as an HTML page, served
//! on every interface to anyone on the LAN.
//!
//! The server answers GET only, asks for no login and changes nothing. `GET /` is the page with
//! the current frame; `GET /events` is a server-sent events stream that carries one event for each
//! frame published, which is each hub report. It is the standard library's [`TcpListener`] with a
//! thread for each connection, so zaf gains no dependency. The page loads nothing from outside the
//! server: its style and script are in it.
//!
//! The server holds the last frame and reads nothing else: no board, no network and no file. What
//! reads the board when a report arrives is the caller's, which hands each new frame to
//! [`Page::publish`].

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// The most connections served at once; a connection past it is answered 503 and closed.
const MAX_CONNECTIONS: usize = 64;

/// The most bytes a request's head may take.
const MAX_HEAD: usize = 8 * 1024;

/// How long a client has to send its request's head.
const HEAD_DEADLINE: Duration = Duration::from_secs(5);

/// How long a write may wait for a client that does not read.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long an event stream stays silent before it writes a comment, which finds a client that
/// has gone.
const KEEPALIVE: Duration = Duration::from_secs(15);

/// Why the dashboard could not listen.
#[derive(Debug, thiserror::Error)]
pub enum BindError {
    /// Another process holds the port.
    #[error("port {port} is already in use")]
    InUse {
        /// The port asked for.
        port: u16,
    },
    /// The port could not be bound for another reason, such as a port the user may not use.
    #[error("cannot listen on port {port}: {source}")]
    Failed {
        /// The port asked for.
        port: u16,
        /// What the system said.
        source: io::Error,
    },
}

/// A socket bound to every interface, not yet serving.
#[derive(Debug)]
pub struct Dashboard {
    listener: TcpListener,
}

impl Dashboard {
    /// Binds every interface on `port`; port 0 takes a free one.
    pub fn bind(port: u16) -> Result<Self, BindError> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).map_err(|source| {
            if source.kind() == io::ErrorKind::AddrInUse {
                BindError::InUse { port }
            } else {
                BindError::Failed { port, source }
            }
        })?;
        Ok(Self { listener })
    }

    /// The port it listens on: the one asked for, or the free one a port 0 got.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map_or(0, |addr| addr.port())
    }

    /// Starts serving with `frame` as the current one. The server runs on its own threads until
    /// the process ends.
    pub fn start(self, frame: &str) -> Page {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                current: Arc::new(Message {
                    version: 0,
                    frame: frame.to_owned(),
                }),
                subscribers: Vec::new(),
            }),
            connections: AtomicUsize::new(0),
        });
        let accepting = Arc::clone(&shared);
        // A thread that cannot start leaves a page nobody can read; the caller finds out at its
        // first request, as it would from any listener that has stopped.
        let _ = thread::Builder::new()
            .name("zaf-hub-accept".to_owned())
            .spawn(move || accept(&self.listener, &accepting));
        Page { shared }
    }
}

/// The running server, seen from the code that reads the board.
#[derive(Debug, Clone)]
pub struct Page {
    shared: Arc<Shared>,
}

impl Page {
    /// Makes `frame` the current one and sends it as one event to every open stream.
    pub fn publish(&self, frame: &str) {
        let mut state = self.shared.state();
        let message = Arc::new(Message {
            version: state.current.version + 1,
            frame: frame.to_owned(),
        });
        state.current = Arc::clone(&message);
        state
            .subscribers
            .retain(|subscriber| subscriber.send(Arc::clone(&message)).is_ok());
    }
}

/// The address of this machine that the LAN reaches it by: the one the system would send from.
/// The system picks it from its routes and sends nothing; with no route it is the loopback.
pub fn lan_address() -> IpAddr {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| {
            // TEST-NET-1: never routed, and a UDP connect only reads the route table.
            socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9))?;
            socket.local_addr()
        })
        .map(|addr| addr.ip())
        .ok()
        .filter(|ip| !ip.is_unspecified())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

/// The page's URL on `address` at `port`.
pub fn lan_url(address: IpAddr, port: u16) -> String {
    format!("http://{}/", SocketAddr::new(address, port))
}

/// One frame and its place in the order of the frames this server has made.
#[derive(Debug)]
struct Message {
    version: u64,
    frame: String,
}

#[derive(Debug)]
struct State {
    current: Arc<Message>,
    subscribers: Vec<Sender<Arc<Message>>>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    connections: AtomicUsize,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens a stream. A client that already has frame `since` hears the current one at once
    /// when it is newer, so a report between its page and its stream is not lost.
    fn subscribe(&self, since: Option<u64>) -> Receiver<Arc<Message>> {
        let (send, receive) = mpsc::channel();
        let mut state = self.state();
        if since.is_some_and(|since| since < state.current.version) {
            let _ = send.send(Arc::clone(&state.current));
        }
        state.subscribers.push(send);
        receive
    }
}

/// Holds one of the connection slots until it is dropped.
struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            // Out of descriptors or the like: wait, rather than spin.
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let taken = shared.connections.fetch_add(1, Ordering::SeqCst);
        let slot = Slot(Arc::clone(shared));
        // Past twice the cap a client is dropped unanswered; between the cap and that it is told
        // to come back, after its request has been read so the answer is not lost to a reset.
        if taken >= MAX_CONNECTIONS * 2 {
            continue;
        }
        let busy = taken >= MAX_CONNECTIONS;
        let shared = Arc::clone(shared);
        // A thread that cannot start drops its slot and its stream.
        let _ = thread::Builder::new()
            .name("zaf-hub-connection".to_owned())
            .spawn(move || {
                let _slot = slot;
                let _ = if busy {
                    refuse(stream)
                } else {
                    serve(stream, &shared)
                };
            });
    }
}

/// What a client asked for.
struct Request {
    method: String,
    path: String,
    query: String,
    last_event_id: Option<u64>,
}

/// What reading a request's head came to.
enum Head {
    Request(Request),
    Malformed,
    TooLarge,
    /// The client left, or was too slow.
    Gone,
}

/// Answers a client the server has no room for.
fn refuse(mut stream: TcpStream) -> io::Result<()> {
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    // Whatever the head says, and however it ends, the answer is the same.
    let _ = read_head(&mut stream);
    respond(&mut stream, "503 Service Unavailable", &[], "busy\n")
}

fn serve(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let request = match read_head(&mut stream) {
        Head::Request(request) => request,
        Head::Malformed => return respond(&mut stream, "400 Bad Request", &[], "bad request\n"),
        Head::TooLarge => {
            return respond(
                &mut stream,
                "431 Request Header Fields Too Large",
                &[],
                "too large\n",
            );
        }
        Head::Gone => return Ok(()),
    };
    // The method comes first: whatever is asked of whatever path, only GET is answered, and
    // nothing is changed by the refusal.
    if request.method != "GET" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            &[("Allow", "GET")],
            "GET only\n",
        );
    }
    match request.path.as_str() {
        "/" => {
            let current = Arc::clone(&shared.state().current);
            respond_html(&mut stream, &page(&current))
        }
        "/events" => {
            let since = request.last_event_id.or_else(|| {
                request
                    .query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("since="))
                    .and_then(|value| value.parse().ok())
            });
            stream_events(&mut stream, &shared.subscribe(since))
        }
        _ => respond(&mut stream, "404 Not Found", &[], "not found\n"),
    }
}

fn read_head(stream: &mut TcpStream) -> Head {
    let started = Instant::now();
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Some(left) = HEAD_DEADLINE.checked_sub(started.elapsed()) else {
            return Head::Gone;
        };
        if stream.set_read_timeout(Some(left)).is_err() {
            return Head::Gone;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Head::Gone,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if end > MAX_HEAD {
                return Head::TooLarge;
            }
            return parse_head(&String::from_utf8_lossy(&bytes[..end]));
        }
        if bytes.len() > MAX_HEAD {
            return Head::TooLarge;
        }
    }
}

fn parse_head(head: &str) -> Head {
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Head::Malformed;
    };
    if method.is_empty() || !target.starts_with('/') || !version.starts_with("HTTP/1.") {
        return Head::Malformed;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let last_event_id = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("last-event-id"))
        .and_then(|(_, value)| value.trim().parse().ok());
    Head::Request(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        last_event_id,
    })
}

/// Writes a whole response and closes it. Every response carries the headers that keep a browser
/// from caching or guessing at it.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    respond_as(stream, status, "text/plain; charset=utf-8", headers, body)
}

fn respond_html(stream: &mut TcpStream, body: &str) -> io::Result<()> {
    // The page loads nothing from outside: its style and script are inline, and the one thing it
    // opens is this server's event stream.
    respond_as(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        &[(
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'",
        )],
        body,
    )
}

fn respond_as(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// Writes the stream's head and then each event as it comes. The opening comment is not an event:
/// it tells a client the stream is open. Returns when the client has gone.
fn stream_events(stream: &mut TcpStream, events: &Receiver<Arc<Message>>) -> io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\
          X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n: connected\n\n",
    )?;
    stream.flush()?;
    loop {
        match events.recv_timeout(KEEPALIVE) {
            Ok(message) => {
                // One line of JSON: the frame's own newlines and quotes are escaped in it.
                let data = serde_json::to_string(&message.frame).unwrap_or_default();
                write!(stream, "id: {}\ndata: {data}\n\n", message.version)?;
            }
            Err(RecvTimeoutError::Timeout) => stream.write_all(b": keepalive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        stream.flush()?;
    }
}

/// The page: the frame as text, and a script that replaces it with each event.
fn page(current: &Message) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>zaf hub</title>\n<style>\n\
         body {{ margin: 0; padding: 1rem; background: #14161a; color: #d8dce2; }}\n\
         pre {{ margin: 0; overflow-x: auto; font: 14px/1.4 ui-monospace, Menlo, Consolas, monospace; }}\n\
         </style>\n</head>\n<body>\n<pre id=\"frame\" data-version=\"{version}\">{frame}</pre>\n\
         <script>\n(function () {{\n\
         \x20 var frame = document.getElementById('frame');\n\
         \x20 var source = new EventSource('/events?since=' + frame.dataset.version);\n\
         \x20 source.onmessage = function (event) {{ frame.textContent = JSON.parse(event.data); }};\n\
         }})();\n</script>\n</body>\n</html>\n",
        version = current.version,
        frame = escape(&current.frame),
    )
}

/// `text` as HTML text: nothing in it is markup.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests;
