//! The MCP row of each generated profile (release 0.5.5, D13, M10): whether the profile holds
//! zaf's MCP server and the settings that make zaf's verbs run through it.
//!
//! The profile's Claude is asked `claude mcp get zaf` with `CLAUDE_CONFIG_DIR` set to its home,
//! read by [`McpRegistration::from_get`], and its `settings.json` is read for the rules and keys
//! `zaf profile init` writes. A profile that holds neither fails the report: `zaf spawn` would
//! start an instance with no way to run `zaf`. Doctor never writes: the fix is named, not run.
//! A profile not generated yet gets no row; its stale and login rows speak for it.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::paint::{bold, piece, plain};
use super::{CONFIG_DIR_VAR, Commands};
use crate::app::profile::{self, McpRegistration, ZAF_ALLOW, ZAF_DENY, ZAF_MCP_NAME};
use crate::composition::STATE_DIR;
use crate::domain::screen::{Line, Meaning};
use crate::manifest::Manifest;
use crate::ports::GateVerdict;

/// A profile's own settings file.
const SETTINGS_FILE: &str = "settings.json";

/// How one generated profile stands on zaf's MCP server and its settings.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProfileMcpCheck {
    /// The profile's name.
    pub profile: String,
    /// Each part that is missing or wrong; none when the profile is right.
    pub problems: Vec<String>,
    /// Whether nothing is missing or wrong.
    pub ok: bool,
}

impl ProfileMcpCheck {
    /// `profile <name>: mcp ok`, or the parts that are missing or wrong with `zaf profile init`.
    pub fn line(&self) -> String {
        plain(&self.painted())
    }

    /// [`ProfileMcpCheck::line`] as pieces: `ok` done, each problem red, the fix an aside.
    pub fn painted(&self) -> Line {
        let mut line = vec![bold(format!("profile {}: mcp ", self.profile))];
        if self.ok {
            line.push(piece(Meaning::Done, "ok"));
            return line;
        }
        line.push(piece(Meaning::Failed, self.problems.join("; ")));
        line.extend([
            piece(Meaning::Dim, " (fix: "),
            piece(Meaning::Verb, "zaf profile init"),
            piece(Meaning::Dim, ")"),
        ]);
        line
    }
}

/// One row per generated profile whose home exists; reads only.
pub fn check(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Vec<ProfileMcpCheck> {
    let state_dir = root.join(STATE_DIR);
    let project_servers = profile::project_mcp_servers(root);
    profile::generated(manifest)
        .into_iter()
        .filter_map(|name| {
            let home = profile::home(&state_dir, name);
            if !home.is_dir() {
                return None;
            }
            let mut problems = server_problems(&home, commands);
            problems.extend(settings_problems(&home, &project_servers));
            Some(ProfileMcpCheck {
                profile: name.to_string(),
                ok: problems.is_empty(),
                problems,
            })
        })
        .collect()
}

/// What `claude mcp get zaf` shows in the profile's Claude, as a problem when it is not ours.
fn server_problems(home: &Path, commands: &dyn Commands) -> Vec<String> {
    let setting = format!("{CONFIG_DIR_VAR}={}", home.display());
    let shown = commands
        .run("env", &[&setting, "claude", "mcp", "get", ZAF_MCP_NAME])
        .map(|output| {
            let verdict = if output.success {
                GateVerdict::Passed
            } else {
                GateVerdict::Failed
            };
            McpRegistration::from_get(&output.text, verdict)
        })
        .unwrap_or(McpRegistration::Missing);
    match shown {
        McpRegistration::Ours => Vec::new(),
        McpRegistration::Missing => vec![format!("server {ZAF_MCP_NAME} missing")],
        McpRegistration::Other(_) => vec![format!("server {ZAF_MCP_NAME} is a different entry")],
    }
}

/// The parts of the profile's `settings.json` that are missing or wrong.
fn settings_problems(
    home: &Path,
    project_servers: &Result<Vec<String>, profile::ProfileError>,
) -> Vec<String> {
    let settings = match fs::read_to_string(home.join(SETTINGS_FILE)) {
        Err(_) => return vec![format!("{SETTINGS_FILE} unreadable")],
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(settings) => settings,
            Err(_) => return vec![format!("{SETTINGS_FILE} is not valid JSON")],
        },
    };
    let strings = |pointer: &str| -> Vec<&str> {
        settings
            .pointer(pointer)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    };
    let (allow, deny) = (strings("/permissions/allow"), strings("/permissions/deny"));
    let mut problems = Vec::new();
    if !allow.contains(&ZAF_ALLOW) {
        problems.push(format!("{ZAF_ALLOW} not allowed"));
    }
    for rule in ZAF_DENY {
        if !deny.contains(&rule) {
            problems.push(format!("{rule} not denied"));
        }
    }
    if allow.contains(&"Bash(zaf *)") {
        problems.push("Bash(zaf *) still allowed".to_owned());
    }
    if settings.get("disableClaudeAiConnectors") != Some(&Value::Bool(true)) {
        problems.push("disableClaudeAiConnectors is not true".to_owned());
    }
    match project_servers {
        Ok(names) => {
            let disabled = strings("/disabledMcpjsonServers");
            for name in names
                .iter()
                .filter(|name| !disabled.contains(&name.as_str()))
            {
                problems.push(format!("disabledMcpjsonServers lacks {name}"));
            }
        }
        Err(error) => problems.push(format!("project .mcp.json unreadable: {error}")),
    }
    problems
}
