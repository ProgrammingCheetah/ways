//! A terminal's key events as the chair reads them: a few named keys and the characters typed.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// A key the chair acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character typed, shifted ones (`+`, `<`, `:`) included.
    Char(char),
    /// Enter.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// `Ctrl-w`: the window prefix.
    CtrlW,
    /// `Ctrl-u`: clears the answer box.
    CtrlU,
    /// `Ctrl-c`: quits as `q` does.
    CtrlC,
}

/// The key `event` is, or none for a release, a repeat of nothing named, or a key the chair has
/// no use for.
pub fn key_of(event: &KeyEvent) -> Option<Key> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let control = event.modifiers.contains(KeyModifiers::CONTROL);
    Some(match event.code {
        KeyCode::Char(c) if control => match c.to_ascii_lowercase() {
            'w' => Key::CtrlW,
            'u' => Key::CtrlU,
            'c' => Key::CtrlC,
            _ => return None,
        },
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
