//! `zaf status`: the board, running instances, slots, guard and open questions on one screen,
//! with every live zaf instance of the machine read from the process table. With `[phases]`, the
//! open tickets are gathered as the tick gathers them and the screen shows where the core places
//! each ([`Status::with_placement`]); a gather that fails is one warning on stderr, and the screen
//! shows the labels' word instead.

use super::Cli;
use super::exit::{self, BACKEND, OK};
use super::screen::Screen;
use crate::adapters::process;
use crate::app::profile;
use crate::app::release::{self, DueRelease};
use crate::app::schedule::gather::{gather_for, read_lists};
use crate::app::status::{self, Status, StatusError};
use crate::composition::Composition;
use crate::domain::Epic;
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

/// The releases of the project whose objectives or design are due, from its release folders, the
/// board's open `epics` and the released check: what `zaf status` and `zaf epic list` both read.
pub(super) fn due_releases(composition: &Composition, epics: &[Epic]) -> Vec<DueRelease> {
    let (root, manifest) = (composition.root(), composition.manifest());
    let vcs = composition.vcs().ok();
    release::due_releases(root, manifest, epics, &|version| {
        release::is_released(root, manifest, vcs.as_deref(), version)
    })
}

/// The screen for a project over the given board, with the machine's live instances and, with
/// `[phases]`, the core's placement; a gather that fails is one warning on stderr.
fn screen(composition: &Composition, board: &dyn Board) -> Result<Status, StatusError> {
    let spawn = &composition.manifest().spawn;
    // The open epics and the open questions are read once and shared by the screen, the releases
    // due and the placement.
    let (epics, questions) = read_lists(board)?;
    let screen = status::status(
        board,
        &composition.state_dir(),
        spawn.slots,
        &process::alive,
        &epics,
        &questions,
    )?
    .with_machine(&process::displayed())
    .with_budget(spawn.budget())
    .with_project(
        composition.manifest().project.name.as_ref(),
        composition.root(),
    );
    let screen = screen.with_releases(&due_releases(composition, &epics));
    Ok(
        match gather_for(composition, board, None, &epics, &questions) {
            Ok(Some(gathered)) => screen.with_placement(&gathered, std::time::SystemTime::now()),
            Ok(None) => screen,
            Err(error) => {
                crate::eprintln_zaf!("zaf status: warning: the placement was not read: {error}");
                screen
            }
        },
    )
}

/// The status as `terminal` draws it: each piece in its meaning's colour where colour is allowed,
/// else the plain text of [`Status::render`]. The board's words went through `clean` in
/// [`Status::lines`], so no piece of them can carry an escape into the painted output.
fn draw(terminal: &Screen, status: &Status, more: bool) -> String {
    terminal.draw(&status.lines(more))
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
                print!("{}", draw(&Screen::detect(), &screen, cli.more));
            }
            OK
        }
        Err(StatusError::Board(error)) => exit::fail(&path, &error),
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            BACKEND
        }
    }
}

#[cfg(test)]
mod tests;
