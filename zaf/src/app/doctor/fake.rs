//! A faked [`Commands`]: canned output per program, and a record of every call.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Mutex, PoisonError};

use super::{CommandOutput, Commands};

/// What `claude mcp get zaf` prints once zaf's entry is registered.
const SHOWS_OURS: &str = "zaf:\n  Scope: User config (available in all your projects)\n  Status: \u{2713} Connected\n  Type: stdio\n  Command: zaf\n  Args: mcp\n  Environment:\n\nTo remove this server, run: claude mcp remove \"zaf\" -s user\n";

/// Programs that answer with fixed text; any other program is not installed.
#[derive(Debug, Default)]
pub struct FakeCommands {
    answers: BTreeMap<String, CommandOutput>,
    calls: Mutex<Vec<Vec<String>>>,
    /// Whether a profile's `claude mcp get zaf` shows zaf's entry.
    profile_mcp: bool,
}

impl FakeCommands {
    /// A fake with nothing installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a program that answers every call with `text` and success.
    pub fn with(mut self, program: &str, text: &str) -> Self {
        self.answers.insert(
            program.to_owned(),
            CommandOutput {
                text: text.to_owned(),
                success: true,
            },
        );
        self
    }

    /// Installs a program whose every call fails with `text`.
    pub fn failing(mut self, program: &str, text: &str) -> Self {
        self.answers.insert(
            program.to_owned(),
            CommandOutput {
                text: text.to_owned(),
                success: false,
            },
        );
        self
    }

    /// Makes `env CLAUDE_CONFIG_DIR=<home> claude mcp get zaf`, which asks a profile for its MCP
    /// server, show zaf's entry; every other call answers as before.
    pub fn with_profile_mcp(mut self) -> Self {
        self.profile_mcp = true;
        self
    }

    /// Every call so far, program first.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Commands for FakeCommands {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|arg| (*arg).to_owned()));
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
        if self.profile_mcp && program == "env" && args.ends_with(&["claude", "mcp", "get", "zaf"])
        {
            return Ok(CommandOutput {
                text: SHOWS_OURS.to_owned(),
                success: true,
            });
        }
        self.answers.get(program).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{program}: not installed"))
        })
    }
}
