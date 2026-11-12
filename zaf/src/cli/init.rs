//! `zaf init`: write a minimal manifest by asking on stdin.

use std::fs;
use std::io;
use std::time::Duration;

use super::Cli;
use super::exit::{BACKEND, NOT_IMPLEMENTED, OK, REFUSED};

use crate::adapters::shell::SystemShell;
use crate::app::init::{InitError, init_with_home};
use crate::app::profile::{ProfileError, current_pack_tag};
use crate::composition::zaf_home;

/// The marketplace's current pack tag, read from a scratch profile that is removed after.
fn learn_pack_tag() -> Result<String, ProfileError> {
    let scratch = std::env::temp_dir().join(format!("zaf-pack-tag-{}", std::process::id()));
    let shell = SystemShell::new(&std::env::temp_dir(), Duration::from_secs(120));
    let tag = fs::create_dir_all(&scratch)
        .map_err(|error| ProfileError::Install {
            line: format!("mkdir {}", scratch.display()),
            output: error.to_string(),
        })
        .and_then(|()| current_pack_tag(&shell, &scratch));
    let _ = fs::remove_dir_all(&scratch);
    tag
}

/// Asks for the project's name and repos, writes `.zaf.toml` in the current directory and
/// prints `wrote .zaf.toml`; exits 1 when any manifest file exists or an answer is unusable.
/// `--from-teams` is not built.
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
    let home = zaf_home();
    match init_with_home(
        &dir,
        home.as_deref(),
        &mut learn_pack_tag,
        &mut io::stdin().lock(),
        &mut io::stdout(),
    ) {
        Ok(written) => {
            let shown = written.strip_prefix(&dir).unwrap_or(&written);
            println!("wrote {}", shown.display());
            OK
        }
        Err(InitError::Pack(error)) => {
            eprintln!("zaf {path}: the pack's current tag: {error}");
            BACKEND
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
