//! The MCP server over stdio, on `rmcp` (I1, D22): the handshake, the tools listed from the
//! catalog and each call handed to the tool host on a blocking task.
//!
//! This is the one module that names `rmcp` and tokio besides the verb that makes the runtime;
//! the host it drives is the [`ToolHost`] port, so a later SDK change stays here. The server is
//! written by hand against `rmcp`'s trait, with no `#[tool]` macros: the tools come from clap at
//! run time.
//!
//! A call maps to a [`ToolCall`] from the tool's `arguments` (`args`, `stdin`, `json`, `more`);
//! an unknown tool name or an argument of the wrong shape is a protocol error and runs nothing
//! (the hidden `mcp` verb is not a tool, so a model cannot start a server inside a server). The
//! result is one text block, `isError` and `_meta["zaf/exit"]`, never structured content.
//!
//! Each tick of a call is a `notifications/progress` on the call's `progressToken` through the
//! peer, with `progress` rising by one each time; a call without a token sends none. When `rmcp`
//! reports the request cancelled, the call's [`Cancel`] is raised. Each call runs on its own
//! blocking task, and no timeout is set.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, MetaObject, PaginatedRequestParams, ProgressNotificationParam,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::domain::{Cancel, META_EXIT_KEY, Tick, ToolCall, ToolResult, ToolSpec};
use crate::ports::ToolHost;

/// D15's rule, shown to the model at the start.
pub const INSTRUCTIONS: &str = "zaf's verbs are MCP tools. Wherever a page says `zaf <verb> <words>`, call the tool `<verb>` (`mcp__zaf__<verb>`) with `args` set to those words. Never type `zaf` in Bash; it is denied.";

/// The most characters Claude Code shows of a server's instructions.
pub const INSTRUCTIONS_LIMIT: usize = 4096;

/// The server's name in the handshake.
const SERVER_NAME: &str = "zaf";

/// The MCP server over a tool host.
#[derive(Clone)]
pub struct McpServer {
    host: Arc<dyn ToolHost>,
}

impl McpServer {
    /// A server answering from `host`.
    pub fn new(host: Arc<dyn ToolHost>) -> Self {
        Self { host }
    }

    /// Serves over this process's stdin and stdout until the client closes the stream. Must run
    /// on a tokio runtime; nothing else may write to standard output meanwhile.
    pub async fn serve_stdio(self) -> Result<(), String> {
        let running = self
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|error| format!("cannot start the MCP server: {error}"))?;
        running
            .waiting()
            .await
            .map(|_| ())
            .map_err(|error| format!("the MCP server stopped: {error}"))
    }

    /// What the tool host lists, as `rmcp`'s tools.
    fn tools(&self) -> Vec<Tool> {
        self.host.list().iter().map(tool_of).collect()
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let call = call_of(&request, &self.host.list())?;
        let cancel = Cancel::new();
        let watcher = {
            let cancel = cancel.clone();
            let token = context.ct.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                cancel.cancel();
            })
        };
        let progress = progress_of(&context, &call.tool);
        let host = Arc::clone(&self.host);
        let result = tokio::task::spawn_blocking(move || host.call(call, &*progress, &cancel))
            .await
            .map_err(|error| ErrorData::internal_error(format!("the call failed: {error}"), None));
        watcher.abort();
        Ok(CallToolResponse::Complete(result_of(&result?)))
    }
}

/// The progress callback of a call: each tick becomes a `notifications/progress` on the call's
/// token, rising by one; with no token it does nothing. It runs on the call's blocking task, so
/// it waits for the send there, and a failed send (a closed peer) is dropped.
fn progress_of(context: &RequestContext<RoleServer>, verb: &str) -> Box<dyn Fn(Tick) + Send> {
    let Some(token) = context.meta.get_progress_token() else {
        return Box::new(|_| {});
    };
    let peer = context.peer.clone();
    let handle = tokio::runtime::Handle::current();
    let verb = verb.to_owned();
    let count = AtomicU64::new(0);
    Box::new(move |tick| {
        let progress = count.fetch_add(1, Ordering::SeqCst) + 1;
        let params = ProgressNotificationParam::new(token.clone(), progress as f64)
            .with_message(tick_message(&verb, &tick));
        let _ = handle.block_on(peer.notify_progress(params));
    })
}

/// A tick's `message`: the call's last stderr line, else `<verb> waiting, <m> min`.
pub fn tick_message(verb: &str, tick: &Tick) -> String {
    match &tick.last_stderr {
        Some(line) => line.clone(),
        None => format!("{verb} waiting, {} min", tick.elapsed.as_secs() / 60),
    }
}

/// The tool listed for `spec`: its name, its description and its input schema.
pub fn tool_of(spec: &ToolSpec) -> Tool {
    let schema = match &spec.input_schema {
        Value::Object(map) => map.clone(),
        _ => Map::from_iter([("type".to_owned(), json!("object"))]),
    };
    Tool::new(
        spec.name.clone(),
        spec.description.clone(),
        Arc::new(schema),
    )
}

/// The input of a tool call: every field is optional and no other is accepted, so a misspelt
/// `arg` is refused rather than run as a call with no arguments.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    stdin: Option<String>,
    #[serde(default)]
    json: bool,
    #[serde(default)]
    more: bool,
}

/// The call `request` makes, when it names a tool of `specs` and its arguments have the shape
/// the schema gives; else a protocol error.
pub fn call_of(request: &CallToolRequestParams, specs: &[ToolSpec]) -> Result<ToolCall, ErrorData> {
    let tool = request.name.as_ref();
    if !specs.iter().any(|spec| spec.name == tool) {
        return Err(ErrorData::invalid_params(
            format!("unknown tool `{tool}`"),
            None,
        ));
    }
    let input: Input = match &request.arguments {
        None => Input::default(),
        Some(arguments) => {
            serde_json::from_value(Value::Object(arguments.clone())).map_err(|error| {
                ErrorData::invalid_params(format!("bad arguments for `{tool}`: {error}"), None)
            })?
        }
    };
    Ok(ToolCall {
        tool: tool.to_owned(),
        args: input.args,
        stdin: input.stdin,
        json: input.json,
        more: input.more,
    })
}

/// The result as the protocol carries it: one text block, `isError`, the exit code under
/// `_meta`, and no structured content.
pub fn result_of(result: &ToolResult) -> CallToolResult {
    let content = vec![ContentBlock::text(result.text.clone())];
    let mut meta = MetaObject::new();
    meta.0.insert(META_EXIT_KEY.to_owned(), json!(result.exit));
    let mut built = if result.is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    built.meta = Some(meta);
    built
}

#[cfg(test)]
mod tests;
