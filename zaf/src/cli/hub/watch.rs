//! The hub's watch (`POST /api/watch`, [`crate::app::hub::controls`]): `zaf spawn --watch` for a
//! project, run inside the hub's process by [`crate::app::spawn::watch`].
//!
//! The loop, its lock and its stop are the hub's own and end with it. Each tick of the loop is the
//! verb's: `zaf spawn --once --version <v>` run as a process of its own with the project's root as
//! its folder, as `POST /api/tick` runs one, since the tick finds its project from the folder it
//! runs in and the hub serves every project from one. A tick that finds the hub holding the
//! project's lock would only ask the scheduler for a tick, so it is told whose tick it is
//! ([`SCHEDULED_BY_VAR`]) and runs. The tick is a process group of its own and the hub waits for it
//! to its end: a Ctrl-C meant for the hub never reaches a tick half done, and a hub that ends leaves
//! the tick running to its end.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use super::super::board::INSTANCE_VAR;
use super::tick::RUN_VAR;
use crate::app::actions::MINUTE;
use crate::app::hub::controls::{Failure, WatchStarted, WatchStopped, Watcher};
use crate::app::schedule::binary;
use crate::app::schedule::scheduler::Asked;
use crate::app::spawn::watch::{SCHEDULED_BY_VAR, Tick, WatchHandle};
use crate::composition::{Composition, STATE_DIR};

/// Starts and stops the watches of the hub's [`Watches`](crate::app::spawn::watch::Watches).
#[derive(Debug, Clone)]
pub struct Hub {
    watches: WatchHandle,
    home: Option<PathBuf>,
    program: Option<PathBuf>,
}

impl Hub {
    /// The watches of `watches` over the projects of the zaf home `home`, each tick run as the
    /// binary this hub was started from.
    pub fn new(watches: WatchHandle, home: Option<PathBuf>) -> Self {
        Self {
            watches,
            home,
            program: None,
        }
    }

    /// [`new`](Self::new), each tick run as `program`, which takes `zaf`'s arguments.
    pub fn of(watches: WatchHandle, home: Option<PathBuf>, program: PathBuf) -> Self {
        Self {
            watches,
            home,
            program: Some(program),
        }
    }
}

impl Watcher for Hub {
    fn start(&self, root: &Path, version: &str) -> Result<WatchStarted, Failure> {
        let composition = Composition::discover_with_home(root, self.home.as_deref())
            .map_err(|error| Failure::Failed(error.to_string()))?;
        let program = match &self.program {
            Some(program) => program.clone(),
            None => binary::path_to_start().map_err(|why| {
                Failure::Failed(format!("zaf cannot be started to run the ticks: {why}"))
            })?,
        };
        let minutes = composition.manifest().spawn.tick_minutes;
        let tick = Process {
            program,
            root: composition.root().to_owned(),
            version: version.to_owned(),
        };
        self.watches
            .start(
                &composition.state_dir(),
                version,
                MINUTE * minutes,
                Arc::new(tick),
            )
            .map_err(Failure::from)
    }

    fn stop(&self, root: &Path) -> Result<WatchStopped, Failure> {
        self.watches
            .stop(&root.join(STATE_DIR))
            .map_err(Failure::from)
    }
}

/// One project's tick, run as a `zaf` process.
struct Process {
    program: PathBuf,
    root: PathBuf,
    version: String,
}

impl Process {
    /// The tick as a command: the verb, in the project's root, for the scheduler of this process.
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(["spawn", "--once", "--version", &self.version])
            .current_dir(&self.root)
            // The hub is no instance, and no phase's run: neither is a tick it runs.
            .env_remove(INSTANCE_VAR)
            .env_remove(RUN_VAR)
            .env(SCHEDULED_BY_VAR, std::process::id().to_string())
            .stdin(Stdio::null())
            // What the tick did is in the project's trace; what went wrong is the hub's to say.
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .process_group(0);
        command
    }
}

impl Tick for Process {
    fn tick(&self, asked: &Asked) -> u8 {
        match self.command().status() {
            Ok(status) => status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(2),
            Err(error) => {
                self.say(&format!(
                    "cannot run a tick ({}) for {}: {error}",
                    asked.line(),
                    self.root.display()
                ));
                2
            }
        }
    }

    fn say(&self, line: &str) {
        eprintln!("zaf hub: watch {}: {line}", self.root.display());
    }
}

#[cfg(test)]
mod tests;
