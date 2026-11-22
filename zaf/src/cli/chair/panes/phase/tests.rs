use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Ask, Ending, PhaseCommand, Routed, bytes_of, first_due, route, version_of};
use crate::app::chair::ReleaseVersion;
use crate::cli::chair::keys::Key;
use crate::cli::chair::support::ways;

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn bytes(routed: &Routed) -> &[u8] {
    match routed {
        Routed::Bytes(bytes) => bytes,
        other => panic!("not bytes: {other:?}"),
    }
}

#[test]
fn ctrl_w_alone_stays_with_the_chair() {
    for c in ['w', 'W'] {
        assert_eq!(
            route(&press(KeyCode::Char(c), KeyModifiers::CONTROL), false),
            Routed::Window
        );
    }
    // Every other key goes to Claude, the chair's own keys and Ctrl-C included.
    for (code, modifiers, sent) in [
        (KeyCode::Char('q'), KeyModifiers::NONE, &b"q"[..]),
        (KeyCode::Char(':'), KeyModifiers::NONE, b":"),
        (KeyCode::Char('o'), KeyModifiers::NONE, b"o"),
        (KeyCode::Char('é'), KeyModifiers::NONE, "é".as_bytes()),
        (KeyCode::Char('c'), KeyModifiers::CONTROL, b"\x03"),
        (KeyCode::Char('u'), KeyModifiers::CONTROL, b"\x15"),
        (KeyCode::Char('x'), KeyModifiers::ALT, b"\x1bx"),
        (KeyCode::Enter, KeyModifiers::NONE, b"\r"),
        (KeyCode::Enter, KeyModifiers::SHIFT, b"\x1b\r"),
        (KeyCode::Tab, KeyModifiers::NONE, b"\t"),
        (KeyCode::BackTab, KeyModifiers::SHIFT, b"\x1b[Z"),
        (KeyCode::Esc, KeyModifiers::NONE, b"\x1b"),
        (KeyCode::Backspace, KeyModifiers::NONE, b"\x7f"),
        (KeyCode::Up, KeyModifiers::NONE, b"\x1b[A"),
        (KeyCode::Left, KeyModifiers::CONTROL, b"\x1b[1;5D"),
        (KeyCode::Delete, KeyModifiers::NONE, b"\x1b[3~"),
        (KeyCode::PageDown, KeyModifiers::SHIFT, b"\x1b[6;2~"),
        (KeyCode::F(5), KeyModifiers::NONE, b"\x1b[15~"),
    ] {
        let routed = route(&press(code, modifiers), false);
        assert_eq!(bytes(&routed), sent, "{code:?} {modifiers:?}");
    }
}

#[test]
fn the_cursor_keys_follow_the_applications_mode_and_a_release_sends_nothing() {
    let up = press(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(bytes(&route(&up, true)), b"\x1bOA");
    assert_eq!(bytes(&route(&up, false)), b"\x1b[A");
    let mut released = press(KeyCode::Char('a'), KeyModifiers::NONE);
    released.kind = KeyEventKind::Release;
    assert_eq!(route(&released, false), Routed::Nothing);
    assert_eq!(
        route(&press(KeyCode::F(20), KeyModifiers::NONE), false),
        Routed::Nothing
    );
}

#[test]
fn the_chairs_own_keys_type_as_a_terminal_sends_them() {
    assert_eq!(bytes_of(Key::Char('y')), b"y");
    assert_eq!(bytes_of(Key::Enter), b"\r");
    assert_eq!(bytes_of(Key::CtrlC), b"\x03");
    assert_eq!(bytes_of(Key::Down), b"\x1b[B");
}

#[test]
fn ending_clears_the_line_types_exit_presses_enter_then_hangs_up_then_kills_each_once() {
    let began = Instant::now();
    let ending = Ending::at(began);
    let at = |ms: u64| began + Duration::from_millis(ms);

    let (ending, due) = ending.step(at(50));
    assert!(due.is_empty(), "{due:?}");
    let (ending, due) = ending.step(at(100));
    assert_eq!(
        due,
        [PhaseCommand::Type(b"\x15".to_vec())],
        "the line cleared again"
    );
    let (ending, due) = ending.step(at(150));
    assert!(due.is_empty(), "a gap after each key: {due:?}");
    let (ending, due) = ending.step(at(200));
    assert_eq!(due, [PhaseCommand::Type(b"/exit".to_vec())]);
    let (ending, due) = ending.step(at(500));
    assert!(due.is_empty(), "Enter waits 400 ms after /exit: {due:?}");
    let (ending, due) = ending.step(at(600));
    assert_eq!(due, [PhaseCommand::Type(b"\r".to_vec())]);
    let (ending, due) = ending.step(at(4000));
    assert!(due.is_empty(), "Enter only once: {due:?}");
    let (ending, due) = ending.step(at(5000));
    assert_eq!(due, [PhaseCommand::HangUp], "hung up after 5 s");
    let (ending, due) = ending.step(at(10_000));
    assert_eq!(due, [PhaseCommand::Kill]);
    let (_, due) = ending.step(at(60_000));
    assert!(due.is_empty(), "{due:?}");

    // A loop that wakes late types one key, never two in one write, and still hangs up and
    // kills in order.
    let (ending, due) = Ending::at(began).step(at(11_000));
    assert_eq!(
        due,
        [
            PhaseCommand::Type(b"\x15".to_vec()),
            PhaseCommand::HangUp,
            PhaseCommand::Kill
        ]
    );
    let (_, due) = ending.step(at(11_000));
    assert!(due.is_empty(), "the next key waits its gap: {due:?}");
}

#[test]
fn the_phase_item_names_its_version() {
    let state = ways();
    let item = state
        .inbox
        .iter()
        .find(|item| item.kind == crate::app::hub::next::NextKind::Phase)
        .expect("a phase item")
        .clone();
    assert_eq!(version_of(&item), Some(ReleaseVersion::from("0.5.3")));
    assert_eq!(first_due(&state), Some(ReleaseVersion::from("0.5.3")));

    let mut by_command = item.clone();
    by_command.title = "a phase is due".to_owned();
    by_command.command = Some("cd '/srv/a b' && zaf begin --version 1.0.0".to_owned());
    assert_eq!(version_of(&by_command), Some(ReleaseVersion::from("1.0.0")));

    let mut nameless = by_command;
    nameless.command = None;
    assert_eq!(version_of(&nameless), None);
}

#[test]
fn the_questions_read_as_the_design_has_them() {
    assert_eq!(
        Ask::Sign(".teams/releases/0.5.3/objectives.md".to_owned())
            .question("objectives")
            .to_string(),
        "Sign it? [Y/n]  .teams/releases/0.5.3/objectives.md"
    );
    assert_eq!(
        Ask::Leave.question("design").to_string(),
        "Claude has not marked the design ready. End the session? [y/N]"
    );
    assert!(Ask::Quit.question("design").to_string().ends_with("[y/N]"));
}

#[test]
fn the_status_line_asks_as_it_does_before_a_control() {
    use crate::cli::chair::theme::Theme;
    let text = |ask: &Ask| {
        super::spans(&ask.question("design"), Theme::new(true))
            .iter()
            .map(|span| span.content.to_string())
            .collect::<String>()
    };
    assert_eq!(
        text(&Ask::Sign("design.md".to_owned())),
        "? Sign it? [Y/n]  design.md"
    );
    assert_eq!(
        text(&Ask::Quit),
        "? Quit? The Claude session ends with zaf. [y/N]"
    );
}

/// The chair drawn into a buffer of its size, as text rows.
fn drawn(chair: &crate::cli::chair::model::Chair, now: Instant) -> Vec<String> {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let (columns, rows) = chair.size;
    let area = Rect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    let wall = ways().gathered_at + Duration::from_secs(7);
    crate::cli::chair::draw::draw_into(chair, &mut buffer, area, now, wall);
    (0..rows)
        .map(|y| (0..columns).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

#[test]
fn the_pane_draws_the_sessions_screen_over_detail_and_live() {
    use crate::cli::chair::panes::phase::PhaseEvent;
    use tui_term::vt100;
    let now = Instant::now();
    let mut chair = crate::cli::chair::support::chair((100, 30));
    chair.key(Key::Char('o'), now);
    let starting = drawn(&chair, now);
    assert!(
        starting
            .iter()
            .any(|row| row.contains("starting the phase of 0.5.3…")),
        "{starting:#?}"
    );

    chair.phase_event(PhaseEvent::Started("design".to_owned()), now);
    let (rows, columns) = chair.phase_size().expect("laid out");
    let mut parser = vt100::Parser::new(rows, columns, 0);
    parser.process(b"\x1b[1mWelcome to Claude Code\x1b[0m\r\n> write the design");
    chair.phase_screen(parser.screen().clone());
    let shown = drawn(&chair, now);

    let title = shown
        .iter()
        .find(|row| row.contains("claude design 0.5.3"))
        .expect("the pane's title");
    assert!(title.contains("^w c leaves"), "{title}");
    let at = |text: &str| shown.iter().position(|row| row.contains(text));
    let title_row = at("claude design 0.5.3").expect("the title");
    assert_eq!(
        at("Welcome to Claude Code"),
        Some(title_row + 1),
        "{shown:#?}"
    );
    assert_eq!(
        at("> write the design█"),
        Some(title_row + 2),
        "the cursor too"
    );
    assert!(
        !shown.iter().any(|row| row.contains("Live")),
        "Detail and Live are under the pane"
    );
    assert!(shown[29].contains("go to Claude"), "{}", shown[29]);
}
