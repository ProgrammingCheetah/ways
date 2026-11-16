//! The hub's dashboard mode (docs/zaf-actions.md §5, ruling 43): the frame as an HTML page, served
//! on every interface to anyone on the LAN.
//!
//! Reading is GET with no login and changes nothing. `GET /` is the page, a Svelte application
//! built by Vite into `zaf/web/dist` and embedded in the binary ([`assets`]), and its script and
//! style are served beside it; `GET /events` is a server-sent events stream that carries one event
//! for each frame published, which is each hub report; `GET /api/events` is a second stream, of
//! `change` events (`project` and `why`), one for each [`Page::announce`]; `GET /api/state` is the
//! machine state as JSON (the hub's JSON interface, docs/zaf-actions.md §5). A method other than
//! GET is refused 405 and changes nothing, but for `POST /api/<control>`: it is a control, which
//! the [`Guard`] admits only with the key the hub made at start, before the request is given any
//! work, and refuses 403 otherwise. It is the standard library's [`TcpListener`] with a thread for
//! each connection, so zaf gains no dependency. The page loads nothing from outside the server.
//!
//! The server holds the last frame. What reads the board when a report arrives is the caller's,
//! which hands each new frame to [`Page::publish`]. The one other read is the machine state,
//! which a `GET /api/state` asks of the [`StateReader`] the caller gave at [`Dashboard::start`],
//! once for the requests that arrive while it runs.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::Change;
use super::controls::Controls;
use super::guard::{Guard, KEY_HEADER};
use super::next::{NextItem, next_for_you};
use super::state::{MachineLine, MachineState, ProjectState};

mod assets;

/// What reads the machine state for a request to `GET /api/state`: every project on the zaf home's
/// list, as [`gather_machine`](super::state::gather_machine) reads them. It fails only when there
/// is nothing to read, such as no zaf home, and says why in plain words.
pub type StateReader = dyn Fn() -> Result<MachineState, String> + Send + Sync;

/// The most connections served at once; a connection past it is answered 503 and closed.
const MAX_CONNECTIONS: usize = 64;

/// The most bytes a request's head may take.
const MAX_HEAD: usize = 8 * 1024;

/// How long a client has to send its request's head.
const HEAD_DEADLINE: Duration = Duration::from_secs(5);

/// The most bytes a control's body may take.
const MAX_BODY: usize = 4 * 1024;

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

    /// Starts serving with `frame` as the current one, `state` to read the machine state for
    /// `GET /api/state` and `guard` to admit the controls. The server runs on its own threads
    /// until the process ends.
    pub fn start(self, frame: &str, state: Arc<StateReader>, guard: Guard) -> Page {
        self.start_with(frame, state, guard, Controls::default())
    }

    /// [`start`](Self::start) with the `controls` a keyed `POST` to `/api/<control>` runs.
    pub fn start_with(
        self,
        frame: &str,
        state: Arc<StateReader>,
        guard: Guard,
        controls: Controls,
    ) -> Page {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                current: Arc::new(Message {
                    version: 0,
                    frame: frame.to_owned(),
                }),
                subscribers: Vec::new(),
                watchers: Vec::new(),
            }),
            connections: AtomicUsize::new(0),
            reader: state,
            gathered: Mutex::new(None),
            guard,
            controls,
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

    /// Sends `change` as one `change` event to every open `/api/events` stream.
    pub fn announce(&self, change: &Change) {
        let data = serde_json::json!({
            "project": change.project.to_string_lossy(),
            "why": change.why.as_str(),
        })
        .to_string();
        let data = Arc::new(data);
        self.shared
            .state()
            .watchers
            .retain(|watcher| watcher.send(Arc::clone(&data)).is_ok());
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
    /// The open `/api/events` streams, each sent the JSON data of every change.
    watchers: Vec<Sender<Arc<String>>>,
}

/// The last read of the machine state: when it began and what it answered, as the body of the
/// response or the reason it could not be read.
type Gathered = (Instant, Result<Arc<String>, String>);

struct Shared {
    state: Mutex<State>,
    connections: AtomicUsize,
    reader: Arc<StateReader>,
    /// The last read of the machine state; held while one runs, so reads go one at a time.
    gathered: Mutex<Option<Gathered>>,
    /// What admits a control: the hub's key, kept here and nowhere else.
    guard: Guard,
    /// What the controls run on.
    controls: Controls,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("state", &self.state)
            .field("connections", &self.connections)
            .finish_non_exhaustive()
    }
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

    /// Opens a change stream.
    fn watch(&self) -> Receiver<Arc<String>> {
        let (send, receive) = mpsc::channel();
        self.state().watchers.push(send);
        receive
    }

    /// The machine state for a request that arrived at `asked`, as the JSON body of the answer.
    /// Reads go one at a time, and a read that began after the request arrived answers every
    /// request that waited for it, so a crowd of requests costs the boards one read, not one each.
    fn machine_state(&self, asked: Instant) -> Result<Arc<String>, String> {
        let mut gathered = self.gathered.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((began, answer)) = gathered.as_ref()
            && *began >= asked
        {
            return answer.clone();
        }
        let began = Instant::now();
        let controls = self.guard.controls();
        let answer = (self.reader)().and_then(|state| api_state(&state, controls).map(Arc::new));
        *gathered = Some((began, answer.clone()));
        answer
    }
}

/// The body of `GET /api/state`: `machine` and `projects` as the machine state holds them, `next`,
/// the Next for you list derived from the same read, and `controls`, whether this hub serves
/// controls (false for `--read-only`), which the page reads to show its buttons or not.
fn api_state(state: &MachineState, controls: bool) -> Result<String, String> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        controls: bool,
        machine: &'a MachineLine,
        next: Vec<NextItem>,
        projects: &'a [ProjectState],
    }
    serde_json::to_string(&Body {
        controls,
        machine: &state.machine,
        next: next_for_you(state),
        projects: &state.projects,
    })
    .map_err(|error| format!("the state cannot be written as JSON: {error}"))
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
    /// What the [`KEY_HEADER`] carried, when the request had one.
    key: Option<String>,
    /// The `Content-Length` the request declared, when it did.
    content_length: Option<usize>,
    /// The bytes of the body that came with the head.
    body: Vec<u8>,
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
    let asked = Instant::now();
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
    // nothing is changed by the refusal; a control is the one POST, and the guard comes before
    // any of its work.
    if request.method != "GET" {
        if request.method == "POST" && is_control(&request.path) {
            return control(&mut stream, shared, &request);
        }
        let allow = [("Allow", "GET")];
        return if is_api(&request.path) {
            respond_refused(&mut stream, "405 Method Not Allowed", &allow, "GET only")
        } else {
            respond(&mut stream, "405 Method Not Allowed", &allow, "GET only\n")
        };
    }
    match request.path.as_str() {
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
        "/api/events" => stream_changes(&mut stream, &shared.watch()),
        "/api/state" => match shared.machine_state(asked) {
            Ok(body) => respond_as(&mut stream, "200 OK", JSON, &[], body.as_bytes()),
            Err(reason) => respond_refused(&mut stream, "503 Service Unavailable", &[], &reason),
        },
        path if is_api(path) => respond_refused(&mut stream, "404 Not Found", &[], "not found"),
        path => match assets::find(path) {
            Some(asset) => respond_asset(&mut stream, &asset),
            None => respond(&mut stream, "404 Not Found", &[], "not found\n"),
        },
    }
}

/// The content type of every answer of the JSON interface.
const JSON: &str = "application/json";

/// Whether `path` is under the JSON interface, whose every answer is JSON.
fn is_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// The routes under `/api/` that answer GET, and so are not controls.
const READ_ROUTES: [&str; 2] = ["/api/state", "/api/events"];

/// Whether a `POST` to `path` is a control: any path under `/api/` that is not a read. A control
/// no task has built yet is a control all the same, so the key is asked for before the route is.
fn is_control(path: &str) -> bool {
    is_api(path) && !READ_ROUTES.contains(&path)
}

/// Answers a `POST` to a control. The guard is asked first, with nothing read of the body and no
/// route looked up: a missing or wrong key is 403 and changes nothing, for a route that exists and
/// for one that does not alike.
fn control(stream: &mut TcpStream, shared: &Shared, request: &Request) -> io::Result<()> {
    if let Err(refusal) = shared.guard.admit(request.key.as_deref()) {
        return respond_refused(stream, "403 Forbidden", &[], &refusal.to_string());
    }
    // Each control is a route of `Controls`; a keyed request to a path none answers is an unknown
    // route, as for any other path under `/api/`. The body is read only for a route that is built.
    if !shared.controls.has(&request.path) {
        return respond_refused(stream, "404 Not Found", &[], "not found");
    }
    let body = match read_body(stream, request) {
        Ok(body) => body,
        Err((status, why)) => return respond_refused(stream, status, &[], why),
    };
    match shared.controls.run(&request.path, &body) {
        Some(answer) => respond_as(stream, answer.status, JSON, &[], answer.body.as_bytes()),
        None => respond_refused(stream, "404 Not Found", &[], "not found"),
    }
}

/// The body of a control's request: `Content-Length` bytes, of which the head's read may already
/// hold some, read within the head's deadline. A body with no length, or one past [`MAX_BODY`], is
/// refused, and so is one that does not arrive.
fn read_body(
    stream: &mut TcpStream,
    request: &Request,
) -> Result<Vec<u8>, (&'static str, &'static str)> {
    let Some(length) = request.content_length else {
        return Err(("411 Length Required", "a control needs a Content-Length"));
    };
    if length > MAX_BODY {
        return Err(("413 Payload Too Large", "the body is too large"));
    }
    let late = ("408 Request Timeout", "the body did not arrive");
    let mut body = request.body.clone();
    body.truncate(length);
    let started = Instant::now();
    let mut chunk = [0_u8; 1024];
    while body.len() < length {
        let left = HEAD_DEADLINE.checked_sub(started.elapsed()).ok_or(late)?;
        stream.set_read_timeout(Some(left)).map_err(|_| late)?;
        let want = chunk.len().min(length - body.len());
        match stream.read(&mut chunk[..want]) {
            Ok(0) | Err(_) => return Err(late),
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    Ok(body)
}

/// The refusal of the JSON interface: an object with one field, `refused`, in plain words.
fn respond_refused(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    why: &str,
) -> io::Result<()> {
    let body = serde_json::json!({ "refused": why }).to_string();
    respond_as(stream, status, JSON, headers, body.as_bytes())
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
            return match parse_head(&String::from_utf8_lossy(&bytes[..end])) {
                Head::Request(request) => Head::Request(Request {
                    body: bytes[end + 4..].to_vec(),
                    ..request
                }),
                other => other,
            };
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
    let headers: Vec<(&str, &str)> = lines.filter_map(|line| line.split_once(':')).collect();
    let header = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.trim().eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.trim())
    };
    let last_event_id = header("last-event-id").and_then(|value| value.parse().ok());
    let key = header(KEY_HEADER).map(str::to_owned);
    let content_length = header("content-length").and_then(|value| value.parse().ok());
    Head::Request(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        last_event_id,
        key,
        content_length,
        body: Vec::new(),
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
    respond_as(
        stream,
        status,
        "text/plain; charset=utf-8",
        headers,
        body.as_bytes(),
    )
}

/// Answers with one of the embedded files. The page loads nothing from outside: its script and
/// style are files of this server, and what it opens is this server's own routes.
fn respond_asset(stream: &mut TcpStream, asset: &assets::Asset) -> io::Result<()> {
    respond_as(
        stream,
        "200 OK",
        asset.content_type,
        &[("Content-Security-Policy", assets::POLICY)],
        asset.body,
    )
}

fn respond_as(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
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
    stream.write_all(body)?;
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

/// Writes the change stream's head and then each change as a `change` event. Returns when the
/// client has gone.
fn stream_changes(stream: &mut TcpStream, changes: &Receiver<Arc<String>>) -> io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\
          X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n: connected\n\n",
    )?;
    stream.flush()?;
    loop {
        match changes.recv_timeout(KEEPALIVE) {
            Ok(data) => write!(stream, "event: change\ndata: {data}\n\n")?,
            Err(RecvTimeoutError::Timeout) => stream.write_all(b": keepalive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        stream.flush()?;
    }
}

#[cfg(test)]
mod tests;
