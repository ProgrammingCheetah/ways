//! The MCP line of `zaf doctor` (release 0.5.5, D14, M10): whether the Engineer's own Claude
//! holds zaf's MCP server at user scope.
//!
//! The Engineer's Claude is asked as it stands, `claude mcp get zaf` with `CLAUDE_CONFIG_DIR`
//! left as the environment has it, and the answer is read by [`McpRegistration::from_get`], the
//! classifier `zaf setup` and `zaf profile init` use. The line is informational: the Engineer may
//! decline the entry in setup and keep Bash `zaf`, so it never fails the report. A machine with
//! no `claude` shows the missing line. Doctor never writes: the fix is named, not run.

use super::Commands;
use super::paint::{bold, piece, plain};
use crate::app::profile::{McpRegistration, ZAF_MCP_NAME, mcp_add_line};
use crate::domain::screen::{Line, Meaning};
use crate::ports::GateVerdict;

/// How zaf's MCP server stands in the Engineer's own Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpCheck {
    /// zaf's entry is registered at user scope.
    Ok,
    /// No `zaf` server is registered, or `claude` could not be asked.
    Missing,
    /// Another `zaf` entry holds the name.
    Different,
}

impl McpCheck {
    /// `mcp: …`, one line.
    pub fn line(&self) -> String {
        plain(&self.painted())
    }

    /// [`McpCheck::line`] as pieces: `ok` done, the rest an aside with its commands cyan.
    pub fn painted(&self) -> Line {
        let mut line = vec![bold("mcp: ")];
        match self {
            Self::Ok => line.extend([piece(Meaning::Done, "ok"), bold(" (user scope)")]),
            Self::Missing => line.extend([
                piece(Meaning::Dim, "missing (fix: "),
                piece(Meaning::Verb, "zaf setup"),
                piece(Meaning::Dim, ", or "),
                piece(Meaning::Verb, mcp_add_line(None)),
                piece(Meaning::Dim, ")"),
            ]),
            Self::Different => line.extend([
                piece(Meaning::Dim, "a different zaf entry (fix: "),
                piece(Meaning::Verb, "zaf setup"),
                piece(Meaning::Dim, " asks to replace it)"),
            ]),
        }
        line
    }
}

/// Asks the Engineer's Claude `claude mcp get zaf`; reads only.
pub fn check(commands: &dyn Commands) -> McpCheck {
    let Ok(output) = commands.run("claude", &["mcp", "get", ZAF_MCP_NAME]) else {
        return McpCheck::Missing;
    };
    let verdict = if output.success {
        GateVerdict::Passed
    } else {
        GateVerdict::Failed
    };
    match McpRegistration::from_get(&output.text, verdict) {
        McpRegistration::Ours => McpCheck::Ok,
        McpRegistration::Missing => McpCheck::Missing,
        McpRegistration::Other(_) => McpCheck::Different,
    }
}
