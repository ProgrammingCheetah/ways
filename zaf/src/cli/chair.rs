//! The captain's chair (release 0.5.3): what `zaf` alone opens on a terminal. A full-screen view
//! of the machine's projects, a tab each, with what waits on the Engineer, what is wrong, what
//! runs and the epics' squares, where a question is answered without leaving it.
//!
//! This file decides when the chair opens (D1) and hands over to the adapter under `chair/`, the
//! only place that names a terminal crate (`tests/architecture.rs` checks it):
//!
//! - `model`: the chair as a state machine, pure, keys in and commands out.
//! - `layout`: the split tree, pure. `panes`: each pane behind one trait, stubs for F6 and F7.
//! - `draw`: the frame, from the model. `keys`, `theme`, `text`: keys, styles and widths.
//! - `source`: the tabs, a gatherer thread per project and the writes; `run`: the loop;
//!   `terminal`: raw mode and the alternate screen, put back on close and on panic.
//! - `build`: the build controls (F9, F15), asked about on the status line, run behind a port;
//!   `pings`: the bell and the desktop notifications (F10); `palette`: every action, by feature
//!   (F13).
//! - `session`: the phase pane's pseudo-terminal and threads, and every call into begin, the
//!   release folder and the runner (F8).
//!
//! The read model and what writes are in `app::chair`.

mod build;
mod draw;
mod keys;
mod layout;
mod model;
mod palette;
mod panes;
mod pings;
mod run;
mod session;
mod source;
#[cfg(test)]
mod support;
mod terminal;
mod text;
mod theme;

use std::path::{Path, PathBuf};

use super::screen::Screen;
use crate::domain::screen::Meaning;

/// What bare `zaf` does (D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bare {
    /// No terminal: as before, clap prints the usage on stderr and exits 2.
    Usage,
    /// The chair opens, on this project's tab, or on the machine's projects for none.
    Open(Option<PathBuf>),
    /// No project here and none on the machine: three lines, exit 0.
    Nowhere,
}

/// What bare `zaf` does on a `terminal` (stdin and stdout both, outside an instance), in
/// `project` when one holds the folder, with `projects` on the machine.
pub fn bare(terminal: bool, project: Option<PathBuf>, projects: usize) -> Bare {
    match (terminal, project) {
        (false, _) => Bare::Usage,
        (true, Some(project)) => Bare::Open(Some(project)),
        (true, None) if projects > 0 => Bare::Open(None),
        (true, None) => Bare::Nowhere,
    }
}

/// The three lines bare `zaf` prints where there is no project at all.
pub fn nowhere(screen: &Screen) -> String {
    let zaf = screen.paint(Meaning::Program, "zaf");
    format!(
        "This folder has no zaf project.\n{zaf} {} makes one.\n{zaf} {} lists the verbs.\n",
        screen.paint(Meaning::Verb, "setup"),
        screen.paint(Meaning::Verb, "help"),
    )
}

/// Opens the chair on the machine's projects, `project` among them and on screen, writing as the
/// zaf home `home`; returns once it is closed. What a child process or a library would print on
/// stderr meanwhile goes to the home's `logs/chair.log`: the chair is the machine's, not one
/// project's, and reading a project never writes into it.
pub fn open(project: Option<&Path>, home: Option<&Path>) -> u8 {
    let (entries, active) = source::entries(project, home);
    if entries.is_empty() {
        print!("{}", nowhere(&Screen::detect()));
        return 0;
    }
    let log = home.map_or_else(
        || PathBuf::from("/dev/null"),
        |home| home.join("logs").join("chair.log"),
    );
    run::run(entries, active, home.map(Path::to_owned), &log)
}

#[cfg(test)]
mod tests;
