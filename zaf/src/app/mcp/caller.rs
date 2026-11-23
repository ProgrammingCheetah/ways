//! Who calls the MCP server, read once at start (design D7, I4, M7).
//!
//! The answer is one of three: an outside caller (the Engineer's own session), an instance of a
//! role, or a refusal that names its reason. The cases, in order:
//!
//! 1. `ZAF_INSTANCE` set and parsing: that instance's role ([`Caller::from_value`]).
//! 2. `ZAF_INSTANCE` set and not parsing, blank included: a refusal naming the value. A bad value
//!    is never read as outside, so the probe fails closed.
//! 3. No `ZAF_INSTANCE`, yet a running launch above this process
//!    ([`instance_above`], the sign guard's own question): a refusal naming that instance, which
//!    has lost its name.
//! 4. Neither: outside.
//!
//! A process tree or a run folder that cannot be read is a refusal too: not being able to tell
//! is not being outside. This guards against a model's mistake, as the sign guard does: a process
//! that reparents itself away from its instance is not seen.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::adapters::process;
use crate::app::actions::{Caller, INSTANCE_VAR};
use crate::app::epic::instance_above;
use crate::app::spawn::runs;
use crate::domain::McpCaller;
use crate::ports::Holder;

/// Why the server could not tell who calls it. Each text names its reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// `ZAF_INSTANCE` is set to a value that is no instance name; the text carries the value.
    #[error("{0}")]
    UnparsedInstance(String),
    /// `ZAF_INSTANCE` is absent, yet this instance runs above the server.
    #[error("{INSTANCE_VAR} is not set, but {0} runs above this server: it has lost its name")]
    NamelessInstance(String),
    /// The process tree is empty, so no launch above could be ruled out.
    #[error("cannot read the process tree, so cannot tell whether an instance runs above")]
    TreeUnreadable,
    /// The run folders cannot be read, so no launch above could be ruled out.
    #[error("cannot read the run folders ({0}), so cannot tell whether an instance runs above")]
    RunsUnreadable(String),
}

/// Reads who calls the server (I4).
pub trait CallerProbe: Send + Sync {
    /// The caller, or the reason it cannot be told.
    fn caller(&self) -> Result<McpCaller, Refusal>;
}

/// Whether a recorded process still runs.
type Alive = Box<dyn Fn(&Holder) -> bool + Send + Sync>;

/// The probe over the process: its environment, its ancestors and the project's run folders.
pub struct ProcessProbe {
    instance: Option<OsString>,
    ancestors: Vec<Holder>,
    state_dir: PathBuf,
    alive: Alive,
}

impl ProcessProbe {
    /// A probe over a given `ZAF_INSTANCE` value, process chain (this process and every one above
    /// it) and liveness check, reading the run folders under `state_dir`.
    pub fn new(
        instance: Option<OsString>,
        ancestors: Vec<Holder>,
        state_dir: PathBuf,
        alive: impl Fn(&Holder) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            instance,
            ancestors,
            state_dir,
            alive: Box::new(alive),
        }
    }

    /// The probe of this very process: its environment and its place in the process table.
    pub fn of_this_process(state_dir: PathBuf) -> Self {
        Self::new(
            std::env::var_os(INSTANCE_VAR),
            process::ancestors(),
            state_dir,
            process::alive,
        )
    }
}

impl CallerProbe for ProcessProbe {
    fn caller(&self) -> Result<McpCaller, Refusal> {
        match Caller::from_value(self.instance.as_deref()) {
            Ok(Caller::Role(role)) => return Ok(McpCaller::Instance(role)),
            Err(unparsed) => return Err(Refusal::UnparsedInstance(unparsed)),
            // No name given: the process tree still has a say.
            Ok(Caller::Engineer) => {}
        }
        if self.ancestors.is_empty() {
            return Err(Refusal::TreeUnreadable);
        }
        let runs = runs::read_all(&self.state_dir)
            .map_err(|error| Refusal::RunsUnreadable(error.to_string()))?;
        match instance_above(&self.ancestors, &runs, &*self.alive) {
            Some(instance) => Err(Refusal::NamelessInstance(instance)),
            None => Ok(McpCaller::Outside),
        }
    }
}

#[cfg(test)]
mod tests;
