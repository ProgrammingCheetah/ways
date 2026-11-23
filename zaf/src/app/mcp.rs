//! The MCP server's use case: a tool call from request to result, through the verb runner.
//!
//! Nothing here names the protocol crate or an async runtime: the server adapter turns its
//! requests into [`ToolCall`]s and the results back.

pub mod caller;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::domain::{
    Cancel, CommandPath, ForbiddenSection, McpCaller, McpVerdict, Policy, Tick, ToolCall,
    ToolResult, ToolSpec,
};
use crate::ports::{VerbRun, VerbRunner};

/// The program name that opens every command line the service builds.
pub const PROGRAM: &str = "zaf";

/// The exit code of a line the checker refuses, as a usage error exits.
pub const USAGE_EXIT: i32 = 2;

/// The exit code of a call the policy or the caller probe refuses: refused by the state (I1, D4).
pub const REFUSED_EXIT: i32 = 1;

/// Who calls, as the probe answered once at the server's start: the caller, or the reason the
/// caller could not be told.
pub type CallerAnswer = Result<McpCaller, String>;

/// Checks a command line the way the binary's parser would, without running it. The clap
/// catalog implements it, so this layer never names the parser.
pub trait LineChecker: Send + Sync {
    /// The parsed command's path when `argv` (the program name first) parses, else the usage
    /// message.
    fn check(&self, argv: &[String]) -> Result<CommandPath, String>;
}

/// What was learned of the child binary the last time it was looked at.
#[derive(Debug)]
struct Seen {
    stamp: Option<String>,
    version: String,
}

/// Knows the server's own version and learns the child binary's: the child's `--version` is
/// read again only when the binary's stamp changed, so a binary costs one run, not one per call.
#[derive(Debug)]
pub struct VersionWatch {
    own: String,
    seen: Mutex<Option<Seen>>,
}

impl VersionWatch {
    /// A watch for a server of version `own`.
    pub fn new(own: impl Into<String>) -> Self {
        Self {
            own: own.into(),
            seen: Mutex::new(None),
        }
    }

    /// The version the child reports when it differs from the server's own, else `None`.
    fn updated_to(&self, runner: &dyn VerbRunner, cwd: &Path, cancel: &Cancel) -> Option<String> {
        // A binary that cannot be read cannot be asked either: the call itself says why.
        let stamp = Some(runner.binary_stamp()?);
        // Held across the read, so concurrent calls read a changed binary once between them.
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let version = match seen.as_ref() {
            Some(known) if known.stamp == stamp => known.version.clone(),
            _ => {
                let outcome = runner.run(
                    VerbRun {
                        argv: vec![PROGRAM.to_owned(), "--version".to_owned()],
                        stdin: None,
                        cwd: cwd.to_path_buf(),
                    },
                    &|_| {},
                    cancel,
                );
                // A read that failed is not remembered: the next call tries again.
                let Some(version) = (outcome.exit == 0)
                    .then(|| version_of(&outcome.stdout))
                    .flatten()
                else {
                    *seen = None;
                    return None;
                };
                *seen = Some(Seen {
                    stamp,
                    version: version.clone(),
                });
                version
            }
        };
        (version != self.own).then_some(version)
    }
}

/// The version in a `--version` output (`zaf 0.5.4`): the last word of its first line.
fn version_of(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().last())
        .map(str::to_owned)
}

/// The line that ends a result when the child's version is not the server's.
fn updated_line(version: &str) -> String {
    format!("zaf was updated to {version}; restart zaf's MCP (/mcp) for the new tool list")
}

/// Lists the tools and runs a call to one.
pub struct ToolService<'a> {
    specs: Vec<ToolSpec>,
    checker: &'a dyn LineChecker,
    runner: &'a dyn VerbRunner,
    cwd: PathBuf,
    watch: &'a VersionWatch,
    caller: CallerAnswer,
    forbidden: ForbiddenSection,
}

impl<'a> ToolService<'a> {
    /// A service over `specs`, checking lines with `checker` and running them through
    /// `runner` in `cwd`; `watch` tells when the child binary is not the server's version.
    /// `caller` is the probe's answer, read once at the server's start, and `forbidden` the
    /// manifest's `[forbidden]`: both decide a call before any child starts.
    pub fn new(
        specs: Vec<ToolSpec>,
        checker: &'a dyn LineChecker,
        runner: &'a dyn VerbRunner,
        cwd: PathBuf,
        watch: &'a VersionWatch,
        caller: CallerAnswer,
        forbidden: ForbiddenSection,
    ) -> Self {
        Self {
            specs,
            checker,
            runner,
            cwd,
            watch,
            caller,
            forbidden,
        }
    }

    /// The tools, as handed in.
    pub fn list(&self) -> Vec<ToolSpec> {
        self.specs.clone()
    }

    /// Runs one call. A caller the probe could not tell refuses every call with its reason; a
    /// line the checker refuses returns its message with exit 2; a command the policy refuses
    /// returns one line, what was refused and the way on, with exit 1. None of these starts a
    /// child. A call that passes runs and returns the verb's output, shaped by [`shape`].
    pub fn call(&self, call: ToolCall, progress: &dyn Fn(Tick), cancel: &Cancel) -> ToolResult {
        let caller = match &self.caller {
            Ok(caller) => caller,
            Err(reason) => return refused(&format!("zaf's MCP refuses every call: {reason}")),
        };
        let argv = line_of(&call);
        let checked = self.checker.check(&argv);
        if let Ok(path) = &checked
            && let McpVerdict::Refuse { what, way_on } =
                Policy::judge(caller, path, &self.forbidden)
        {
            return refused(&format!("{what} is refused: {way_on}"));
        }
        let updated = self
            .watch
            .updated_to(self.runner, &self.cwd, cancel)
            .map(|version| updated_line(&version));
        if let Err(message) = checked {
            return shape(String::new(), message, USAGE_EXIT, updated.as_deref());
        }
        let run = VerbRun {
            argv,
            stdin: call.stdin,
            cwd: self.cwd.clone(),
        };
        let outcome = self.runner.run(run, progress, cancel);
        shape(
            outcome.stdout,
            outcome.stderr,
            outcome.exit,
            updated.as_deref(),
        )
    }
}

/// The result of a refused call: `line` on one line, then `exit 1`.
fn refused(line: &str) -> ToolResult {
    let one_line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    shape(String::new(), one_line, REFUSED_EXIT, None)
}

/// One block of text, stdout then stderr, then the `updated` line when there is one, with
/// `exit <n>` as the last line when `exit` is not 0 (so the update line comes just before it);
/// `is_error` is true exactly then, and the exit is carried either way.
fn shape(stdout: String, stderr: String, exit: i32, updated: Option<&str>) -> ToolResult {
    let mut text = stdout;
    append_line_ended(&mut text, &stderr);
    if let Some(line) = updated {
        append_line_ended(&mut text, line);
    }
    if exit != 0 {
        append_line_ended(&mut text, &format!("exit {exit}"));
    }
    ToolResult {
        text,
        is_error: exit != 0,
        exit,
    }
}

/// Appends `part` to `text`, starting it on a new line when `text` does not end in one.
fn append_line_ended(text: &mut String, part: &str) {
    if part.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(part);
}

/// `zaf [--json] [--more] <verb> <args…>`.
fn line_of(call: &ToolCall) -> Vec<String> {
    let mut argv = vec![PROGRAM.to_owned()];
    if call.json {
        argv.push("--json".into());
    }
    if call.more {
        argv.push("--more".into());
    }
    argv.push(call.tool.clone());
    argv.extend(call.args.iter().cloned());
    argv
}

#[cfg(test)]
mod tests;
