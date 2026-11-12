//! `zaf status`: the board, running instances, slots, guard and open questions on one screen,
//! with every live zaf instance of the machine read from the process table.

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::adapters::process;
use crate::app::profile;
use crate::app::status::{self, Status, StatusError};
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

/// The screen for a project over the given board, with the machine's live instances.
fn screen(composition: &Composition, board: &dyn Board) -> Result<Status, StatusError> {
    let spawn = &composition.manifest().spawn;
    status::status(
        board,
        &composition.state_dir(),
        spawn.slots,
        &process::alive,
    )
    .map(|screen| {
        screen
            .with_machine(&process::machine())
            .with_budget(spawn.budget())
    })
}

/// Reads and prints the screen for a project over the given board.
fn print_status(cli: &Cli, composition: &Composition, board: &dyn Board) -> u8 {
    let path = cli.verb.path();
    let state_dir = composition.state_dir();
    match screen(composition, board) {
        Ok(screen) => {
            let screen =
                screen.with_stale(profile::stale_homes(&state_dir, composition.manifest()));
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
