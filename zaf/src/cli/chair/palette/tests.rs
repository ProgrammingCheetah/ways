use std::collections::BTreeSet;

use super::{Run, at, entries, len};
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::text::width;

#[test]
fn every_action_of_the_chair_is_listed_once() {
    let names: Vec<&str> = entries().map(|entry| entry.name).collect();
    for wanted in [
        "Answer the top question",
        "Open the selected epic",
        "Sign the selected epic",
        "Dependency graph of the epic",
        "Why is the ticket waiting",
        "Follow the ticket live",
        "Start the due phase",
        "Leave the phase pane",
        "Start the build",
        "Stop the build",
        "Tick now",
        "Stop the selected task",
        "Read the board again",
        "Undo the last ruling",
        "Quit (the build goes on)",
    ] {
        assert!(
            names.contains(&wanted),
            "{wanted} is missing from {names:?}"
        );
    }
    let keys: BTreeSet<&str> = entries().map(|entry| entry.key).collect();
    assert_eq!(keys.len(), len(), "no two entries share a key");
}

#[test]
fn the_top_question_comes_first_and_quitting_last() {
    assert_eq!(at(0).map(|entry| entry.run), Some(Run::Key(Key::Char('a'))));
    assert_eq!(
        at(len() - 1).map(|entry| entry.run),
        Some(Run::Key(Key::Char('q')))
    );
    assert_eq!(at(len()), None);
}

#[test]
fn an_epic_opens_in_its_pane_and_the_phase_pane_is_left_by_a_window_key() {
    let run = |name: &str| {
        entries()
            .find(|entry| entry.name == name)
            .map(|entry| entry.run)
    };
    assert_eq!(
        run("Open the selected epic"),
        Some(Run::In(PaneId::Epics, Key::Enter))
    );
    assert_eq!(
        run("Leave the phase pane"),
        Some(Run::Window(Key::Char('c')))
    );
}

#[test]
fn every_entry_fits_the_palette_box() {
    for entry in entries() {
        assert!(width(entry.name) <= 28, "{} is too long", entry.name);
        assert!(width(entry.key) <= 7, "{} is too long", entry.key);
    }
}
