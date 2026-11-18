//! `zaf epic list|show|sign|file`: the open epics and where each stands, the Engineer's signature,
//! and filing an epic of a version.
//!
//! `list` prints one line per open epic, `#<n> <stage> <done> of <total>  <title>`; `show`
//! prints that line for one epic and then one indented line per task,
//! `#<t> <state> [<group>]  <title>`, and with `--more` a blank line and the epic's body. `sign`
//! prints `signed #<n> sha256:<hex>`, or `#<n> is already signed` when the signature holds the
//! body; it exits 1 inside an instance or for a ticket that is no open epic. A new signature asks
//! the running scheduler for a tick; a request that cannot be written says so on stderr and
//! changes neither the signature nor the exit. `file --version <v> --title <t>` reads the body on
//! stdin and prints the new epic's id (the open epic's of that version and title, when there is
//! one); an empty title or body exits 1, as does a board that cannot file an epic.

use std::time::SystemTime;

use serde_json::json;

use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, EpicAction};
use crate::adapters::process;
use crate::app::epic::{self, EpicError, FileOutcome, SignGuard, SignOutcome};
use crate::app::schedule::request;
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
        EpicAction::Show { epic } => epic::show(board.as_ref(), *epic).map(|view| {
            if cli.json {
                println!("{}", json!(view));
            } else if cli.more {
                println!("{}", view.render_more());
            } else {
                println!("{}", view.render());
            }
        }),
        EpicAction::File { version, title } => {
            let body = match std::io::read_to_string(std::io::stdin()) {
                Ok(body) => body,
                Err(error) => {
                    eprintln!("zaf {path}: reading the body on stdin: {error}");
                    return BACKEND;
                }
            };
            epic::file(board.as_ref(), version, title, &body).map(|outcome| {
                let number = outcome.epic();
                if cli.json {
                    let word = match outcome {
                        FileOutcome::Filed(_) => "filed",
                        FileOutcome::AlreadyFiled(_) => "already_filed",
                    };
                    println!(
                        "{}",
                        json!({ "epic": number.get(), "version": version, "outcome": word })
                    );
                } else {
                    println!("{number}");
                }
            })
        }
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
                // A signed epic is planned at the next tick: the scheduler that runs is asked for
                // it now. With none, nothing is asked. A request that cannot be written is one
                // line on stderr; the epic stays signed and the exit is the signature's.
                if signed {
                    let who = format!("sign {epic}");
                    if let Err(not_asked) =
                        request::ask(&state_dir, &who, SystemTime::now(), &process::alive)
                    {
                        eprintln!("zaf {path}: {}", not_asked.left_to_the_timer(&who));
                    }
                }
            })
        }
    };
    match done {
        Ok(()) => OK,
        Err(EpicError::Board(error)) => exit::fail(&path, &error),
        Err(
            error @ (EpicError::NotAnEpic(_)
            | EpicError::InsideInstance { .. }
            | EpicError::EmptyTitle
            | EpicError::EmptyBody
            | EpicError::BadVersion(_)),
        ) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
        Err(error @ EpicError::Runs(_)) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}
