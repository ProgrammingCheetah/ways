use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Key, key_of};

fn press(code: KeyCode, modifiers: KeyModifiers) -> Option<Key> {
    key_of(&KeyEvent::new(code, modifiers))
}

#[test]
fn control_keys_are_named_and_the_rest_typed() {
    assert_eq!(
        press(KeyCode::Char('w'), KeyModifiers::CONTROL),
        Some(Key::CtrlW)
    );
    assert_eq!(
        press(KeyCode::Char('U'), KeyModifiers::CONTROL),
        Some(Key::CtrlU)
    );
    assert_eq!(
        press(KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Key::CtrlC)
    );
    assert_eq!(press(KeyCode::Char('x'), KeyModifiers::CONTROL), None);
    assert_eq!(
        press(KeyCode::Char('+'), KeyModifiers::SHIFT),
        Some(Key::Char('+'))
    );
    assert_eq!(
        press(KeyCode::Char('q'), KeyModifiers::NONE),
        Some(Key::Char('q'))
    );
    assert_eq!(press(KeyCode::Enter, KeyModifiers::NONE), Some(Key::Enter));
    assert_eq!(press(KeyCode::Left, KeyModifiers::NONE), Some(Key::Left));
    assert_eq!(press(KeyCode::F(5), KeyModifiers::NONE), None);
}

#[test]
fn a_release_is_no_key() {
    let mut event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    event.kind = KeyEventKind::Release;

    assert_eq!(key_of(&event), None);
}
