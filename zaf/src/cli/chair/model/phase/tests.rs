use std::time::{Duration, Instant};

use super::super::{Chair, Command, Mode, Status};
use super::signed_path;
use crate::app::chair::gatherer::GatherEvent;
use crate::app::chair::{ReleaseVersion, WatchView};
use crate::cli::chair::build::Control;
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::palette;
use crate::cli::chair::panes::Tone;
use crate::cli::chair::panes::phase::{PhaseCommand, PhaseEvent, QUIET_FOR, Question, Stage};
use crate::cli::chair::support::{chair, root, start, ways};

const SIZE: (u16, u16) = (100, 30);

fn press(chair: &mut Chair, keys: &[Key], now: Instant) -> Vec<Command> {
    keys.iter().flat_map(|key| chair.key(*key, now)).collect()
}

fn typed(bytes: &[u8]) -> Command {
    Command::Phase(PhaseCommand::Type(bytes.to_vec()))
}

/// The chair with the design of 0.5.3 started from `o`, its action running.
fn running(now: Instant) -> Chair {
    let mut chair = chair(SIZE);
    let commands = press(&mut chair, &[Key::Char('o')], now);
    assert!(
        matches!(
            commands.as_slice(),
            [Command::Phase(PhaseCommand::Start { version, .. })]
                if *version == ReleaseVersion::from("0.5.3")
        ),
        "{commands:?}"
    );
    chair.phase_event(PhaseEvent::Started("design".to_owned()), now);
    chair
}

/// The question on the status line about the session, as plain words.
fn asked(chair: &Chair, now: Instant) -> Option<String> {
    match chair.status(now) {
        Status::Phase(question) => Some(question.to_string()),
        _ => None,
    }
}

fn stage(chair: &Chair) -> Option<Stage> {
    chair.phase.session.as_ref().map(|session| session.stage)
}

#[test]
fn o_lays_the_phase_pane_over_detail_and_live_at_its_size() {
    let mut chair = chair(SIZE);
    let commands = press(&mut chair, &[Key::Char('o')], start());

    assert!(chair.layout.phase_open());
    assert_eq!(chair.focus, PaneId::Phase);
    let (rows, columns) = chair.phase_size().expect("laid out");
    assert_eq!(
        commands,
        [Command::Phase(PhaseCommand::Start {
            root: root(),
            version: ReleaseVersion::from("0.5.3"),
            rows,
            columns,
        })]
    );
    let shown: Vec<PaneId> = chair
        .layout
        .panes(chair.panes_area())
        .into_iter()
        .map(|(pane, _)| pane)
        .collect();
    assert!(shown.contains(&PaneId::Phase), "{shown:?}");
    assert!(!shown.contains(&PaneId::Detail) && !shown.contains(&PaneId::Live));

    // A second `o` starts nothing more.
    chair.set_focus(PaneId::Inbox);
    assert!(press(&mut chair, &[Key::Char('o')], start()).is_empty());
    assert_eq!(chair.focus, PaneId::Phase, "the pane takes the focus back");
}

#[test]
fn enter_on_the_due_phase_item_runs_it() {
    let mut chair = chair(SIZE);
    let now = start();
    let commands = press(
        &mut chair,
        &[Key::Char('j'), Key::Char('j'), Key::Enter],
        now,
    );
    assert!(
        matches!(
            commands.as_slice(),
            [Command::Phase(PhaseCommand::Start { .. })]
        ),
        "{commands:?}"
    );
    assert_eq!(chair.focus, PaneId::Phase);
}

#[test]
fn while_the_pane_has_the_focus_keys_go_to_claude_but_ctrl_w() {
    let now = start();
    let mut chair = running(now);
    assert!(chair.typing_to_claude());

    assert_eq!(press(&mut chair, &[Key::Char('q')], now), [typed(b"q")]);
    assert_eq!(press(&mut chair, &[Key::CtrlC], now), [typed(b"\x03")]);
    assert_eq!(press(&mut chair, &[Key::Char(':')], now), [typed(b":")]);
    assert_eq!(chair.mode, Mode::Normal, "the palette did not open");

    // Ctrl-w, then a window key: the chair's.
    assert!(press(&mut chair, &[Key::CtrlW], now).is_empty());
    assert_eq!(chair.mode, Mode::Window);
    assert!(!chair.typing_to_claude());
    press(&mut chair, &[Key::Char('h')], now);
    assert_ne!(chair.focus, PaneId::Phase, "moved away");
    assert_eq!(stage(&chair), Some(Stage::Running), "the session goes on");
    assert!(!chair.typing_to_claude());
}

#[test]
fn ctrl_w_c_asks_first_when_the_phase_is_not_ready() {
    let now = start();
    let mut chair = running(now);

    assert!(press(&mut chair, &[Key::CtrlW, Key::Char('c')], now).is_empty());
    assert_eq!(
        asked(&chair, now).as_deref(),
        Some("Claude has not marked the design ready. End the session? [y/N]")
    );
    assert!(!chair.typing_to_claude(), "the question takes the keys");

    // Enter keeps it going; y ends it.
    assert!(press(&mut chair, &[Key::Enter], now).is_empty());
    assert_eq!(stage(&chair), Some(Stage::Running));
    let commands = press(
        &mut chair,
        &[Key::CtrlW, Key::Char('c'), Key::Char('y')],
        now,
    );
    assert_eq!(commands, [typed(b"\x0c")], "Claude's input cleared first");
    assert!(matches!(stage(&chair), Some(Stage::Ending(_))));
}

#[test]
fn the_ready_file_types_exit_then_enter_then_hangs_up_after_five_seconds() {
    let now = start();
    let mut chair = running(now);

    let commands = chair.phase_event(PhaseEvent::Ready, now);

    assert_eq!(
        commands,
        [typed(b"\x0c")],
        "Claude's input cleared before /exit: nothing typed ahead is sent (N6)"
    );
    assert_eq!(
        chair
            .note
            .as_ref()
            .map(|note| (note.tone, note.words.as_str())),
        Some((Tone::Done, "Ready for Exit."))
    );
    assert!(chair.phase.session.as_ref().is_some_and(|s| s.ready));
    assert_eq!(
        chair.tick(now + Duration::from_millis(100)),
        [typed(b"\x15")]
    );
    assert_eq!(
        chair.tick(now + Duration::from_millis(200)),
        [typed(b"/exit")]
    );
    assert!(chair.tick(now + Duration::from_millis(450)).is_empty());
    assert_eq!(chair.tick(now + Duration::from_millis(600)), [typed(b"\r")]);
    assert!(chair.tick(now + Duration::from_secs(2)).is_empty());
    assert_eq!(
        chair.tick(now + Duration::from_secs(5)),
        [Command::Phase(PhaseCommand::HangUp)]
    );
    // Told again, nothing more is typed.
    assert!(chair.phase_event(PhaseEvent::Ready, now).is_empty());
    // Ctrl-w c once ready: the session is already ending.
    chair.set_focus(PaneId::Phase);
    assert!(press(&mut chair, &[Key::CtrlW, Key::Char('c')], now).is_empty());
}

#[test]
fn the_end_closes_the_pane_and_the_sign_question_follows() {
    let now = start();
    let mut chair = running(now);
    chair.phase_event(PhaseEvent::Ready, now);

    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    assert!(!chair.layout.phase_open(), "Detail and Live are back");
    assert_eq!(chair.focus, PaneId::Inbox);
    assert_eq!(stage(&chair), Some(Stage::Closed));
    assert!(
        chair.tick(now + Duration::from_secs(6)).is_empty(),
        "no hang-up"
    );

    chair.phase_event(
        PhaseEvent::Asked("Sign /srv/ways/.teams/releases/0.5.3/design.md? [Y/n] ".to_owned()),
        now,
    );
    // The pane closed on its own: a second passes before a key counts (R1).
    let now = now + QUIET_FOR;
    assert_eq!(
        chair.status(now),
        Status::Phase(Question {
            words: "Sign it?".to_owned(),
            yes: true,
            aside: Some(".teams/releases/0.5.3/design.md".to_owned()),
        })
    );
    assert!(
        press(&mut chair, &[Key::Char('j')], now).is_empty(),
        "j answers nothing"
    );
    assert_eq!(
        press(&mut chair, &[Key::Enter], now),
        [Command::Phase(PhaseCommand::Answer("y\n".to_owned()))]
    );

    let commands = chair.phase_event(
        PhaseEvent::Done {
            tone: Tone::Done,
            words: "signed .teams/releases/0.5.3/design.md".to_owned(),
            next: None,
        },
        now,
    );
    assert_eq!(commands, [Command::ReadNow(0)]);
    assert!(chair.phase.session.is_none());
    assert!(!chair.phase.follow);
}

#[test]
fn n_leaves_the_document_unsigned() {
    let now = start();
    let mut chair = running(now);
    chair.phase_event(
        PhaseEvent::Exited {
            ready: false,
            leads: true,
        },
        now,
    );
    chair.phase_event(PhaseEvent::Asked("Sign d.md? [Y/n] ".to_owned()), now);
    assert_eq!(
        press(&mut chair, &[Key::Char('n')], now + QUIET_FOR),
        [Command::Phase(PhaseCommand::Answer("n\n".to_owned()))]
    );
}

#[test]
fn signed_objectives_lead_on_to_the_design_item() {
    let now = start();
    let mut chair = running(now);
    chair.set_focus(PaneId::Epics);
    chair.phase_event(
        PhaseEvent::Done {
            tone: Tone::Done,
            words: "signed objectives.md".to_owned(),
            next: Some("design".to_owned()),
        },
        now,
    );
    assert_eq!(
        chair.note.as_ref().map(|note| note.words.as_str()),
        Some("signed objectives.md · the design is due next: o runs it")
    );
    assert!(chair.phase.follow);

    chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));

    assert!(!chair.phase.follow);
    assert_eq!(chair.focus, PaneId::Inbox);
    assert_eq!(chair.cursor.inbox, 2, "the phase item is selected");
}

#[test]
fn q_asks_first_while_a_session_runs() {
    let now = start();
    let mut chair = running(now);
    chair.set_focus(PaneId::Inbox);

    assert!(press(&mut chair, &[Key::Char('q')], now).is_empty());
    assert_eq!(
        asked(&chair, now).as_deref(),
        Some("Quit? The Claude session ends with zaf. [y/N]")
    );
    assert!(press(&mut chair, &[Key::Char('n')], now).is_empty());
    assert_eq!(
        press(&mut chair, &[Key::Char('q'), Key::Char('y')], now),
        [Command::Quit]
    );

    // Once the action has ended (and its pane's quiet second has passed), q quits at once.
    let mut chair = running(now);
    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    assert_eq!(
        press(&mut chair, &[Key::Char('q')], now + QUIET_FOR),
        [Command::Quit]
    );
}

#[test]
fn a_refusal_closes_the_pane_and_says_why() {
    let now = start();
    let mut chair = running(now);
    chair.phase_event(
        PhaseEvent::Refused("no phase of 0.5.3 is due".to_owned()),
        now,
    );
    assert!(chair.phase.session.is_none());
    assert!(!chair.layout.phase_open());
    assert_eq!(
        chair
            .note
            .as_ref()
            .map(|note| (note.tone, note.words.as_str())),
        Some((Tone::Failed, "no phase of 0.5.3 is due"))
    );
}

#[test]
fn the_sign_prompt_names_its_document_from_the_root() {
    assert_eq!(
        signed_path(
            "Sign /srv/ways/.teams/releases/0.5.3/objectives.md? [Y/n] ",
            &root()
        ),
        ".teams/releases/0.5.3/objectives.md"
    );
    assert_eq!(
        signed_path("Sign /elsewhere/design.md? [Y/n] ", &root()),
        "/elsewhere/design.md"
    );
}

#[test]
fn the_palette_starts_the_due_phase_and_leaves_its_pane_from_anywhere() {
    let now = start();
    let mut chair = chair(SIZE);
    let at = |name: &str| {
        palette::entries()
            .position(|entry| entry.name == name)
            .expect("listed")
    };
    press(&mut chair, &[Key::CtrlW, Key::Char('c')], now);
    assert_eq!(
        chair.note.as_ref().map(|note| note.words.as_str()),
        Some("no Claude session is open")
    );

    chair.mode = Mode::Palette(at("Start the due phase"));
    let commands = press(&mut chair, &[Key::Enter], now);
    assert!(
        matches!(
            commands.as_slice(),
            [Command::Phase(PhaseCommand::Start { .. })]
        ),
        "{commands:?}"
    );
    chair.phase_event(PhaseEvent::Started("design".to_owned()), now);
    press(&mut chair, &[Key::CtrlW, Key::Char('h')], now);
    assert_ne!(chair.focus, PaneId::Phase);

    chair.mode = Mode::Palette(at("Leave the phase pane"));
    assert!(press(&mut chair, &[Key::Enter], now).is_empty());
    assert_eq!(
        asked(&chair, now).as_deref(),
        Some("Claude has not marked the design ready. End the session? [y/N]")
    );
    assert_eq!(press(&mut chair, &[Key::Char('y')], now), [typed(b"\x0c")]);

    // The palette's q asks first too while the session runs.
    chair.mode = Mode::Palette(at("Quit (the build goes on)"));
    assert!(press(&mut chair, &[Key::Enter], now).is_empty());
    assert_eq!(
        asked(&chair, now).as_deref(),
        Some("Quit? The Claude session ends with zaf. [y/N]")
    );
}

#[test]
fn a_paste_goes_to_claude_bracketed_when_its_terminal_asked() {
    let now = start();
    let mut chair = running(now);
    assert!(chair.typing_to_claude());

    assert_eq!(
        chair.paste("one\rtwo", now),
        [typed(b"one\rtwo")],
        "as it came while the session has not asked for bracketed paste"
    );

    let mut parser = tui_term::vt100::Parser::new(20, 80, 0);
    parser.process(b"\x1b[?2004h");
    chair.phase_screen(parser.screen().clone());
    assert_eq!(
        chair.paste("one\rtwo\x1b[201~/exit\r", now),
        [typed(b"\x1b[200~one\rtwo[201~/exit\r\x1b[201~")],
        "bracketed, and no marker inside ends it early"
    );
    // Markers nested so that taking one out once would make another (N1 of the re-check), and
    // the other controls a terminal acts on: none reaches Claude inside the paste.
    for nested in [
        "a\x1b[20\x1b[20\x1b[200~1~1~\rrun this",
        "a\x1b[2\x1b[200~01~\rrun this",
        "a\u{9b}201~\x07\x03\x7f\rrun this",
    ] {
        let commands = chair.paste(nested, now);
        let [Command::Phase(PhaseCommand::Type(bytes))] = commands.as_slice() else {
            panic!("one paste typed: {commands:?}");
        };
        let inner = bytes
            .strip_prefix(b"\x1b[200~")
            .and_then(|rest| rest.strip_suffix(b"\x1b[201~"))
            .expect("wrapped");
        let text = String::from_utf8(inner.to_vec()).expect("text");
        assert!(
            text.chars()
                .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')),
            "{text:?}"
        );
        assert!(text.ends_with("\rrun this"), "{text:?}");
    }
}

#[test]
fn a_paste_answers_no_question_about_the_session() {
    let now = start();
    let mut chair = running(now);
    chair.phase_event(PhaseEvent::Ready, now);
    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    chair.phase_event(
        PhaseEvent::Asked(format!("Sign {}/x/design.md? [Y/n] ", root().display())),
        now,
    );

    let later = now + QUIET_FOR;
    let commands = chair.paste("n\r", later);

    assert!(commands.is_empty(), "{commands:?}");
    assert!(asked(&chair, later).is_some(), "the question stays open");
}

#[test]
fn keys_typed_to_claude_never_spill_into_the_chair_when_its_pane_closes_on_its_own() {
    let now = start();
    let mut chair = running(now);
    assert!(chair.typing_to_claude());
    // Claude ran `zaf begin ready`: the chair types `/exit` while the Engineer still types.
    chair.phase_event(PhaseEvent::Ready, now);
    assert!(
        press(&mut chair, &[Key::Char('l'), Key::Char('o')], now).is_empty(),
        "what is typed while the session ends is dropped, not typed after /exit"
    );
    assert!(chair.paste("ok", now).is_empty());

    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    assert_eq!(chair.focus, PaneId::Inbox);
    let soon = now + QUIET_FOR / 2;
    let commands = press(
        &mut chair,
        &"oks fine\rsq"
            .chars()
            .map(|c| if c == '\r' { Key::Enter } else { Key::Char(c) })
            .collect::<Vec<_>>(),
        soon,
    );
    assert!(commands.is_empty(), "{commands:?}");
    assert_eq!(chair.mode, Mode::Normal);
    assert!(chair.pending.is_empty(), "nothing ruled or signed");
    assert!(chair.click(10, 5, soon).is_empty());

    chair.phase_event(PhaseEvent::Asked("Sign d.md? [Y/n] ".to_owned()), soon);
    assert_eq!(
        chair.status(soon),
        Status::Note(chair.note.as_ref().expect("a note")),
        "the status line says the session ended before it asks"
    );
    assert_eq!(
        chair.note.as_ref().map(|note| note.words.as_str()),
        Some("session ended")
    );
    assert!(
        press(&mut chair, &[Key::Enter], soon).is_empty(),
        "Enter typed for Claude signs nothing"
    );

    // Each key dropped starts the quiet second again (Q1): a second after the close is not
    // enough while the typing goes on.
    let still = now + QUIET_FOR;
    assert!(
        press(&mut chair, &[Key::Char('y')], still).is_empty(),
        "half a second after the last key, still quiet"
    );
    assert!(
        chair.paste("y", still).is_empty(),
        "a paste starts it again too"
    );
    assert_eq!(
        chair.status(still + QUIET_FOR / 2),
        Status::Note(chair.note.as_ref().expect("a note")),
        "the status line still says why"
    );
    let later = still + QUIET_FOR;
    assert!(asked(&chair, later).is_some());
    assert_eq!(
        press(&mut chair, &[Key::Enter], later),
        [Command::Phase(PhaseCommand::Answer("y\n".to_owned()))],
        "a second with no key, Enter signs as D10 has it"
    );
}

#[test]
fn the_quiet_second_outlasts_the_note_that_says_why() {
    let now = start();
    let mut chair = running(now);
    chair.phase_event(PhaseEvent::Ready, now);
    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    chair.phase_event(PhaseEvent::Asked("Sign d.md? [Y/n] ".to_owned()), now);
    // Typing goes on for longer than a note lasts, a key every half second.
    let mut at = now;
    while at < now + crate::cli::chair::model::NOTE_FOR * 2 {
        at += QUIET_FOR / 2;
        assert!(press(&mut chair, &[Key::Char('y')], at).is_empty());
        let _ = chair.tick(at);
        assert_eq!(
            chair.note.as_ref().map(|note| note.words.as_str()),
            Some("session ended"),
            "the note lasts as long as the quiet"
        );
    }
    assert!(asked(&chair, at + QUIET_FOR).is_some());
}

#[test]
fn a_pane_the_engineer_closes_takes_keys_at_once() {
    let now = start();
    let mut chair = running(now);
    chair.phase.session.as_mut().expect("a session").ready = true;
    press(&mut chair, &[Key::CtrlW, Key::Char('c')], now);
    assert!(matches!(stage(&chair), Some(Stage::Ending(_))));
    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    assert!(!chair.quiet(now), "asked for, so no quiet second");
    assert_eq!(press(&mut chair, &[Key::Char('q')], now), [Command::Quit]);
}

/// The chair whose session closed on its own, a second on, so keys count again.
fn closed(now: Instant) -> (Chair, Instant) {
    let mut chair = running(now);
    chair.phase_event(PhaseEvent::Ready, now);
    chair.phase_event(
        PhaseEvent::Exited {
            ready: true,
            leads: true,
        },
        now,
    );
    (chair, now + QUIET_FOR)
}

/// The session's action asks to sign `d.md`.
fn asks_to_sign(chair: &mut Chair, now: Instant) {
    chair.phase_event(PhaseEvent::Asked("Sign d.md? [Y/n] ".to_owned()), now);
}

fn answers_sign(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::Phase(PhaseCommand::Answer(_))))
}

fn still_asks(chair: &Chair) -> bool {
    chair
        .phase
        .session
        .as_ref()
        .is_some_and(|session| session.ask.is_some())
}

#[test]
fn chair_ruling_enter_does_not_sign() {
    let (mut chair, now) = closed(start());
    press(&mut chair, &[Key::Char('a')], now);
    assert!(matches!(chair.mode, Mode::Answer(_)), "{:?}", chair.mode);
    asks_to_sign(&mut chair, now);
    assert_eq!(asked(&chair, now), None, "the sign question waits");

    let mut commands = press(&mut chair, &[Key::Char('o'), Key::Char('k')], now);
    commands.extend(press(&mut chair, &[Key::Enter], now));

    assert!(!answers_sign(&commands), "{commands:?}");
    assert_eq!(chair.mode, Mode::Normal, "the Enter sent the ruling");
    assert!(!chair.pending.is_empty(), "the ruling is queued");
    assert!(still_asks(&chair), "the document is still unsigned");
}

#[test]
fn chair_build_question_y_does_not_sign() {
    let (mut chair, now) = closed(start());
    let mut state = ways();
    state.now.watch = Some(WatchView {
        running: false,
        pid: None,
        version: None,
    });
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    press(&mut chair, &[Key::Char('b')], now);
    assert!(matches!(chair.mode, Mode::Ask(_)), "{:?}", chair.mode);
    asks_to_sign(&mut chair, now);
    assert_eq!(asked(&chair, now), None, "the sign question waits");

    let commands = press(&mut chair, &[Key::Char('y')], now);

    assert!(!answers_sign(&commands), "{commands:?}");
    assert!(matches!(
        commands.as_slice(),
        [Command::Control(Control::StartBuild { .. })]
    ));
    assert!(still_asks(&chair), "the document is still unsigned");
}

#[test]
fn chair_sign_question_asked_after_answer_box_closes() {
    let (mut chair, now) = closed(start());
    press(&mut chair, &[Key::Char('a')], now);
    asks_to_sign(&mut chair, now);
    assert_eq!(asked(&chair, now), None, "the sign question waits");
    press(&mut chair, &[Key::Esc], now);
    assert_eq!(chair.mode, Mode::Normal);

    let later = now + QUIET_FOR;
    assert!(asked(&chair, later).is_some(), "asked once the box closes");
    assert_eq!(
        press(&mut chair, &[Key::Enter], later),
        [Command::Phase(PhaseCommand::Answer("y\n".to_owned()))]
    );
}

/// A ruling in the answer box ended with Enter while the sign question waited behind it.
fn ruling_ended_with_enter() -> (Chair, Instant) {
    let (mut chair, now) = closed(start());
    press(&mut chair, &[Key::Char('a')], now);
    asks_to_sign(&mut chair, now);
    press(
        &mut chair,
        &[Key::Char('o'), Key::Char('k'), Key::Enter],
        now,
    );
    assert_eq!(chair.mode, Mode::Normal, "the Enter sent the ruling");
    (chair, now)
}

#[test]
fn sign_question_after_mode_closes_ignores_immediate_enter() {
    let (mut chair, now) = ruling_ended_with_enter();

    assert!(
        chair.quiet(now),
        "the quiet time starts when the mode closes"
    );
    let commands = press(&mut chair, &[Key::Enter], now);

    assert!(!answers_sign(&commands), "{commands:?}");
    assert!(still_asks(&chair), "the document is still unsigned");
}

#[test]
fn sign_question_after_mode_closes_signs_after_quiet_for() {
    let (mut chair, now) = ruling_ended_with_enter();

    let later = now + QUIET_FOR;
    assert!(asked(&chair, later).is_some());
    assert_eq!(
        press(&mut chair, &[Key::Enter], later),
        [Command::Phase(PhaseCommand::Answer("y\n".to_owned()))]
    );
}

#[test]
fn sign_question_after_mode_closes_ignores_enter_then_y() {
    let (mut chair, now) = ruling_ended_with_enter();

    let commands = press(&mut chair, &[Key::Enter, Key::Char('y')], now);

    assert!(!answers_sign(&commands), "{commands:?}");
    assert!(still_asks(&chair), "the document is still unsigned");
}
