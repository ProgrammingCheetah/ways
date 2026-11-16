//! `zaf status`: the board, running instances, slots, guard and open questions on one screen,
//! with every live zaf instance of the machine read from the process table. With `[phases]`, the
//! open tickets are gathered as the tick gathers them and the screen shows where the core places
//! each ([`Status::with_placement`]); a gather that fails is one warning on stderr, and the screen
//! shows the labels' word instead.

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::adapters::process;
use crate::app::profile;
use crate::app::schedule::gather::gather_for;
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

/// The screen for a project over the given board, with the machine's live instances and, with
/// `[phases]`, the core's placement; a gather that fails is one warning on stderr.
fn screen(composition: &Composition, board: &dyn Board) -> Result<Status, StatusError> {
    let spawn = &composition.manifest().spawn;
    let screen = status::status(
        board,
        &composition.state_dir(),
        spawn.slots,
        &process::alive,
    )?
    .with_machine(&process::displayed())
    .with_budget(spawn.budget());
    Ok(match gather_for(composition, board) {
        Ok(Some(gathered)) => screen.with_placement(&gathered, std::time::SystemTime::now()),
        Ok(None) => screen,
        Err(error) => {
            eprintln!("zaf status: warning: the placement was not read: {error}");
            screen
        }
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
                print!("{}", screen.render(cli.more));
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
