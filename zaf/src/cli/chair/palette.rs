//! The palette's registry (F13): every action of the chair, as `:` lists them, one section per
//! feature. Choosing an entry presses its key, so an entry never holds a handler of its own: the
//! key's handler is the action, wherever it is built.
//!
//! A feature that adds a key adds its entry to its own section here, and nowhere else. Every
//! entry presses a key whose handler is built: Enter in Epics reads the epic, `s` signs it after
//! its undo, `d` draws its graph, `w` says why a ticket waits and `l` follows it in Live (F6, F7);
//! `o` runs the due phase in the phase pane and `Ctrl-w c` leaves it (F8, `model/phase.rs`).

use super::keys::Key;
use super::layout::PaneId;

/// What a palette entry does: a key, a window command after `Ctrl-w`, or a key in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// Press this key.
    Key(Key),
    /// Press `Ctrl-w`, then this key.
    Window(Key),
    /// Focus this pane, then press this key in it.
    In(PaneId, Key),
    /// Show the next project.
    NextTab,
}

/// A palette entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// What it does.
    pub name: &'static str,
    /// Its key, as shown.
    pub key: &'static str,
    /// What choosing it runs.
    pub run: Run,
}

/// One feature's entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// The feature, as the design names it.
    pub feature: &'static str,
    /// Its entries, in the order the palette lists them.
    pub entries: &'static [Entry],
}

const fn entry(name: &'static str, key: &'static str, run: Run) -> Entry {
    Entry { name, key, run }
}

const fn key(character: char) -> Run {
    Run::Key(Key::Char(character))
}

/// Every action of the chair, by feature, in the order `:` lists them: what waits on the Engineer
/// first, the chair's own keys last, quitting at the very end.
pub const SECTIONS: [Section; 6] = [
    Section {
        feature: "F5 answering",
        entries: &[entry("Answer the top question", "a", key('a'))],
    },
    Section {
        feature: "F6 epics",
        entries: &[
            entry(
                "Open the selected epic",
                "Enter",
                Run::In(PaneId::Epics, Key::Enter),
            ),
            entry("Sign the selected epic", "s", key('s')),
            entry("Dependency graph of the epic", "d", key('d')),
            entry("Flip the graph: before/after", "v", key('v')),
        ],
    },
    Section {
        feature: "F7 tickets",
        entries: &[
            entry("Why is the ticket waiting", "w", key('w')),
            entry("Follow the ticket live", "l", key('l')),
        ],
    },
    Section {
        feature: "F8 phases",
        entries: &[
            entry("Start the due phase", "o", key('o')),
            entry("Leave the phase pane", "^w c", Run::Window(Key::Char('c'))),
        ],
    },
    Section {
        feature: "F9 and F15 controls",
        entries: &[
            entry("Start the build", "b", key('b')),
            entry("Stop the build", "B", key('B')),
            entry("Tick now", "t", key('t')),
            entry("Stop the selected task", "x", key('x')),
        ],
    },
    Section {
        feature: "the chair",
        entries: &[
            entry("Read the board again", "r", key('r')),
            entry("Undo the last ruling", "u", key('u')),
            entry("Maximise this pane", "^w o", Run::Window(Key::Char('o'))),
            entry("Equal panes", "^w =", Run::Window(Key::Char('='))),
            entry("Next project", "1-9", Run::NextTab),
            entry("Quit (the build goes on)", "q", key('q')),
        ],
    },
];

/// Every entry, in the order the palette lists them.
pub fn entries() -> impl Iterator<Item = &'static Entry> {
    SECTIONS.iter().flat_map(|section| section.entries)
}

/// How many entries the palette lists.
pub fn len() -> usize {
    entries().count()
}

/// The entry at `index`.
pub fn at(index: usize) -> Option<Entry> {
    entries().nth(index).copied()
}

#[cfg(test)]
mod tests;
