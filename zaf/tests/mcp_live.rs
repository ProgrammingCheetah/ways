//! Claude Code against the built `zaf mcp`: the one test that proves a real client reaches the
//! `status` tool (D22, I1), and not only that the server speaks MCP to a test's own client.
//!
//! It is `#[ignore]`d, so `cargo test` skips it: it needs the `claude` CLI on PATH and logged in,
//! and it spends a model call. Run it with
//!
//! ```text
//! cargo test --test mcp_live -- --ignored live_claude
//! ```
//!
//! With `claude` absent from PATH it fails and says so; it never passes without having asked.

mod common;

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use common::{Project, outcome};
use serde_json::json;

/// A manifest with a board, which `zaf status` reads.
const BOARD_MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n";

/// The line `zaf status` opens with, which nothing else prints: the open questions' count, here
/// none.
const STATUS_LINE: &str = "questions 0";

/// How long Claude Code has to answer.
const PATIENCE: Duration = Duration::from_secs(240);

/// The `claude` the machine has, not the stand-in `tests/fixtures/` puts first on a test's PATH.
fn claude() -> Command {
    let mut command = Command::new("claude");
    command
        .env_remove("CLAUDECODE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
#[ignore = "runs Claude Code against zaf mcp; `cargo test --test mcp_live -- --ignored live_claude`"]
fn live_claude_reaches_status() {
    let version = match claude().arg("--version").output() {
        Ok(output) => output,
        Err(error) => panic!(
            "`claude` is not on PATH ({error}): install Claude Code and log in to run the live \
             test"
        ),
    };
    assert!(
        version.status.success(),
        "`claude --version` failed: {}",
        String::from_utf8_lossy(&version.stderr)
    );

    let mut project = Project::new(BOARD_MANIFEST);
    // Whatever the board is asked, it answers an empty list.
    project.on("*", "[]");

    // The config names the built binary, run the way the tests run it: its PATH and the fake
    // `gh`'s scene ride in the server's `env`, which Claude Code adds to its own.
    let server = project.command_in(&project.root, &["mcp"]);
    let env: serde_json::Map<String, serde_json::Value> = server
        .get_envs()
        .filter_map(|(key, value)| Some((key.to_str()?.to_owned(), json!(value?.to_str()?))))
        .collect();
    let config = project.temp.path().join("mcp.json");
    let body = json!({ "mcpServers": { "zaf": {
        "command": server.get_program().to_str().expect("binary path"),
        "args": ["mcp"],
        "env": env,
    } } });
    std::fs::write(&config, body.to_string()).expect("write the MCP config");

    let mut command = claude();
    command
        .current_dir(&project.root)
        .args(["-p", "--model", "haiku", "--mcp-config"])
        .arg(&config)
        .args(["--strict-mcp-config", "--allowedTools", "mcp__zaf__status"])
        .args([
            "--",
            "Call the mcp__zaf__status tool once, with no arguments, then reply with exactly \
             what it returned and nothing else.",
        ]);
    let child = command.spawn().expect("start claude");
    let pid = child.id();
    let (done, answered) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(child.wait_with_output());
    });
    let output = match answered.recv_timeout(PATIENCE) {
        Ok(output) => output.expect("wait for claude"),
        Err(_) => {
            kill(pid);
            panic!("claude did not answer within {PATIENCE:?}");
        }
    };
    let (code, stdout, stderr) = outcome(&output);
    assert_eq!(code, 0, "claude failed: {stderr}\n{stdout}");
    assert!(
        stdout
            .lines()
            .any(|line| line.trim().trim_matches('`').starts_with(STATUS_LINE)),
        "claude did not reach the zaf status tool: no line starting `{STATUS_LINE}` in its \
         answer:\n{stdout}\n{stderr}"
    );
}

/// Ends a Claude Code that outlived its patience.
fn kill(pid: u32) {
    let pid = i32::try_from(pid).expect("a pid");
    // SAFETY: `kill` takes any ints and only signals the process it names.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}
