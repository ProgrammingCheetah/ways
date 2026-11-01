//! `zaf init`: write a minimal manifest by asking on stdin.

use std::io;

use super::Cli;
use super::exit::{BACKEND, NOT_IMPLEMENTED, OK, REFUSED};

use crate::app::init::{InitError, init};

/// Asks for the project's name and repos, writes `ZAF.toml` in the current directory and
/// exits 0; exits 1 when one exists or an answer is unusable. `--from-teams` is not built.
pub fn run(cli: &Cli, from_teams: bool) -> u8 {
    let path = cli.verb.path();
    if from_teams {
        eprintln!("zaf {path} --from-teams: not built");
        return NOT_IMPLEMENTED;
    }
    let dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    match init(&dir, &mut io::stdin().lock(), &mut io::stdout()) {
        Ok(written) => {
            println!("wrote {}", written.display());
            OK
        }
        Err(InitError::Io(error)) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
    }
}

#[cfg(test)]
mod tests;
