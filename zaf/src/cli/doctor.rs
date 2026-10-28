//! `zaf doctor`: validate the manifest and verify the required tools.

use std::io;
use std::process::Command;

use super::Cli;
use super::exit::{self, OK, REFUSED};

use crate::app::doctor::{CommandOutput, Commands, doctor};
use crate::composition::Composition;

/// Runs real processes; the one [`Commands`] the command line uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCommands;

impl Commands for SystemCommands {
    /// Output is stdout then stderr, because some tools print their version on stderr.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        let output = Command::new(program).args(args).output()?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(CommandOutput {
            text,
            success: output.status.success(),
        })
    }
}

/// Loads the manifest (an error is printed and exits 1), checks the tools and folders,
/// prints one line per check, or the table with `--json`; exits 1 when anything fails.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let report = doctor(composition.root(), composition.manifest(), &SystemCommands);
    if cli.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("zaf {path}: {error}");
                return exit::BACKEND;
            }
        }
    } else {
        for line in report.lines() {
            println!("{line}");
        }
    }
    if report.passed() { OK } else { REFUSED }
}
