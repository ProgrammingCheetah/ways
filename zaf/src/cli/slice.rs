//! `zaf slice …`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

/// Runs a slice action.
pub fn run(cli: &Cli, _action: &super::SliceAction) -> u8 {
    not_built(&cli.verb.path())
}
