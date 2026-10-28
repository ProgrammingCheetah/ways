//! `zaf brief`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

use crate::domain::IssueNumber;

/// Builds a task's context pack.
pub fn run(cli: &Cli, _task: IssueNumber) -> u8 {
    not_built(&cli.verb.path())
}
