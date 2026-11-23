//! zaf's MCP server in a profile's own Claude config (release 0.5.5, D13).
//!
//! Every generated profile holds `mcpServers.zaf` at user scope, written by Claude's own
//! `claude mcp add-json --scope user`, never by hand. This file is the piece `profile init`,
//! `zaf setup` and `zaf doctor` share: the command lines, and a pure classifier of what
//! `claude mcp get zaf` shows.

use std::path::Path;

use super::{CONFIG_DIR_VAR, ProfileError, single_quote};
use crate::manifest::CommandLine;
use crate::ports::{GateVerdict, Shell};

/// The name the server is registered under.
pub const ZAF_MCP_NAME: &str = "zaf";

/// The one entry zaf registers: a bare `zaf mcp`, as the profiles' hook is a bare `zaf`.
pub const ZAF_MCP_ENTRY: &str = r#"{"type":"stdio","command":"zaf","args":["mcp"]}"#;

/// What `claude mcp get zaf` shows for a config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRegistration {
    /// No `zaf` server is registered.
    Missing,
    /// The `zaf` server is registered with exactly [`ZAF_MCP_ENTRY`], at user scope.
    Ours,
    /// Another `zaf` entry is registered; what `get` printed.
    Other(String),
}

impl McpRegistration {
    /// Reads the output and the exit of `claude mcp get zaf`.
    ///
    /// A failed `get` is how Claude says it knows no such server (`No MCP server found with
    /// name: zaf`), and an empty answer shows nothing either: both are [`Missing`]. A passing
    /// `get` is [`Ours`] only when it shows the user-scope stdio server `zaf` with the single
    /// argument `mcp` and no environment, headers or URL; any other text is [`Other`].
    ///
    /// [`Missing`]: McpRegistration::Missing
    /// [`Ours`]: McpRegistration::Ours
    /// [`Other`]: McpRegistration::Other
    pub fn from_get(output: &str, exit: GateVerdict) -> Self {
        let text = output.trim();
        if exit != GateVerdict::Passed || text.is_empty() {
            return Self::Missing;
        }
        if shows_ours(text) {
            Self::Ours
        } else {
            Self::Other(text.to_owned())
        }
    }
}

/// Whether the text `claude mcp get zaf` printed is our entry and nothing more.
///
/// ```text
/// zaf:
///   Scope: User config (available in all your projects)
///   Status: ✓ Connected
///   Type: stdio
///   Command: zaf
///   Args: mcp
///   Environment:
/// ```
fn shows_ours(text: &str) -> bool {
    let (mut scope, mut kind, mut command, mut args) = (None, None, None, None);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("zaf:") {
        return false;
    }
    for line in lines {
        // The hint after the server's fields ("To remove this server, run: …").
        if line.trim().is_empty() {
            break;
        }
        // A field is indented by two spaces; a deeper line belongs to `Environment:` or the like.
        let Some(field) = line.strip_prefix("  ") else {
            break;
        };
        if field.starts_with(' ') {
            return false;
        }
        let Some((key, value)) = field.split_once(':') else {
            return false;
        };
        let value = value.trim();
        match key {
            "Scope" => scope = Some(value),
            "Type" => kind = Some(value),
            "Command" => command = Some(value),
            "Args" => args = Some(value),
            // What Claude reports about the server's reachability, not its entry.
            "Status" => {}
            "Environment" if value.is_empty() => {}
            _ => return false,
        }
    }
    scope.is_some_and(|scope| scope.starts_with("User config"))
        && kind == Some("stdio")
        && command == Some("zaf")
        && args == Some("mcp")
}

/// `claude mcp <rest>` with `CLAUDE_CONFIG_DIR` set to `config_dir`; `None` leaves it unset,
/// which is the Engineer's own Claude.
fn mcp_line(config_dir: Option<&Path>, rest: &str) -> String {
    match config_dir {
        Some(dir) => format!(
            "{CONFIG_DIR_VAR}={} claude mcp {rest}",
            single_quote(&dir.to_string_lossy())
        ),
        None => format!("claude mcp {rest}"),
    }
}

/// `claude mcp get zaf`.
pub fn mcp_get_line(config_dir: Option<&Path>) -> String {
    mcp_line(config_dir, &format!("get {ZAF_MCP_NAME}"))
}

/// `claude mcp add-json --scope user zaf '<entry>'`.
pub fn mcp_add_line(config_dir: Option<&Path>) -> String {
    mcp_line(
        config_dir,
        &format!(
            "add-json --scope user {ZAF_MCP_NAME} {}",
            single_quote(ZAF_MCP_ENTRY)
        ),
    )
}

/// `claude mcp remove --scope user zaf`.
pub fn mcp_remove_line(config_dir: Option<&Path>) -> String {
    mcp_line(config_dir, &format!("remove --scope user {ZAF_MCP_NAME}"))
}

/// What `claude mcp get zaf` prints once zaf's entry is registered, for the tests' fake shells.
#[cfg(test)]
pub(crate) const GET_SHOWS_OURS: &str = "zaf:\n  Scope: User config (available in all your projects)\n  Status: \u{2713} Connected\n  Type: stdio\n  Command: zaf\n  Args: mcp\n  Environment:\n\nTo remove this server, run: claude mcp remove \"zaf\" -s user\n";

/// What registering did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Registered {
    /// The entry was there; nothing was issued after the `get`.
    Already,
    /// The entry was added.
    Added,
    /// Another entry was removed and ours added.
    Replaced,
}

/// Brings the `zaf` server in the Claude config at `home` to [`ZAF_MCP_ENTRY`].
///
/// Issues `get` first and nothing more when it shows our entry; adds when it shows nothing;
/// removes and adds when it shows another `zaf` entry (a profile is zaf's own, never hand-edited).
/// A failed `remove` or `add-json` is an error naming the command.
pub(super) fn register_mcp(shell: &dyn Shell, home: &Path) -> Result<Registered, ProfileError> {
    let get = mcp_get_line(Some(home));
    let ran = shell
        .run(&CommandLine::from(get.clone()))
        .map_err(|error| ProfileError::Install {
            line: get,
            output: error.to_string(),
        })?;
    match McpRegistration::from_get(&ran.output, ran.verdict) {
        McpRegistration::Ours => Ok(Registered::Already),
        McpRegistration::Missing => {
            super::run(shell, &mcp_add_line(Some(home)))?;
            Ok(Registered::Added)
        }
        McpRegistration::Other(_) => {
            super::run(shell, &mcp_remove_line(Some(home)))?;
            super::run(shell, &mcp_add_line(Some(home)))?;
            Ok(Registered::Replaced)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = GET_SHOWS_OURS;

    #[test]
    fn a_failed_or_empty_get_shows_nothing() {
        let said = "No MCP server found with name: zaf\n";
        assert_eq!(
            McpRegistration::from_get(said, GateVerdict::Failed),
            McpRegistration::Missing
        );
        assert_eq!(
            McpRegistration::from_get("", GateVerdict::Passed),
            McpRegistration::Missing
        );
    }

    #[test]
    fn our_entry_at_user_scope_is_ours() {
        assert_eq!(
            McpRegistration::from_get(OURS, GateVerdict::Passed),
            McpRegistration::Ours
        );
    }

    #[test]
    fn any_other_zaf_entry_is_other() {
        let other = |text: String| {
            assert_eq!(
                McpRegistration::from_get(&text, GateVerdict::Passed),
                McpRegistration::Other(text.trim().to_owned()),
                "{text}"
            );
        };
        other(OURS.replace("Command: zaf", "Command: /opt/zaf"));
        other(OURS.replace("Args: mcp", "Args: mcp --verbose"));
        other(OURS.replace("Type: stdio", "Type: http"));
        other(OURS.replace("User config", "Local config"));
        other(OURS.replace("  Environment:\n", "  Environment:\n    TOKEN=abc\n"));
        other(OURS.replace("  Environment:\n", "  URL: http://localhost:1\n"));
        other("something else entirely\n".to_owned());
    }

    #[test]
    fn the_lines_name_the_config_dir_or_leave_it_unset() {
        let home = Path::new("/p/.zaf/profiles/lean");
        assert_eq!(
            mcp_get_line(Some(home)),
            "CLAUDE_CONFIG_DIR='/p/.zaf/profiles/lean' claude mcp get zaf"
        );
        assert_eq!(
            mcp_add_line(Some(home)),
            "CLAUDE_CONFIG_DIR='/p/.zaf/profiles/lean' claude mcp add-json --scope user zaf '{\"type\":\"stdio\",\"command\":\"zaf\",\"args\":[\"mcp\"]}'"
        );
        assert_eq!(
            mcp_remove_line(Some(home)),
            "CLAUDE_CONFIG_DIR='/p/.zaf/profiles/lean' claude mcp remove --scope user zaf"
        );
        assert_eq!(mcp_get_line(None), "claude mcp get zaf");
        assert_eq!(
            mcp_add_line(None),
            "claude mcp add-json --scope user zaf '{\"type\":\"stdio\",\"command\":\"zaf\",\"args\":[\"mcp\"]}'"
        );
        assert_eq!(mcp_remove_line(None), "claude mcp remove --scope user zaf");
    }
}
