//! The MCP server's CLI layer: it reads clap's `Cli`, so it lives here; the app layer sees it
//! only through [`crate::app::mcp::LineChecker`].
//!
//! `zaf mcp` is the hidden verb that serves the catalog over stdio. It is the one verb with a
//! tokio runtime: it composes the catalog, the verb-process runner and the app layer's
//! [`ToolService`], and hands them to the server adapter.

pub mod catalog;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::CommandFactory;

use crate::adapters::mcp_rmcp::McpServer;
use crate::adapters::verb_process::{VerbProcess, directory};
use crate::app::mcp::caller::{CallerProbe, ProcessProbe};
use crate::app::mcp::{CallerAnswer, ToolService, VersionWatch};
use crate::composition::{Composition, CompositionError, STATE_DIR};
use crate::domain::{Cancel, ForbiddenSection, Tick, ToolCall, ToolResult, ToolSpec};
use crate::ports::ToolHost;

pub use catalog::Catalog;

use super::{Cli, exit};

/// The tool host over the catalog and the verb-process runner: it builds a [`ToolService`] for
/// each use, since the service borrows what it runs through.
struct Host {
    specs: Vec<ToolSpec>,
    catalog: Catalog,
    runner: VerbProcess,
    cwd: PathBuf,
    /// Remembers the child binary's version across calls.
    watch: VersionWatch,
    /// Who calls, read once when the server started.
    caller: CallerAnswer,
    /// The manifest's `[forbidden]`.
    forbidden: ForbiddenSection,
}

/// What the server reads once at its start, before it serves: who calls and what the manifest
/// forbids. A manifest that cannot be read refuses every call, since `[forbidden]` is unknown.
struct Start {
    caller: CallerAnswer,
    forbidden: ForbiddenSection,
}

impl Start {
    /// Reads the manifest found from `cwd` and probes `probe` for the caller. A directory with
    /// no project has no `[forbidden]`; the probe then reads the run folders of `cwd`'s `.zaf`.
    fn read(cwd: &Path, probe: impl FnOnce(PathBuf) -> Box<dyn CallerProbe>) -> Self {
        let (state_dir, forbidden) = match Composition::discover(cwd) {
            Ok(project) => {
                let commands = &project.manifest().forbidden.commands;
                let forbidden = ForbiddenSection::new(commands.iter().map(AsRef::as_ref));
                (project.state_dir(), Ok(forbidden))
            }
            Err(CompositionError::NoProject { .. }) => {
                (cwd.join(STATE_DIR), Ok(ForbiddenSection::default()))
            }
            Err(error) => (
                cwd.join(STATE_DIR),
                Err(format!("cannot read the manifest: {error}")),
            ),
        };
        let caller = probe(state_dir)
            .caller()
            .map_err(|refusal| refusal.to_string());
        match forbidden {
            Ok(forbidden) => Self { caller, forbidden },
            // The caller's own refusal reads first; else the manifest's.
            Err(reason) => Self {
                caller: caller.and(Err(reason)),
                forbidden: ForbiddenSection::default(),
            },
        }
    }
}

impl Host {
    /// A host of the tools of `command` (hidden verbs are left out), running verbs in `cwd`
    /// for `start`'s caller under `start`'s `[forbidden]`.
    fn new(command: &clap::Command, runner: VerbProcess, cwd: PathBuf, start: Start) -> Self {
        Self {
            specs: Catalog::from_command(command),
            catalog: Catalog::new(command.clone()),
            runner,
            cwd,
            watch: VersionWatch::new(env!("CARGO_PKG_VERSION")),
            caller: start.caller,
            forbidden: start.forbidden,
        }
    }

    fn service(&self) -> ToolService<'_> {
        ToolService::new(
            self.specs.clone(),
            &self.catalog,
            &self.runner,
            self.cwd.clone(),
            &self.watch,
            self.caller.clone(),
            self.forbidden.clone(),
        )
    }
}

impl ToolHost for Host {
    fn list(&self) -> Vec<ToolSpec> {
        self.service().list()
    }

    fn call(&self, call: ToolCall, progress: &dyn Fn(Tick), cancel: &Cancel) -> ToolResult {
        self.service().call(call, progress, cancel)
    }
}

/// `zaf mcp`: serves the tools over this process's stdio until the client closes it. Exit 0
/// when the client left, 2 when the server could not start or broke.
pub fn run(_cli: &Cli) -> u8 {
    let cwd = directory(
        std::env::var_os("ZAF_ROOT"),
        std::env::var_os("CLAUDE_PROJECT_DIR"),
        || std::env::current_dir().ok(),
    )
    .unwrap_or_else(|| PathBuf::from("."));
    let start = Start::read(&cwd, |state_dir| {
        Box::new(ProcessProbe::of_this_process(state_dir))
    });
    let host = Host::new(&Cli::command(), VerbProcess::new(), cwd, start);
    let server = McpServer::new(Arc::new(host));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::eprintln_zaf!("zaf mcp: cannot start the runtime: {error}");
            return exit::BACKEND;
        }
    };
    let served = runtime.block_on(server.serve_stdio());
    // A read of stdin that is still pending would hold the runtime's drop: leave it behind.
    runtime.shutdown_background();
    match served {
        Ok(()) => exit::OK,
        Err(error) => {
            crate::eprintln_zaf!("zaf mcp: {error}");
            exit::BACKEND
        }
    }
}
