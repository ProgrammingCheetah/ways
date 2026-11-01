//! `zaf guard`: computes the budget level and writes `.zaf/guard.json` for the spawner.

use std::time::SystemTime;

use super::Cli;
use super::exit::{self, BACKEND, OK};
use super::report::default_config_dir;
use crate::app::guard::{self, GuardState};
use crate::app::report::{self, PriceTable, Query};
use crate::composition::Composition;

/// The week the budget covers, in hours.
const WEEK_HOURS: f64 = 168.0;

/// Computes, stores and prints the guard.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    compute(cli, &composition, default_config_dir().as_deref())
}

/// The week's spend is everything the project's profiles and the default config dir
/// spent: the weekly limit is the account's, not one profile's.
fn compute(
    cli: &Cli,
    composition: &Composition,
    default_config_dir: Option<&std::path::Path>,
) -> u8 {
    let path = cli.verb.path();
    let state_dir = composition.state_dir();
    let query = Query {
        roots: report::transcript_roots(&state_dir, default_config_dir),
        hours: WEEK_HOURS,
        matching: String::new(),
        top: 0,
        prices: PriceTable::default(),
        now: SystemTime::now(),
    };
    match guard::run(&state_dir, query, &composition.manifest().spawn.guard) {
        Ok(state) => {
            print_state(cli, &state);
            OK
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}

fn print_state(cli: &Cli, state: &GuardState) {
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".to_owned())
        );
    } else {
        println!("{}", state.line());
    }
}

#[cfg(test)]
mod tests;
