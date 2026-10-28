//! `zaf status`: the board, running instances, slots, guard and open questions on one screen.

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::app::status::{self, StatusError};
use crate::composition::Composition;
use crate::ports::Board;

/// Prints the screen.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    print_status(cli, &composition, board.as_ref())
}

/// Reads and prints the screen for a project over the given board.
fn print_status(cli: &Cli, composition: &Composition, board: &dyn Board) -> u8 {
    let path = cli.verb.path();
    let slots = composition.manifest().spawn.slots;
    match status::status(board, &composition.state_dir(), slots) {
        Ok(screen) => {
            if cli.json {
                println!("{}", screen.to_json());
            } else {
                print!("{}", screen.render_text());
            }
            OK
        }
        Err(StatusError::Board(error)) => exit::fail(&path, &error),
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}

#[cfg(test)]
mod tests;
