//! `zaf epic list|sign`: the open epics and where each stands, and the Engineer's signature.
//!
//! `list` prints one line per open epic, `#<n> <stage> <done> of <total>  <title>`. `sign`
//! prints `signed #<n> sha256:<hex>`, or `#<n> is already signed` when the signature holds the
//! body; it exits 1 inside an instance or for a ticket that is no open epic.

use std::time::SystemTime;

use serde_json::json;

use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, EpicAction};
use crate::adapters::process;
use crate::app::epic::{self, EpicError, SignGuard, SignOutcome};
use crate::composition::Composition;

/// Runs the verb.
pub fn run(cli: &Cli, action: &EpicAction) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let done = match action {
        EpicAction::List => epic::list(board.as_ref()).map(|lines| {
            if cli.json {
                println!("{}", json!(lines));
            } else {
                for line in &lines {
                    println!("{}", line.render());
                }
            }
        }),
        EpicAction::Sign { epic } => {
            let ancestors = process::ancestors();
            let state_dir = composition.state_dir();
            let guard = SignGuard {
                state_dir: &state_dir,
                ancestors: &ancestors,
                alive: &process::alive,
            };
            epic::sign(board.as_ref(), *epic, &guard, SystemTime::now()).map(|outcome| {
                let (signed, hash, terse) = match &outcome {
                    SignOutcome::Signed(hash) => {
                        (true, hash, format!("signed {epic} sha256:{hash}"))
                    }
                    SignOutcome::AlreadySigned(hash) => {
                        (false, hash, format!("{epic} is already signed"))
                    }
                };
                if cli.json {
                    println!(
                        "{}",
                        json!({ "epic": epic, "signed": signed, "sha256": hash.as_str() })
                    );
                } else {
                    println!("{terse}");
                }
            })
        }
    };
    match done {
        Ok(()) => OK,
        Err(EpicError::Board(error)) => exit::fail(&path, &error),
        Err(error @ (EpicError::NotAnEpic(_) | EpicError::InsideInstance { .. })) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
        Err(error @ EpicError::Runs(_)) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}
