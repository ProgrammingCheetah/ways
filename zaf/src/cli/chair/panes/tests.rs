use super::{Cursor, first_shown, step};
use crate::cli::chair::keys::Key;
use crate::cli::chair::support::ways;

#[test]
fn the_window_follows_the_selection() {
    assert_eq!(first_shown(0, 5), 0);
    assert_eq!(first_shown(4, 5), 0);
    assert_eq!(first_shown(5, 5), 1);
    assert_eq!(first_shown(9, 0), 0);
}

#[test]
fn a_step_stays_inside_the_list() {
    assert_eq!(step(Key::Char('j'), 0, 3), Some(1));
    assert_eq!(step(Key::Down, 2, 3), Some(2));
    assert_eq!(step(Key::Up, 0, 3), Some(0));
    assert_eq!(step(Key::Char('k'), 2, 3), Some(1));
    assert_eq!(step(Key::Char('j'), 0, 0), Some(0));
    assert_eq!(step(Key::Enter, 1, 3), None);
}

#[test]
fn a_cursor_is_brought_back_inside_a_smaller_state() {
    let mut cursor = Cursor {
        inbox: 9,
        problem: 9,
        running: 9,
        epic: 1,
        ticket: 9,
        ..Cursor::default()
    };

    cursor.clamp(&ways());

    assert_eq!(
        (
            cursor.inbox,
            cursor.problem,
            cursor.running,
            cursor.epic,
            cursor.ticket
        ),
        (2, 1, 0, 1, 1)
    );
}
