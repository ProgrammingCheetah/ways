//! `zaf setup`: prepare `.zaf/`, then hand the Engineer what it cannot do.

use super::Cli;
use super::doctor::SystemCommands;
use super::exit::{self, OK, REFUSED};

use crate::app::setup::setup;
use crate::composition::Composition;

/// Prepares the state folder, clones a missing design repo, runs doctor and prints the
/// worksheet; exits 1 while anything is left for the Engineer. Installs nothing.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let report = match setup(
        composition.root(),
        &composition.state_dir(),
        composition.manifest(),
        &SystemCommands,
    ) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return exit::BACKEND;
        }
    };
    for line in report.doctor.lines() {
        println!("{line}");
    }
    for line in report.worksheet_lines() {
        println!("{line}");
    }
    if report.is_ready() { OK } else { REFUSED }
}
