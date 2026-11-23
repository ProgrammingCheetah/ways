//! The tool host: what the MCP server adapter drives, so it never names the app layer.

use crate::domain::{Cancel, Tick, ToolCall, ToolResult, ToolSpec};

/// Lists the tools and answers a call to one, synchronously; no async or protocol type crosses
/// this port. The app layer's `ToolService` is what answers behind it.
pub trait ToolHost: Send + Sync {
    /// The tools, as the catalog lists them.
    fn list(&self) -> Vec<ToolSpec>;

    /// Runs one call to its end, reporting progress through `progress` and stopping when
    /// `cancel` is raised.
    fn call(&self, call: ToolCall, progress: &dyn Fn(Tick), cancel: &Cancel) -> ToolResult;
}
