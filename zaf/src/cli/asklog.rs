//! `zaf asklog`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

/// Prints the ask log's summary.
pub fn run(cli: &Cli) -> u8 {
    not_built(&cli.verb.path())
}
