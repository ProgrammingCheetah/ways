//! `zaf setup`: move a root `ZAF.toml`, make the two homes, generate the profiles, then hand
//! the Engineer what it cannot do.

use std::io::ErrorKind;

use super::brief::INSTANCE_VAR;
use super::doctor::ShellCommands;
use super::exit::{self, OK, REFUSED};
use super::{Cli, first_run};

use crate::app::setup::{move_legacy_manifest, setup};
use crate::composition::{Composition, zaf_home};

/// Whether this run is an instance's: `ZAF_INSTANCE` set and not blank.
fn as_instance() -> bool {
    std::env::var_os(INSTANCE_VAR).is_some_and(|name| !name.to_string_lossy().trim().is_empty())
}

/// Renames a root `ZAF.toml` to `.zaf.toml` (the Engineer's run only, said in one line),
/// makes the project's `.zaf/` and the zaf home, replaces an old `.zaf/.gitignore` that holds
/// only `*`, clones a missing design repo, generates the profiles as `zaf profile init` does,
/// runs doctor and prints the worksheet; exits 1 while anything is left for the Engineer (a
/// pack refusal included) and 2 when a profile cannot be generated. A `ZAF.toml` beside a new manifest file exits 1 and
/// moves nothing. Installs nothing. Outside a project it makes the zaf home and refuses.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    // The one verb that does not warn about the retired name: it is the one that moves it.
    let composition = match Composition::discover_here_quiet() {
        Ok(composition) => composition,
        Err(error) => {
            first_run();
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let zaf_home = zaf_home();
    match move_legacy_manifest(composition.root(), zaf_home.as_deref(), as_instance()) {
        Ok(moved) => {
            if let Some(line) = moved.line() {
                println!("{line}");
            }
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return if error.kind() == ErrorKind::AlreadyExists {
                REFUSED
            } else {
                exit::BACKEND
            };
        }
    }
    let report = match setup(
        composition.root(),
        zaf_home.as_deref(),
        composition.manifest(),
        &ShellCommands::new(composition.shell()),
        composition.shell().as_ref(),
    ) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return exit::BACKEND;
        }
    };
    if let Some(profiles) = report
        .profiles
        .as_ref()
        .filter(|profiles| !profiles.unchanged())
    {
        for line in profiles.lines() {
            println!("{line}");
        }
    }
    for line in report.doctor.lines() {
        println!("{line}");
    }
    for line in report.worksheet_lines() {
        println!("{line}");
    }
    if report.is_ready() { OK } else { REFUSED }
}
