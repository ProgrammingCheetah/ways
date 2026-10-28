//! `zaf changes`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

use crate::domain::IssueNumber;

/// Shows a task's change: stat first, hunks on request.
pub fn run(cli: &Cli, _task: IssueNumber, _path: Option<&str>) -> u8 {
    not_built(&cli.verb.path())
}
