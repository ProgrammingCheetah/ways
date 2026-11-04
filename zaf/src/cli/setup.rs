//! `zaf setup`: make the two homes, then hand the Engineer what it cannot do.

use super::doctor::ShellCommands;
use super::exit::{self, OK, REFUSED};
use super::{Cli, first_run};

use crate::app::setup::setup;
use crate::composition::{Composition, zaf_home};

/// Makes the project's `.zaf/` and the zaf home, replaces an old `.zaf/.gitignore` that holds
/// only `*`, clones a missing design repo, runs doctor and prints the worksheet; exits 1 while
/// anything is left for the Engineer. Installs nothing. Outside a project it makes the zaf
/// home and refuses.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => {
            first_run();
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let report = match setup(
        composition.root(),
        zaf_home().as_deref(),
        composition.manifest(),
        &ShellCommands::new(composition.shell()),
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
