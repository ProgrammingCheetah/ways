use super::*;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use crate::domain::Tick;

fn spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.to_owned(),
        description: format!("the {name} verb"),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

fn request(name: &str, arguments: Value) -> CallToolRequestParams {
    let request = CallToolRequestParams::new(name.to_owned());
    match arguments {
        Value::Object(map) => request.with_arguments(map),
        _ => request,
    }
}

/// A host that lists two tools and answers every call with its tool, args and flags.
struct Echo;

impl ToolHost for Echo {
    fn list(&self) -> Vec<ToolSpec> {
        vec![spec("status"), spec("board")]
    }

    fn call(&self, call: ToolCall, _progress: &dyn Fn(Tick), _cancel: &Cancel) -> ToolResult {
        ToolResult {
            text: format!("{call:?}"),
            is_error: false,
            exit: 0,
        }
    }
}

#[test]
fn instructions_within_limit() {
    assert!(
        INSTRUCTIONS.chars().count() <= INSTRUCTIONS_LIMIT,
        "{} characters",
        INSTRUCTIONS.chars().count()
    );
    assert!(INSTRUCTIONS.contains(
        "Wherever a page says `zaf <verb> <words>`, call the tool `<verb>` (`mcp__zaf__<verb>`) \
         with `args` set to those words. Never type `zaf` in Bash; it is denied."
    ));
    let server = McpServer::new(Arc::new(Echo));
    let info = server.get_info();
    assert_eq!(info.instructions.as_deref(), Some(INSTRUCTIONS));
    assert!(info.capabilities.tools.is_some());
    assert_eq!(info.server_info.name, "zaf");
}

#[test]
fn the_tools_are_the_hosts_listing() {
    let tools = McpServer::new(Arc::new(Echo)).tools();
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, ["status", "board"]);
    assert_eq!(tools[0].description.as_deref(), Some("the status verb"));
    assert_eq!(tools[0].input_schema.get("type"), Some(&json!("object")));
}

#[test]
fn a_call_maps_its_arguments() {
    let specs = Echo.list();
    let call = call_of(
        &request(
            "board",
            json!({ "args": ["ready", "--x"], "stdin": "in", "json": true, "more": true }),
        ),
        &specs,
    )
    .expect("maps");
    assert_eq!(
        call,
        ToolCall {
            tool: "board".into(),
            args: vec!["ready".into(), "--x".into()],
            stdin: Some("in".into()),
            json: true,
            more: true,
        }
    );
    let bare = call_of(&request("status", Value::Null), &specs).expect("maps");
    assert_eq!(
        bare,
        ToolCall {
            tool: "status".into(),
            ..ToolCall::default()
        }
    );
}

#[test]
fn an_unknown_tool_is_a_protocol_error_and_the_server_is_not_a_tool() {
    let specs = Echo.list();
    for name in ["mcp", "nope", ""] {
        let error = call_of(&request(name, Value::Null), &specs).expect_err("refused");
        assert!(error.message.contains("unknown tool"), "{}", error.message);
    }
}

#[test]
fn arguments_of_the_wrong_shape_are_a_protocol_error() {
    let specs = Echo.list();
    for bad in [
        json!({ "args": "ready" }),
        json!({ "args": [1] }),
        json!({ "json": "yes" }),
        json!({ "stdin": 3 }),
        json!({ "arg": ["ready"] }),
    ] {
        let error = call_of(&request("status", bad.clone()), &specs).expect_err("refused");
        assert!(error.message.contains("bad arguments"), "{bad}: {error:?}");
    }
}

#[test]
fn a_result_is_one_text_block_with_the_exit_in_meta() {
    let ok = serde_json::to_value(result_of(&ToolResult {
        text: "out\n".into(),
        is_error: false,
        exit: 0,
    }))
    .expect("serializes");
    assert_eq!(ok["content"].as_array().map(Vec::len), Some(1));
    assert_eq!(ok["content"][0]["type"], "text");
    assert_eq!(ok["content"][0]["text"], "out\n");
    assert_eq!(ok["isError"], false);
    assert_eq!(ok["_meta"]["zaf/exit"], 0);
    assert!(ok.get("structuredContent").is_none());

    let failed = serde_json::to_value(result_of(&ToolResult {
        text: "bad\nexit 1".into(),
        is_error: true,
        exit: 1,
    }))
    .expect("serializes");
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["_meta"]["zaf/exit"], 1);
    assert!(failed.get("structuredContent").is_none());
}

/// A host standing in for the runner: each tool does what its name says.
///
/// - `ticks`: three ticks (two with a stderr line, one without, at 120 s), then returns.
/// - `hold`: waits for `cancel` to be raised, notes that it saw it, then returns.
/// - `slow`: waits until `release` is raised.
/// - `quick`: returns at once.
struct Fake {
    saw_cancel: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl Fake {
    fn new() -> Self {
        Self {
            saw_cancel: Arc::new(AtomicBool::new(false)),
            release: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn done(text: &str) -> ToolResult {
    ToolResult {
        text: text.to_owned(),
        is_error: false,
        exit: 0,
    }
}

impl ToolHost for Fake {
    fn list(&self) -> Vec<ToolSpec> {
        ["ticks", "hold", "slow", "quick"].map(spec).to_vec()
    }

    fn call(&self, call: ToolCall, progress: &dyn Fn(Tick), cancel: &Cancel) -> ToolResult {
        match call.tool.as_str() {
            "ticks" => {
                progress(Tick {
                    elapsed: Duration::from_secs(60),
                    last_stderr: Some("first line".into()),
                });
                progress(Tick {
                    elapsed: Duration::from_secs(120),
                    last_stderr: None,
                });
                progress(Tick {
                    elapsed: Duration::from_secs(180),
                    last_stderr: Some("third line".into()),
                });
                done("ticked")
            }
            "hold" => {
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                self.saw_cancel.store(true, Ordering::SeqCst);
                done("stopped")
            }
            "slow" => {
                while !self.release.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                done("slow done")
            }
            _ => done("quick done"),
        }
    }
}

/// A client speaking JSON-RPC lines to a server on an in-memory stream.
struct Client {
    writer: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    inbox: VecDeque<Value>,
}

impl Client {
    /// Starts `host`'s server and completes the handshake.
    async fn start(host: Arc<dyn ToolHost>) -> Self {
        let (near, far) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let running = McpServer::new(host)
                .serve(tokio::io::split(far))
                .await
                .expect("the server starts");
            let _ = running.waiting().await;
        });
        let (read, writer) = tokio::io::split(near);
        let mut client = Self {
            writer,
            lines: BufReader::new(read).lines(),
            inbox: VecDeque::new(),
        };
        client
            .send(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "0" }
                }
            }))
            .await;
        let reply = client.next().await;
        assert_eq!(reply["id"], 0, "{reply}");
        client
            .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        client
    }

    async fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .expect("writes");
    }

    /// The next message from the server, within ten seconds.
    async fn next(&mut self) -> Value {
        if let Some(message) = self.inbox.pop_front() {
            return message;
        }
        let line = tokio::time::timeout(Duration::from_secs(10), self.lines.next_line())
            .await
            .expect("the server answers in time")
            .expect("reads")
            .expect("the stream is open");
        serde_json::from_str(&line).expect("a JSON line")
    }

    /// Calls `tool`, with `token` as its progress token when given.
    async fn call(&mut self, id: u64, tool: &str, token: Option<&str>) {
        let mut params = json!({ "name": tool, "arguments": {} });
        if let Some(token) = token {
            params["_meta"] = json!({ "progressToken": token });
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params }))
            .await;
    }

    /// Reads until the response to `id`: the progress notifications before it, and the response.
    async fn until_response(&mut self, id: u64) -> (Vec<Value>, Value) {
        let mut progress = Vec::new();
        loop {
            let message = self.next().await;
            if message["method"] == "notifications/progress" {
                progress.push(message["params"].clone());
            } else if message["id"] == id && message.get("method").is_none() {
                return (progress, message);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ticks_become_rising_progress() {
    let mut client = Client::start(Arc::new(Fake::new())).await;
    client.call(1, "ticks", Some("tok-1")).await;
    let (progress, response) = client.until_response(1).await;
    assert_eq!(response["result"]["content"][0]["text"], "ticked");
    assert_eq!(progress.len(), 3, "{progress:?}");
    for params in &progress {
        assert_eq!(params["progressToken"], "tok-1");
    }
    let rising: Vec<f64> = progress
        .iter()
        .map(|params| params["progress"].as_f64().expect("a number"))
        .collect();
    assert!(
        rising.windows(2).all(|pair| pair[1] >= pair[0] + 1.0),
        "{rising:?}"
    );
    assert_eq!(progress[0]["message"], "first line");
    assert_eq!(progress[2]["message"], "third line");
}

#[tokio::test(flavor = "multi_thread")]
async fn waiting_message_without_stderr() {
    let tick = Tick {
        elapsed: Duration::from_secs(125),
        last_stderr: None,
    };
    assert_eq!(tick_message("check", &tick), "check waiting, 2 min");

    let mut client = Client::start(Arc::new(Fake::new())).await;
    client.call(1, "ticks", Some("tok-2")).await;
    let (progress, _) = client.until_response(1).await;
    assert_eq!(progress[1]["message"], "ticks waiting, 2 min");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_without_a_token_sends_no_progress() {
    let mut client = Client::start(Arc::new(Fake::new())).await;
    client.call(1, "ticks", None).await;
    let (progress, response) = client.until_response(1).await;
    assert_eq!(response["result"]["content"][0]["text"], "ticked");
    assert!(progress.is_empty(), "{progress:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_reaches_runner() {
    let host = Arc::new(Fake::new());
    let saw = Arc::clone(&host.saw_cancel);
    let mut client = Client::start(host).await;
    client.call(7, "hold", Some("tok-3")).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!saw.load(Ordering::SeqCst), "not cancelled yet");
    client
        .send(json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": 7, "reason": "the user stopped it" }
        }))
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !saw.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the runner saw the call's Cancel raised");
}

#[tokio::test(flavor = "multi_thread")]
async fn calls_run_side_by_side() {
    let host = Arc::new(Fake::new());
    let release = Arc::clone(&host.release);
    let mut client = Client::start(host).await;
    client.call(1, "slow", None).await;
    client.call(2, "quick", None).await;
    let (_, quick) = client.until_response(2).await;
    assert_eq!(quick["result"]["content"][0]["text"], "quick done");
    assert!(!release.load(Ordering::SeqCst));
    release.store(true, Ordering::SeqCst);
    let (_, slow) = client.until_response(1).await;
    assert_eq!(slow["result"]["content"][0]["text"], "slow done");
}
