//! `zaf ask`: not built yet; the use case lands in `crate::app` and is wired here.

use super::Cli;
use super::exit::not_built;

use crate::domain::IssueNumber;

/// Asks the keeper up to five questions.
pub fn run(cli: &Cli, _task: IssueNumber, _questions: &[String]) -> u8 {
    not_built(&cli.verb.path())
}
