//! The shell: one command line run to completion, with its exit status and output.

use super::{GateRun, PortError};
use crate::manifest::CommandLine;

/// Runs a command line the caller composed itself: a `command:` step or a named test.
///
/// A line is run by `sh -c` in the directory and under the time bound the adapter
/// was built with. The result is the [`GateRun`] shape (a verdict and the combined
/// output): the gate port runs only the levels the manifest declares, while this
/// port runs any line, so the two share the shape of a finished run and nothing else.
pub trait Shell {
    /// Runs the line and waits. A non-zero exit is a finished run with a failed verdict;
    /// a shell that cannot start, or a run past its time bound, is [`PortError::Unavailable`].
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError>;
}
