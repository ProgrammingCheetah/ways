//! `zaf mcp` through the built binary: MCP spoken over its stdio, the tools it lists, a call
//! answered as the verb answers on a terminal's own, and the verb hidden from the help.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use clap::CommandFactory;
use common::{Project, outcome};
use serde_json::{Value, json};
use zaf::cli::Cli;
use zaf::cli::mcp::Catalog;

const MANIFEST: &str = "[project]\nname = \"example\"\n";

/// A manifest with a board, which `zaf status` reads.
const BOARD_MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n";

/// A client over the server's stdio: one JSON message a line, each way.
struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    /// Starts `zaf mcp` in the project and shakes hands.
    fn start(project: &Project) -> Self {
        Self::start_command(project.command_in(&project.root, &["mcp"]))
    }

    /// Starts the server `command` runs and shakes hands.
    fn start_command(mut command: std::process::Command) -> Self {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("start zaf mcp");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut client = Self {
            child,
            stdin,
            stdout,
            next: 1,
        };
        let initialized = client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        );
        assert!(initialized.get("error").is_none(), "{initialized}");
        client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        client
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{message}").expect("write");
        stdin.flush().expect("flush");
    }

    /// Sends a request and reads until its response, which it returns whole.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read");
            assert!(read > 0, "the server closed before answering {method}");
            let message: Value = serde_json::from_str(line.trim()).expect("a JSON line");
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
        }
    }

    /// Closes the server's stdin and waits for it to exit.
    fn finish(mut self) -> i32 {
        drop(self.stdin.take());
        self.child.wait().expect("wait").code().expect("exit code")
    }
}

#[test]
fn lists_the_catalog() {
    let project = Project::new(MANIFEST);
    let mut client = Client::start(&project);
    let listed = client.request("tools/list", json!({}));
    let names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_owned())
        .collect();
    let catalog: Vec<String> = Catalog::from_command(&Cli::command())
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    assert!(!catalog.is_empty());
    assert_eq!(names, catalog);
    assert!(!names.iter().any(|name| name == "mcp"), "{names:?}");
    assert!(
        listed["result"]["tools"][0]["inputSchema"]["properties"]["args"].is_object(),
        "{listed}"
    );
    assert_eq!(
        client.finish(),
        0,
        "the server exits 0 when its stdin closes"
    );
}

#[test]
fn initialize_carries_the_instructions() {
    let project = Project::new(MANIFEST);
    let mut command = project.command_in(&project.root, &["mcp"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().expect("start zaf mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let hello = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }
    });
    writeln!(stdin, "{hello}").expect("write");
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read");
    let answer: Value = serde_json::from_str(line.trim()).expect("a JSON line");
    let instructions = answer["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(instructions.chars().count() <= 4096);
    assert!(
        instructions.contains("Never type `zaf` in Bash; it is denied."),
        "{instructions}"
    );
    assert_eq!(answer["result"]["serverInfo"]["name"], "zaf");
    drop(stdin);
    child.wait().expect("wait");
}

#[test]
fn status_through_server_matches_cli() {
    let mut project = Project::new(BOARD_MANIFEST);
    // Whatever the board is asked, it answers an empty list.
    project.on("*", "[]");
    let (code, direct, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(!direct.is_empty(), "status prints something");

    let mut client = Client::start(&project);
    let called = client.request("tools/call", json!({ "name": "status", "arguments": {} }));
    let result = &called["result"];
    assert_eq!(result["isError"], false, "{called}");
    assert_eq!(
        result["content"].as_array().map(Vec::len),
        Some(1),
        "{called}"
    );
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(result["content"][0]["text"], direct.as_str());
    assert_eq!(result["_meta"]["zaf/exit"], 0);
    assert!(result.get("structuredContent").is_none());
    client.finish();
}

#[test]
fn a_failing_verb_is_an_error_result_with_its_exit() {
    let project = Project::new(BOARD_MANIFEST);
    let mut client = Client::start(&project);
    let called = client.request(
        "tools/call",
        json!({ "name": "board", "arguments": { "args": ["no-such-action"] } }),
    );
    let result = &called["result"];
    assert_eq!(result["isError"], true, "{called}");
    assert_eq!(result["_meta"]["zaf/exit"], 2, "{called}");
    let text = result["content"][0]["text"].as_str().expect("text");
    assert!(text.ends_with("exit 2"), "{text}");
    client.finish();
}

#[test]
fn the_server_verb_is_not_a_tool_it_can_call() {
    let project = Project::new(MANIFEST);
    let mut client = Client::start(&project);
    let called = client.request("tools/call", json!({ "name": "mcp", "arguments": {} }));
    assert!(called.get("result").is_none(), "{called}");
    assert_eq!(called["error"]["code"], -32602, "{called}");
    client.finish();
}

/// Waits until `program` can be started: a file just written is "text file busy" while a
/// process forked by a parallel test still holds the write handle it inherited.
fn runs_when_ready(program: &std::path::Path) {
    for _ in 0..100 {
        match Command::new(program).arg("--version").output() {
            Err(error) if error.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => return,
        }
    }
    panic!("{} stayed busy", program.display());
}

#[test]
fn swapped_binary_adds_line() {
    use std::os::unix::fs::PermissionsExt;
    let project = Project::new(BOARD_MANIFEST);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("zaf");
    // A link holds no write handle, so no forked sibling can keep it "busy" to exec; a copy is
    // the fallback where the link crosses filesystems.
    if std::fs::hard_link(env!("CARGO_BIN_EXE_zaf"), &path).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_zaf"), &path).expect("copy the binary");
        runs_when_ready(&path);
    }
    let mut client = Client::start_command(project.command_of(&path, &project.root, &["mcp"]));
    let text_of = |called: &Value| called["result"]["content"][0]["text"].to_string();

    // The binary the server was started from is its own version: no line.
    let before = client.request("tools/call", json!({ "name": "status", "arguments": {} }));
    assert!(!text_of(&before).contains("zaf was updated"), "{before}");

    // An update replaces the file at the path with a stand-in of another version.
    let stand_in = dir.path().join("zaf.new");
    std::fs::write(
        &stand_in,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'zaf 99.0.0'; else echo ran; fi\n",
    )
    .expect("write the stand-in");
    std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    runs_when_ready(&stand_in);
    std::fs::rename(&stand_in, &path).expect("replace the binary");

    let after = client.request("tools/call", json!({ "name": "status", "arguments": {} }));
    let text = after["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert_eq!(
        text, "ran\nzaf was updated to 99.0.0; restart zaf's MCP (/mcp) for the new tool list",
        "{after}"
    );
    client.finish();
}

/// A manifest that forbids `zaf do pr-merge`.
const FORBIDDING: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forbidden]\ncommands = [\"zaf do pr-merge\"]\n";

/// A project whose homes exist, so the first-run notice never reaches a test's output.
fn set_up_project(manifest: &str) -> Project {
    let project = Project::new(manifest);
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
        std::fs::remove_file(project.root.join(".zaf/actions").join(name)).expect("take it away");
    }
    project
}

/// Writes the project action `name` with `body`.
fn write_action(project: &Project, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = project.root.join(".zaf/actions");
    std::fs::create_dir_all(&dir).expect("actions folder");
    let file = dir.join(name);
    std::fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: the {name} action\n# zaf 1: it failed\n{body}\n"),
    )
    .expect("write the action");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// A server run as an instance that an action's body started, so the nesting markers are set on
/// it. The manifest's `[forbidden]` stands before the child: it refuses a call there.
fn nested_server(project: &Project) -> Client {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        std::iter::once(bin.to_owned()).chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    let mut command = project.command_in(&project.root, &["mcp"]);
    command
        .env("PATH", search_path)
        .env("ZAF_INSTANCE", "programmer instance 1 on #9")
        .env("ZAF_DEPTH", "1")
        .env("ZAF_ACTION_CHAIN", "outer");
    Client::start_command(command)
}

fn call_do(client: &mut Client, args: &[&str]) -> Value {
    let called = client.request(
        "tools/call",
        json!({ "name": "do", "arguments": { "args": args } }),
    );
    assert!(called.get("error").is_none(), "{called}");
    called
}

#[test]
fn mcp_do_forbidden_not_run_from_zaf() {
    let project = set_up_project(FORBIDDING);
    write_action(&project, "pr-merge", "touch \"$ZAF_ROOT/pr-merge.ran\"");
    let mut client = nested_server(&project);
    let called = call_do(&mut client, &["pr-merge", "5"]);
    assert_eq!(called["result"]["isError"], true, "{called}");
    assert_eq!(called["result"]["_meta"]["zaf/exit"], 1, "{called}");
    let text = called["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        text.starts_with("zaf do pr-merge is refused") && text.ends_with("\nexit 1"),
        "{called}"
    );
    assert_eq!(
        text.lines().count(),
        2,
        "one refusal line, then the exit: {text}"
    );
    assert!(!project.root.join("pr-merge.ran").exists(), "it ran");
    client.finish();
}

#[test]
fn mcp_action_body_still_nests() {
    let project = set_up_project(FORBIDDING);
    write_action(
        &project,
        "pr-merge",
        "echo \"$ZAF_DEPTH\" > \"$ZAF_ROOT/pr-merge.depth\"",
    );
    // As `slice-merge` does: the body calls the binary, which sees the action above it.
    write_action(
        &project,
        "outer",
        "echo \"$ZAF_DEPTH\" > \"$ZAF_ROOT/outer.depth\"\nzaf do pr-merge",
    );
    let mut client = nested_server(&project);
    let called = call_do(&mut client, &["outer"]);
    assert_eq!(called["result"]["isError"], false, "{called}");
    assert_eq!(called["result"]["_meta"]["zaf/exit"], 0, "{called}");
    // The call itself was a top-level run (depth 0); its body's call was nested (depth 1).
    let depth = |name: &str| {
        std::fs::read_to_string(project.root.join(name))
            .unwrap_or_else(|error| panic!("{name}: {error}"))
            .trim()
            .to_owned()
    };
    assert_eq!(depth("outer.depth"), "0");
    assert_eq!(depth("pr-merge.depth"), "1");
    client.finish();
}

#[test]
fn help_points_to_docs() {
    let project = Project::new(MANIFEST);
    let (code, help, stderr) = outcome(&project.zaf(&["mcp", "--help"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(help.contains("docs/zaf-actions.md"), "{help}");
}

#[test]
fn hidden_from_help() {
    let project = Project::new(MANIFEST);
    let (code, help, stderr) = outcome(&project.zaf(&["--help"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !help
            .lines()
            .any(|line| line.trim_start().starts_with("mcp")),
        "{help}"
    );
    let (code, _, stderr) = outcome(&project.zaf(&["mcp", "--help"]));
    assert_eq!(code, 0, "{stderr}");
}
