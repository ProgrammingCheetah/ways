//! `zaf check`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

use crate::domain::IssueNumber;

/// Runs the configured check steps for a target, or reads the last result.
pub fn run(cli: &Cli, _target: &str, _status: bool) -> u8 {
    not_built(&cli.verb.path())
}

/// Checks a task's acceptance lines and declared interfaces (`check <task>` by another name).
pub fn verify(cli: &Cli, _task: IssueNumber) -> u8 {
    not_built(&cli.verb.path())
}
