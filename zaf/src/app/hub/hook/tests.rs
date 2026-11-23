use std::path::Path;

use serde_json::{Value, json};

use super::{TESTING, ToolCall, WAITING_ON_REVIEW, WORKING};

/// A real `PostToolUse` payload of an MCP tool call, captured on a `claude -p` run (the note
/// beside it names the version and the command line).
fn mcp_fixture() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hook/mcp-post-tool-use.json");
    let text = std::fs::read_to_string(&path).expect("the fixture is there");
    serde_json::from_str(&text).expect("the fixture is JSON")
}

#[test]
fn hook_fixture_is_an_mcp_post_tool_use_payload() {
    let payload = mcp_fixture();
    assert!(payload.is_object());
    assert_eq!(payload["hook_event_name"], "PostToolUse");
    let tool = payload["tool_name"].as_str().expect("a tool_name");
    assert!(tool.starts_with("mcp__"), "tool_name is {tool}");
    assert!(payload["tool_input"].is_object());
}

#[test]
fn hook_fixture_carries_args_as_an_array_of_strings() {
    let payload = mcp_fixture();
    let args = payload["tool_input"]["args"]
        .as_array()
        .expect("args is an array");
    assert!(args.iter().all(Value::is_string));
    assert_eq!(args, &[Value::from("pr")]);
}

/// The fixture's shape with this tool and these args.
fn mcp_call(tool: &str, args: Option<Value>) -> ToolCall {
    let mut payload = mcp_fixture();
    payload["tool_name"] = json!(tool);
    match args {
        Some(args) => payload["tool_input"]["args"] = args,
        None => {
            payload["tool_input"]
                .as_object_mut()
                .unwrap()
                .remove("args");
        }
    }
    ToolCall::parse(&payload.to_string())
}

#[test]
fn mcp_check_pr_waits_on_review() {
    for target in ["pr", "pr:12:330"] {
        let call = mcp_call("mcp__zaf__check", Some(json!([target])));
        assert_eq!(call.word(), WAITING_ON_REVIEW, "{target}");
    }
}

#[test]
fn mcp_check_of_a_task_is_testing() {
    let call = mcp_call("mcp__zaf__check", Some(json!(["123"])));
    assert_eq!(call.word(), TESTING);
}

#[test]
fn mcp_verify_is_testing() {
    let call = mcp_call("mcp__zaf__verify", Some(json!(["123"])));
    assert_eq!(call.word(), TESTING);
}

#[test]
fn mcp_status_is_working() {
    assert_eq!(mcp_call("mcp__zaf__status", None).word(), WORKING);
    assert_eq!(
        mcp_call("mcp__zaf__status", Some(json!("x"))).word(),
        WORKING
    );
    assert_eq!(
        mcp_call("mcp__zaf__status", Some(json!([1, null]))).word(),
        WORKING
    );
}

#[test]
fn an_mcp_arg_is_one_word() {
    let call = mcp_call(
        "mcp__zaf__board",
        Some(json!(["report", "12", "all tests pass"])),
    );
    assert_eq!(call.word(), WORKING);
    let call = mcp_call(
        "mcp__zaf__board",
        Some(json!(["report", "12", "run zaf check pr"])),
    );
    assert_eq!(call.word(), WORKING);
}

#[test]
fn another_servers_tool_is_working() {
    let call = mcp_call("mcp__other__check", Some(json!(["pr"])));
    assert_eq!(call.word(), WORKING);
}

#[test]
fn the_recorded_fixture_parses_with_its_args() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hook/mcp-post-tool-use.json"),
    )
    .expect("the fixture is there");
    let call = ToolCall::parse(&text);
    assert_eq!(call.tool, "mcp__zaf__check");
    assert_eq!(call.args, vec!["pr".to_owned()]);
    assert_eq!(call.word(), WAITING_ON_REVIEW);
}
