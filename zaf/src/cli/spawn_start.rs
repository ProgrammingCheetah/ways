//! `zaf spawn start --version <v>`: starts the project's scheduler detached and prints its pid
//! (M13, D19, I11).
//!
//! The start is the app layer's ([`crate::app::spawn::scheduler::start`]), the one the chair's `b`
//! runs; this verb only validates the version as the hub's controls do ([`valid_version`]), runs
//! it from the project's root over this machine's processes and says what happened. A new start
//! prints the pid and the log's path; a scheduler already holding the lock is handed back (its pid,
//! said to be running already) and nothing is started. Both exit `0`. An invalid version exits `1`
//! and starts nothing; a start that failed or ended at once, or a project that cannot be read,
//! exits `2`. With `--json` it prints an object with `pid`, `started` and `log`, the log being
//! `null` for a scheduler handed back.

use std::path::Path;

use super::Cli;
use super::exit::{self, BACKEND, OK, REFUSED};
use crate::app::chair::ReleaseVersion;
use crate::app::hub::controls::{Failure, valid_version};
use crate::app::spawn::scheduler::{Processes, Start, start};
use crate::composition::Composition;

/// What the verb printed and the exit code it ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// The line (or the JSON) for stdout, when the verb succeeded.
    pub out: Option<String>,
    /// The line for stderr, when it did not.
    pub err: Option<String>,
    /// The exit code.
    pub code: u8,
}

/// Runs `zaf spawn start --version <version>` from the project at the current directory, over this
/// machine's processes.
pub fn run(cli: &Cli, version: &str) -> u8 {
    let path = cli.verb.path();
    if !valid_version(version) {
        crate::eprintln_zaf!("zaf {path}: {}", not_valid(version));
        return REFUSED;
    }
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let machine = crate::cli::chair::build::machine::Real;
    let said = start_said(composition.root(), version, &machine, cli.json);
    if let Some(out) = &said.out {
        println!("{out}");
    }
    if let Some(err) = &said.err {
        crate::eprintln_zaf!("zaf {path}: {err}");
    }
    said.code
}

fn not_valid(version: &str) -> String {
    format!(
        "{version:?} is not a version: use letters, digits, dots, hyphens, underscores and plus \
         signs, never starting with a hyphen"
    )
}

/// The start of the scheduler of the project at `root` for `version` over `processes`, as the
/// verb says it.
pub fn start_said(root: &Path, version: &str, processes: &dyn Processes, json: bool) -> Said {
    if !valid_version(version) {
        return Said {
            out: None,
            err: Some(not_valid(version)),
            code: REFUSED,
        };
    }
    let release = ReleaseVersion::from(version);
    match start(root, &release, processes) {
        Ok(start) => Said {
            out: Some(line(root, &start, json)),
            err: None,
            code: OK,
        },
        Err(Failure::Refused(why)) => Said {
            out: None,
            err: Some(why),
            code: REFUSED,
        },
        Err(Failure::Failed(why)) => Said {
            out: None,
            err: Some(why),
            code: BACKEND,
        },
    }
}

/// `path` as it reads from `root`: relative when it is inside.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn line(root: &Path, start: &Start, json: bool) -> String {
    match start {
        Start::Started { pid, log } => {
            if json {
                serde_json::json!({
                    "pid": pid,
                    "started": true,
                    "log": shown(root, log),
                })
                .to_string()
            } else {
                format!("started scheduler pid {pid}; log {}", shown(root, log))
            }
        }
        Start::Running(holding) => {
            if json {
                serde_json::json!({
                    "pid": holding.pid,
                    "started": false,
                    "log": serde_json::Value::Null,
                })
                .to_string()
            } else {
                format!(
                    "scheduler already running: pid {}, since {} (zaf {}, --version {})",
                    holding.pid, holding.since, holding.zaf, holding.version
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
