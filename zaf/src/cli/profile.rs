//! `zaf profile init`: generate the hermetic Claude profiles under `.zaf/profiles/`.

use super::exit::{self, BACKEND, OK};
use super::{Cli, ProfileAction};

use crate::app::profile;
use crate::composition::Composition;

/// Generates the profiles and prints each file written, or `nothing changed`; with
/// `--json` prints the report. Exits 2 when a file cannot be written.
pub fn run(cli: &Cli, action: &ProfileAction) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let ProfileAction::Init = action;
    let report = match profile::init(&composition.state_dir(), composition.manifest()) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    if cli.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("zaf {path}: {error}");
                return BACKEND;
            }
        }
    } else {
        for line in report.lines() {
            println!("{line}");
        }
    }
    OK
}
