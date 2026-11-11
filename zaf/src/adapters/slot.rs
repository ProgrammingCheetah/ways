//! The machine's build slot, as `[gate] slot` names it.
//!
//! The slot is a command prefix the machine provides, which waits until a build may run
//! and then runs its arguments. A slotted gate line runs as `<slot> sh -c '<line>'`, with
//! `ZAF_SLOT_HELD=1` exported inside the shell, so the line and everything it starts
//! (an action, a nested `zaf check`) know the slot is held and take no second one. While
//! the run lasts, zaf keeps `.zaf/slots/<pid>`, which `zaf status` reads.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::process;
use super::runner::single_quote;
use crate::domain::{SLOTS_DIR, SlotRecord};
use crate::ports::PortError;

/// The variable a held slot exports. A nested run that finds it set takes no slot.
pub const HELD_VAR: &str = "ZAF_SLOT_HELD";

/// The variable naming the instance that runs.
const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// The holder recorded for a run no instance started.
const ENGINEER: &str = "the Engineer";

/// Whether this process already runs inside a held slot.
pub fn held_here() -> bool {
    std::env::var_os(HELD_VAR).is_some()
}

/// The holder this process records: the instance `ZAF_INSTANCE` names, else the Engineer.
pub fn holder_here() -> String {
    std::env::var(INSTANCE_VAR)
        .ok()
        .filter(|instance| !instance.trim().is_empty())
        .unwrap_or_else(|| ENGINEER.to_owned())
}

/// A slot to run lines inside: the command prefix, the state folder its records go under
/// and who the records name as the holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    prefix: String,
    state_dir: PathBuf,
    holder: String,
}

impl Slot {
    /// A slot behind `prefix`, with its records under `state_dir`'s `slots/` folder.
    ///
    /// An inherent constructor: it joins a command, a folder and a name, which no std
    /// conversion takes.
    pub fn new(prefix: &str, state_dir: &Path, holder: &str) -> Self {
        Self {
            prefix: prefix.trim().to_owned(),
            state_dir: state_dir.to_owned(),
            holder: holder.to_owned(),
        }
    }

    /// The line to hand to `sh -c` so `line` runs inside the slot.
    pub fn command(&self, line: &str) -> String {
        let inside = format!("export {HELD_VAR}=1\n{line}");
        format!("{} sh -c {}", self.prefix, single_quote(&inside))
    }

    /// Keeps this process's record until the returned guard drops, for one run at `level`.
    pub fn hold(&self, level: &str) -> Result<Held, PortError> {
        let folder = self.state_dir.join(SLOTS_DIR);
        std::fs::create_dir_all(&folder).map_err(|error| file_error(&folder, &error))?;
        let pid = std::process::id();
        let path = folder.join(pid.to_string());
        let record = SlotRecord {
            holder: self.holder.clone(),
            level: level.to_owned(),
            since: humantime::format_rfc3339_seconds(SystemTime::now()).to_string(),
            start: process::holder_of(pid)
                .map(|holder| holder.start)
                .unwrap_or_default(),
        };
        let text = serde_json::to_string(&record)
            .map_err(|error| PortError::Backend(format!("slot record: {error}")))?;
        std::fs::write(&path, text).map_err(|error| file_error(&path, &error))?;
        Ok(Held { path })
    }
}

/// A slot record kept for the length of a run; dropping it removes the file.
#[derive(Debug)]
pub struct Held {
    path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn file_error(path: &Path, error: &std::io::Error) -> PortError {
    PortError::Backend(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
