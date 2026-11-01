//! `zaf report`: tokens per model, the subagent growth profile and the per-run table.

use std::path::PathBuf;
use std::time::SystemTime;

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::app::guard;
use crate::app::report::{self, PriceTable, Query};
use crate::composition::Composition;

/// The flags of `zaf report`.
#[derive(Debug, clap::Args)]
pub struct ReportArgs {
    /// Read the default Claude config dir too (`$CLAUDE_CONFIG_DIR` or `~/.claude`),
    /// not only the project's profiles.
    #[arg(long)]
    pub all: bool,
    /// Only transcripts modified in the last this many hours.
    #[arg(long, default_value_t = 168.0)]
    pub hours: f64,
    /// Only runs whose path or first prompt contains this text.
    #[arg(long = "match")]
    pub matching: Option<String>,
    /// How many rows of the per-run table.
    #[arg(long, default_value_t = 25)]
    pub top: usize,
}

/// The default Claude config dir: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub(super) fn default_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
}

/// Prints the report.
pub fn run(cli: &Cli, args: &ReportArgs) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let config_dir = if args.all { default_config_dir() } else { None };
    print_report(cli, &composition, args, config_dir.as_deref())
}

/// Reads and prints the report for a project, with the default config dir given or not.
fn print_report(
    cli: &Cli,
    composition: &Composition,
    args: &ReportArgs,
    default_config_dir: Option<&std::path::Path>,
) -> u8 {
    let path = cli.verb.path();
    let state_dir = composition.state_dir();
    let prices = guard::read_local(&state_dir)
        .map_err(|error| error.to_string())
        .and_then(|local| PriceTable::from_local_toml(&local).map_err(|error| error.to_string()));
    let prices = match prices {
        Ok(prices) => prices,
        Err(message) => {
            eprintln!("zaf {path}: {message}");
            return BACKEND;
        }
    };
    let query = Query {
        roots: report::transcript_roots(&state_dir, default_config_dir),
        hours: args.hours,
        matching: args.matching.clone().unwrap_or_default(),
        top: args.top,
        prices,
        now: SystemTime::now(),
    };
    let report = report::report(&query);
    if cli.json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.render_text());
    }
    OK
}

#[cfg(test)]
mod tests;
