//! `zaf report`: tokens per model, the subagent growth profile and the per-run table.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::app::guard;
use crate::app::report::ledger::{self, LEDGER_FILE, Ledger, LedgerRow};
use crate::app::report::{self, PriceTable, Query, epics};
use crate::composition::{Composition, zaf_home};
use crate::ports::Board;

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
    /// Print one row per epic from the ledger and the board instead: what it cost beside
    /// what it finished. The ledger outlives transcripts, so `--hours`, `--match` and
    /// `--top` do not apply; `--all` adds the runs of every project.
    #[arg(long)]
    pub epics: bool,
}

/// The default Claude config dir: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub(super) fn default_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
}

/// Ingests the project's profiles and the default Claude config dir into the ledger in the
/// zaf home; a failure's message names the file.
pub(super) fn ingest_ledger(composition: &Composition) -> Result<usize, String> {
    let home = zaf_home().ok_or_else(|| {
        format!("no home for the ledger ~/.zaf/{LEDGER_FILE}: set ZAF_HOME or HOME")
    })?;
    let mut ledger = Ledger::open(&home.join(LEDGER_FILE)).map_err(|error| error.to_string())?;
    let sources = ledger::sources(
        &composition.state_dir(),
        composition.manifest().project.name.as_ref(),
        default_config_dir().as_deref(),
    );
    ledger.ingest(&sources).map_err(|error| error.to_string())
}

/// Fills the ledger, then prints the report.
pub fn run(cli: &Cli, args: &ReportArgs) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    if let Err(message) = ingest_ledger(&composition) {
        eprintln!("zaf {path}: {message}");
        return BACKEND;
    }
    if args.epics {
        return run_epics(cli, &composition, args);
    }
    let config_dir = if args.all { default_config_dir() } else { None };
    print_report(cli, &composition, args, config_dir.as_deref())
}

/// Reads the ledger and the board and prints the per-epic rows.
fn run_epics(cli: &Cli, composition: &Composition, args: &ReportArgs) -> u8 {
    let path = cli.verb.path();
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let rows = match ledger_rows(composition, args.all) {
        Ok(rows) => rows,
        Err(message) => {
            eprintln!("zaf {path}: {message}");
            return BACKEND;
        }
    };
    let prices = match read_prices(composition) {
        Ok(prices) => prices,
        Err(message) => {
            eprintln!("zaf {path}: {message}");
            return BACKEND;
        }
    };
    print_epics(cli, &rows, board.as_ref(), &prices)
}

/// The ledger's rows for the project, or for every project with `all`.
fn ledger_rows(composition: &Composition, all: bool) -> Result<Vec<LedgerRow>, String> {
    let home = zaf_home().ok_or_else(|| {
        format!("no home for the ledger ~/.zaf/{LEDGER_FILE}: set ZAF_HOME or HOME")
    })?;
    let ledger = Ledger::open(&home.join(LEDGER_FILE)).map_err(|error| error.to_string())?;
    let rows = ledger.rows().map_err(|error| error.to_string())?;
    Ok(project_rows(
        rows,
        all,
        composition.manifest().project.name.as_ref(),
        composition.root(),
    ))
}

/// The rows that count for the project named `name` at `root`, or every row with `all`.
fn project_rows(rows: Vec<LedgerRow>, all: bool, name: &str, root: &Path) -> Vec<LedgerRow> {
    rows.into_iter()
        .filter(|row| all || ledger::is_this_project(&row.project, name, root))
        .collect()
}

/// Prints the per-epic rows; a board that fails exits `2`.
fn print_epics(cli: &Cli, rows: &[LedgerRow], board: &dyn Board, prices: &PriceTable) -> u8 {
    let view = match epics::epic_view(rows, board, prices) {
        Ok(view) => view,
        Err(error) => {
            eprintln!("zaf {}: the board failed: {error}", cli.verb.path());
            return BACKEND;
        }
    };
    if cli.json {
        println!("{}", view.to_json());
    } else {
        print!("{}", view.render_text());
    }
    OK
}

/// The price table: the defaults with `[report.prices]` of `local.toml` over them.
fn read_prices(composition: &Composition) -> Result<PriceTable, String> {
    guard::read_local(&composition.state_dir())
        .map_err(|error| error.to_string())
        .and_then(|local| PriceTable::from_local_toml(&local).map_err(|error| error.to_string()))
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
    let prices = match read_prices(composition) {
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
