//! `zaf update`: replace this zaf with the newest release, or with `--check` say whether one is out.
//!
//! The newest release is read from GitHub each time; nothing is stored. Up to date prints
//! nothing and exits 0. An update prints one line, `zaf <old> → <new> (<path>)`. A check that
//! finds a newer release prints it and exits 1. A failure leaves the old binary in place.

use semver::Version;

use super::Cli;
use super::exit::{BACKEND, OK, REFUSED};
use crate::adapters::github::{GhCommand, GithubReleases};
use crate::app::update::{self, HOME_REPO, Outcome, UpdateError};
use crate::manifest::RepoName;
use crate::ports::Releases;

/// Runs the verb.
pub fn run(cli: &Cli, check: bool) -> u8 {
    let releases = GithubReleases::from(GhCommand::from(RepoName::from(HOME_REPO)));
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("zaf update: cannot find this binary: {error}");
            return BACKEND;
        }
    };
    run_with(cli, check, &releases, &exe)
}

/// The verb over the given releases and binary path.
pub fn run_with(cli: &Cli, check: bool, releases: &dyn Releases, exe: &std::path::Path) -> u8 {
    let current = running_version();
    let outcome = if check {
        update::check(releases, &current)
    } else {
        update::update(releases, &current, exe, update::target())
    };
    match outcome {
        Ok(outcome) => report(cli, &outcome),
        Err(error) => {
            eprintln!("zaf update: {error}");
            match error {
                UpdateError::NoRelease => REFUSED,
                UpdateError::Port(_)
                | UpdateError::NoArchive { .. }
                | UpdateError::Install { .. } => BACKEND,
            }
        }
    }
}

/// This binary's version.
pub fn running_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

fn report(cli: &Cli, outcome: &Outcome) -> u8 {
    if cli.json {
        let json = match outcome {
            Outcome::Current { version } => serde_json::json!({
                "current": version.to_string(), "newest": version.to_string(), "updated": false,
            }),
            Outcome::Behind { current, newest } => serde_json::json!({
                "current": current.to_string(), "newest": newest.to_string(), "updated": false,
            }),
            Outcome::Updated { from, to, path } => serde_json::json!({
                "current": to.to_string(), "from": from.to_string(), "newest": to.to_string(),
                "updated": true, "path": path.display().to_string(),
            }),
        };
        println!("{json}");
    }
    match outcome {
        Outcome::Current { .. } => OK,
        Outcome::Behind { current, newest } => {
            if !cli.json {
                println!("zaf {current}, newest {newest}: run zaf update");
            }
            REFUSED
        }
        Outcome::Updated { from, to, path } => {
            if !cli.json {
                println!("zaf {from} → {to} ({})", path.display());
            }
            OK
        }
    }
}
