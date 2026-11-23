//! The verb runner: runs one verb of the binary and captures what it produced.

use std::path::PathBuf;

use crate::domain::{Cancel, Tick};

/// One run of a verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbRun {
    /// The command line, as built: the program name first (`zaf`), then the flags, the verb
    /// and its arguments. The adapter runs the binary on disk in the program name's place.
    pub argv: Vec<String>,
    /// What the verb reads on its standard input, if anything.
    pub stdin: Option<String>,
    /// The directory the verb runs in.
    pub cwd: PathBuf,
}

/// What a verb run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbOutcome {
    /// What the verb wrote to standard output.
    pub stdout: String,
    /// What the verb wrote to standard error.
    pub stderr: String,
    /// The verb's exit code.
    pub exit: i32,
}

/// Runs a verb to its end, synchronously; no async type crosses this port.
///
/// The adapter calls `tick` while the run goes and stops the run when `cancel` is raised. A
/// run it could not start is an outcome too, with its reason on `stderr` and exit 2.
pub trait VerbRunner: Send + Sync {
    /// Runs `run`, reporting progress through `tick`, and returns what it produced.
    fn run(&self, run: VerbRun, tick: &dyn Fn(Tick), cancel: &Cancel) -> VerbOutcome;

    /// A stamp of the binary a run would start now: it differs when the file at that path was
    /// replaced or changed, and is the same while it is not. `None` when the binary cannot be
    /// read. It lets a caller tell that the binary changed without running it.
    fn binary_stamp(&self) -> Option<String>;
}
