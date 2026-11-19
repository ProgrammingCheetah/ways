//! `zaf epic list|show|sign|file`: the open epics and where each stands, the Engineer's signature,
//! and filing an epic of a version.
//!
//! `list` prints one line per open epic, `#<n> <stage> <done> of <total>  <title>`; `show`
//! prints that line for one epic and then one indented line per task,
//! `#<t> <state> [<group>]  <title>`, and with `--more` a blank line and the epic's body. `sign`
//! prints `signed #<n> sha256:<hex>`, or `#<n> is already signed` when the signature holds the
//! body; it exits 1 inside an instance or for a ticket that is no open epic. A new signature asks
//! the running scheduler for a tick; a request that cannot be written says so on stderr and
//! changes neither the signature nor the exit. When that signature is the last one of its
//! version, at a terminal, outside an instance and without `--json`, it asks `Every epic of <v> is
//! signed. Start the build now? [Y/n]`: yes becomes `zaf spawn --watch --version <v>` in this
//! process's place (exit 2 if it cannot start, the epic still signed), no or closed input exits
//! 0 as signing does. `file --version <v> --title <t>` reads the body on
//! stdin and prints the new epic's id (the open epic's of that version and title, when there is
//! one); an empty title or body exits 1, as does a board that cannot file an epic.

use std::time::SystemTime;

use serde_json::json;

use super::do_action::this_terminal;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::screen::{DefaultAnswer, Screen, ask_yes};
use super::setup::as_instance;
use super::{Cli, EpicAction};
use crate::adapters::process;
use crate::app::actions::Terminal;
use crate::app::epic::{self, EpicError, FileOutcome, SignGuard, SignOutcome};
use crate::app::schedule::{binary, request};
use crate::composition::Composition;
use crate::domain::Signature;

/// Whether a new signature may ask to start the build: the Engineer's own run (no instance),
/// at a terminal, with text output.
fn offers_build(json: bool, instance: bool, terminal: Terminal) -> bool {
    !json && !instance && terminal == Terminal::Present
}

/// Q5's question for `version`, as `zaf epic sign` and `zaf begin` ask it.
pub(super) fn build_question(version: &str) -> String {
    format!("Every epic of {version} is signed. Start the build now?")
}

/// The command `zaf spawn --watch --version <version>` run by `exe`, the binary this process is,
/// never the first `zaf` on PATH.
fn spawn_watch_command(exe: std::path::PathBuf, version: &str) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(["spawn", "--watch", "--version", version]);
    command
}

/// Becomes `zaf spawn --watch --version <version>` in this process's place, run by this very
/// binary (as `zaf update` finds it); it comes back only with the error of an exec that failed.
pub(super) fn exec_spawn_watch(version: &str) -> std::io::Error {
    let exe = match std::env::current_exe().map(binary::without_deleted) {
        Ok(exe) => exe,
        Err(error) => return error,
    };
    std::os::unix::process::CommandExt::exec(&mut spawn_watch_command(exe, version))
}

/// Asks Q5 on the terminal and, on a yes, hands it to `zaf spawn --watch --version <version>` in
/// place: this process becomes the scheduler. A no, or closed input, ends as signing does (0); an
/// exec that fails says so on stderr and exits 2, the epic still signed.
fn start_build(version: &str) -> u8 {
    let yes = ask_yes(
        &Screen::detect(),
        &build_question(version),
        DefaultAnswer::Yes,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    );
    if !yes {
        return OK;
    }
    let error = exec_spawn_watch(version);
    eprintln!("zaf epic sign: cannot start `zaf spawn --watch --version {version}`: {error}");
    BACKEND
}

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
    let mut build: Option<String> = None;
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
                    // The last epic of its version signed: the build is what the Engineer
                    // wants next, asked only where someone can answer. A board that cannot say
                    // changes nothing: the epic stays signed.
                    if offers_build(cli.json, as_instance(), this_terminal()) {
                        let signature = Signature {
                            hash: hash.clone(),
                            at: SystemTime::now(),
                        };
                        build = epic::build_ready(board.as_ref(), *epic, signature)
                            .ok()
                            .flatten();
                    }
                }
            })
        }
    };
    match done {
        Ok(()) => match build {
            Some(version) => start_build(&version),
            None => OK,
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_start_runs_the_current_executable() {
        let exe = binary::without_deleted(std::env::current_exe().unwrap());
        let command = spawn_watch_command(exe.clone(), "0.5.2");
        assert_eq!(command.get_program(), exe.as_os_str());
        assert_ne!(command.get_program(), "zaf");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["spawn", "--watch", "--version", "0.5.2"]);
        let deleted = std::path::PathBuf::from("/opt/zaf (deleted)");
        let command = spawn_watch_command(binary::without_deleted(deleted), "0.5.2");
        assert_eq!(command.get_program(), "/opt/zaf");
    }
}
