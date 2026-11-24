use std::time::Duration;

use super::{Chair, Command, Mode, Status};
use crate::app::chair::gatherer::GatherEvent;
use crate::app::chair::look::{EpicRead, Look, Looked, TicketRead};
use crate::app::chair::pending::UNDO_WINDOW;
use crate::app::chair::{
    ChairAction, Fix, FixVerb, InboxItem, Outcome, ReleaseVersion, Ruling, Signing, WatchView,
};
use crate::app::hub::next::NextKind;
use crate::cli::chair::build::{Ask, Control};
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::palette;
use crate::cli::chair::panes::{DetailView, Tone, hint};
use crate::cli::chair::pings::Notice;
use crate::cli::chair::support::{chair, root, start, ways};
use crate::domain::{EpicStage, TicketId};

const SMALL: (u16, u16) = (80, 24);

fn press(chair: &mut Chair, keys: &[Key], now: std::time::Instant) -> Vec<Command> {
    keys.iter().flat_map(|key| chair.key(*key, now)).collect()
}

fn typed(text: &str) -> Vec<Key> {
    text.chars().map(Key::Char).collect()
}

fn note(chair: &Chair) -> Option<(Tone, String)> {
    chair
        .note
        .as_ref()
        .map(|note| (note.tone, note.words.clone()))
}

#[test]
fn j_and_k_move_and_detail_follows_the_focus() {
    let mut chair = chair(SMALL);
    let now = start();

    press(
        &mut chair,
        &[Key::Char('j'), Key::Char('j'), Key::Char('j')],
        now,
    );
    assert_eq!(chair.cursor.inbox, 2, "held at the last item");
    press(&mut chair, &[Key::Char('k')], now);
    assert_eq!(chair.cursor.inbox, 1);

    press(&mut chair, &[Key::Tab], now);
    assert_eq!(chair.focus, PaneId::Problems);
    assert_eq!(chair.cursor.detail, DetailView::Problem);
    press(&mut chair, &[Key::Tab, Key::Tab], now);
    assert_eq!(chair.focus, PaneId::Epics);
    assert_eq!(chair.cursor.detail, DetailView::Epic);
}

#[test]
fn window_keys_move_resize_and_maximise() {
    let mut chair = chair(SMALL);
    let now = start();

    press(&mut chair, &[Key::CtrlW, Key::Char('l')], now);
    assert_eq!(chair.focus, PaneId::Epics);
    assert_eq!(chair.mode, Mode::Normal, "one window command per Ctrl-w");
    press(&mut chair, &[Key::CtrlW, Key::Down], now);
    assert_eq!(chair.focus, PaneId::Detail);
    press(&mut chair, &[Key::CtrlW, Key::Char('w')], now);
    assert_eq!(chair.focus, PaneId::Live);

    press(&mut chair, &[Key::CtrlW, Key::Char('o')], now);
    assert_eq!(chair.layout.maximised(), Some(PaneId::Live));
    press(&mut chair, &[Key::CtrlW, Key::Char('h')], now);
    assert_eq!(
        chair.focus,
        PaneId::Live,
        "no moving while one pane fills the screen"
    );
    press(&mut chair, &[Key::CtrlW, Key::Char('o')], now);
    assert_eq!(chair.layout.maximised(), None);

    chair.set_focus(PaneId::Problems);
    press(&mut chair, &[Key::CtrlW, Key::Char('-')], now);
    assert_eq!(
        note(&chair),
        Some((Tone::Plain, "that pane cannot get smaller".to_owned()))
    );
    chair.set_focus(PaneId::Epics);
    let before = chair.layout.clone();
    press(&mut chair, &[Key::CtrlW, Key::Char('+')], now);
    assert_ne!(chair.layout, before);
    press(&mut chair, &[Key::CtrlW, Key::Char('=')], now);
    assert_eq!(chair.layout, before);
}

#[test]
fn enter_opens_the_answer_box_with_claudes_answer_in_it() {
    let mut chair = chair(SMALL);
    let now = start();

    press(&mut chair, &[Key::Enter], now);

    let Mode::Answer(answer) = &chair.mode else {
        panic!("the answer box is open: {:?}", chair.mode);
    };
    assert_eq!(answer.question, TicketId::from(285));
    assert_eq!(answer.text, "Change nothing: print the plan and exit.");
    assert_eq!(chair.focus, PaneId::Detail);
    assert_eq!(chair.cursor.detail, DetailView::Answer);
    assert_eq!(chair.hints()[1].what, "rule");
}

#[test]
fn the_answer_box_edits_and_refuses_an_empty_ruling() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter], now);

    press(&mut chair, &typed(" Yes!"), now);
    press(&mut chair, &[Key::Backspace], now);
    let Mode::Answer(answer) = &chair.mode else {
        panic!("still open");
    };
    assert!(answer.text.ends_with("exit. Yes"), "{}", answer.text);

    press(&mut chair, &[Key::CtrlU, Key::Enter], now);
    assert!(
        matches!(chair.mode, Mode::Answer(_)),
        "an empty ruling keeps the box open"
    );
    assert_eq!(
        note(&chair),
        Some((
            Tone::Failed,
            "the answer is empty; nothing was written".to_owned()
        ))
    );
    assert!(chair.pending.is_empty());

    press(&mut chair, &[Key::Esc], now);
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(chair.focus, PaneId::Inbox);
    assert_eq!(note(&chair), Some((Tone::Plain, "not ruled".to_owned())));
}

#[test]
fn a_ruling_waits_its_undo_window_then_is_written() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::CtrlU], now);
    press(&mut chair, &typed("Change nothing."), now);

    let commands = press(&mut chair, &[Key::Enter], now);

    assert!(commands.is_empty(), "nothing is written at once");
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(chair.focus, PaneId::Inbox);
    let shown = chair.shown().expect("read");
    assert!(
        shown.question(TicketId::from(285)).is_none(),
        "it leaves the inbox"
    );
    assert_eq!(chair.badge(0), 2);
    assert_eq!(
        chair.status(now + Duration::from_millis(1_200)),
        Status::Pending {
            words: "#285 ruled".to_owned(),
            left: 4,
            more: 0,
        }
    );
    assert_eq!(chair.hints()[0].key, "u");

    assert!(
        chair
            .tick(now + UNDO_WINDOW - Duration::from_millis(1))
            .is_empty()
    );
    let written = chair.tick(now + UNDO_WINDOW);
    let ruling = Ruling::new(root(), TicketId::from(285), "Change nothing.").expect("words");
    assert_eq!(written, [Command::Write(ChairAction::Rule(ruling))]);
    assert!(
        chair
            .shown()
            .expect("read")
            .question(TicketId::from(285))
            .is_none(),
        "it stays out until a read shows it gone"
    );
}

#[test]
fn u_takes_a_ruling_back_before_it_is_written() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::Enter], now);
    assert_eq!(chair.pending.len(), 1);

    press(&mut chair, &[Key::Char('u')], now);

    assert!(chair.pending.is_empty());
    assert!(chair.hidden.is_empty());
    assert!(
        chair
            .shown()
            .expect("read")
            .question(TicketId::from(285))
            .is_some()
    );
    assert_eq!(
        note(&chair),
        Some((Tone::Undone, "#285 is back in your inbox".to_owned()))
    );
    assert!(
        chair.tick(now + UNDO_WINDOW * 2).is_empty(),
        "never written"
    );
}

#[test]
fn q_and_ctrl_c_quit_from_anywhere() {
    let mut chair = chair(SMALL);
    let now = start();

    assert_eq!(chair.key(Key::Char('q'), now), [Command::Quit]);
    assert_eq!(chair.key(Key::CtrlC, now), [Command::Quit]);
    press(&mut chair, &[Key::Enter], now);
    assert_eq!(
        chair.key(Key::Char('q'), now),
        [],
        "q is typed into the answer box"
    );
    assert_eq!(chair.key(Key::CtrlC, now), [Command::Quit]);
}

#[test]
fn a_refused_ruling_comes_back_and_the_project_is_read_again() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::Enter], now);
    let written: [Command; 1] = chair.tick(now + UNDO_WINDOW).try_into().expect("one write");
    let [Command::Write(action)] = written else {
        panic!("a write");
    };

    let commands = chair.written(
        &action,
        &Outcome::Refused {
            question: TicketId::from(285),
            why: "#285 is not an open question; nothing was changed".to_owned(),
        },
        now,
    );

    assert_eq!(commands, [Command::ReadNow(0)]);
    assert!(chair.hidden.is_empty());
    assert_eq!(
        chair.note.as_ref().map(|note| note.tone),
        Some(Tone::Failed)
    );
}

#[test]
fn a_read_without_the_ruled_question_stops_hiding_it() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::Enter], now);
    let _ = chair.tick(now + UNDO_WINDOW);
    assert_eq!(chair.hidden.len(), 1);

    let at = std::time::SystemTime::now();
    assert!(chair.gathered(0, GatherEvent::Started(at)).is_empty());
    assert_eq!(chair.tab().reading, Some(at));
    chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));
    assert_eq!(
        chair.hidden.len(),
        1,
        "a read that still shows it keeps it hidden"
    );
    assert_eq!(chair.tab().reading, None);

    let mut ruled = ways();
    ruled.inbox.remove(0);
    chair.gathered(0, GatherEvent::Gathered(Box::new(ruled)));
    assert!(chair.hidden.is_empty());
}

#[test]
fn digits_and_clicks_on_the_tab_bar_switch_projects() {
    let mut chair = chair(SMALL);
    let now = start();
    chair.cursor.inbox = 2;

    assert_eq!(
        chair.key(Key::Char('2'), now),
        [Command::Shown { from: 0, to: 1 }]
    );
    assert_eq!(chair.active, 1);
    assert_eq!(
        chair.cursor.inbox, 0,
        "a project's cursor starts at the top"
    );
    assert!(
        chair.key(Key::Char('7'), now).is_empty(),
        "no seventh project"
    );

    let spans = chair.tab_spans();
    assert_eq!(spans[0], (6, 6 + 4 + 3), "ways and its ?3");
    let commands = chair.click(spans[0].0 + 1, 0, now);
    assert_eq!(commands, [Command::Shown { from: 1, to: 0 }]);
}

#[test]
fn a_click_focuses_a_pane_and_selects_its_row() {
    let mut chair = chair(SMALL);
    let now = start();

    chair.click(5, 13 + 2, now);
    assert_eq!(chair.focus, PaneId::Problems);
    assert_eq!(chair.cursor.problem, 1);

    chair.click(5, 3 + 1, now);
    assert_eq!(chair.focus, PaneId::Inbox);
    assert_eq!(chair.cursor.inbox, 1);

    chair.wheel(5, 4, true, now);
    assert_eq!(chair.cursor.inbox, 2);
}

#[test]
fn the_palette_lists_every_action_and_runs_the_chosen_one() {
    let mut chair = chair(SMALL);
    let now = start();

    press(&mut chair, &[Key::Char(':')], now);
    assert_eq!(chair.mode, Mode::Palette(0));
    press(&mut chair, &[Key::Char('k')], now);
    assert_eq!(
        chair.mode,
        Mode::Palette(palette::len() - 1),
        "round to the last"
    );
    assert_eq!(chair.key(Key::Enter, now), [Command::Quit]);

    press(&mut chair, &[Key::Char(':'), Key::Esc], now);
    assert_eq!(chair.mode, Mode::Normal);
    chair.cursor.inbox = 2;
    press(&mut chair, &[Key::Char(':'), Key::Enter], now);
    assert!(
        matches!(&chair.mode, Mode::Answer(answer) if answer.question == TicketId::from(285)),
        "the top question: {:?}",
        chair.mode
    );
}

#[test]
fn enter_on_a_problem_runs_a_zaf_fix_or_says_what_frees_it() {
    let mut chair = chair(SMALL);
    let now = start();
    chair.set_focus(PaneId::Problems);

    let commands = press(&mut chair, &[Key::Enter], now);
    let fix = Fix::new(
        root(),
        FixVerb::ProfileInit,
        "cd /srv/ways && zaf profile init",
    );
    assert_eq!(commands, [Command::Write(ChairAction::Fix(fix))]);

    press(&mut chair, &[Key::Char('j'), Key::Enter], now);
    assert_eq!(
        note(&chair),
        Some((
            Tone::Plain,
            "#230 is freed by the next tick: its instance has ended".to_owned()
        ))
    );
}

#[test]
fn epics_keys_pick_tickets_and_open_the_views() {
    let mut chair = chair(SMALL);
    let now = start();
    chair.set_focus(PaneId::Epics);

    press(&mut chair, &[Key::Right, Key::Right], now);
    assert_eq!(chair.cursor.ticket, 2);
    press(&mut chair, &[Key::Char('w')], now);
    assert_eq!(chair.cursor.detail, DetailView::Why(TicketId::from(234)));
    press(&mut chair, &[Key::Char('d')], now);
    assert_eq!(chair.cursor.detail, DetailView::Graph);
    press(&mut chair, &[Key::Char('j')], now);
    assert_eq!((chair.cursor.epic, chair.cursor.ticket), (1, 0));
    assert_eq!(
        chair.cursor.detail,
        DetailView::Graph,
        "the graph follows the epic"
    );

    press(&mut chair, &[Key::Enter], now);
    assert_eq!(chair.focus, PaneId::Detail);
    assert_eq!(chair.cursor.detail, DetailView::Read);
    press(&mut chair, &[Key::Esc], now);
    assert_eq!(chair.focus, PaneId::Epics);
    assert_eq!(chair.cursor.detail, DetailView::Epic);

    press(&mut chair, &[Key::Char('l')], now);
    assert_eq!(chair.cursor.live, Some(TicketId::from(255)));
    assert!(note(&chair).is_some_and(|(_, words)| words.contains("is not running: it is done")));
}

#[test]
fn a_terminal_below_80_by_24_does_not_draw_the_panes() {
    let mut chair = chair(SMALL);
    assert!(chair.fits());

    chair.resize(79, 24);
    assert!(!chair.fits());
    assert!(chair.click(5, 5, start()).is_empty());

    chair.resize(120, 32);
    assert!(chair.fits());
}

#[test]
fn a_shrunk_terminal_gives_the_splits_their_first_shares() {
    let mut chair = chair((120, 32));
    let now = start();
    chair.set_focus(PaneId::Inbox);
    for _ in 0..6 {
        press(&mut chair, &[Key::CtrlW, Key::Char('+')], now);
    }
    assert_ne!(chair.layout, crate::cli::chair::layout::Layout::chair());

    chair.resize(80, 24);

    assert!(chair.fits());
}

#[test]
fn the_inbox_kinds_are_answered_or_explained() {
    let mut chair = chair(SMALL);
    let shown = chair.shown().expect("read");
    assert_eq!(shown.inbox[2].kind, NextKind::Phase);
    chair.cursor.inbox = 1;

    press(&mut chair, &[Key::Char('a')], start());

    assert!(
        matches!(&chair.mode, Mode::Answer(answer) if answer.question == TicketId::from(285)),
        "a answers the top question whatever is selected"
    );
}

/// The chair over ways with no scheduler running.
fn idle(size: (u16, u16)) -> Chair {
    let mut chair = chair(size);
    let mut state = ways();
    state.now.watch = Some(WatchView {
        running: false,
        pid: None,
        version: None,
    });
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair
}

#[test]
fn b_asks_on_the_status_line_and_a_yes_starts_the_build() {
    let mut chair = idle(SMALL);
    let now = start();

    assert_eq!(press(&mut chair, &[Key::Char('b')], now), []);
    let start_build = Control::StartBuild {
        root: root(),
        version: ReleaseVersion::from("0.5.2"),
    };
    assert!(
        matches!(&chair.mode, Mode::Ask(Ask::Confirm { control, yes: true, .. }) if *control == start_build),
        "{:?}",
        chair.mode
    );
    assert!(matches!(chair.status(now), Status::Ask(_)));
    assert_eq!(chair.hints()[0], hint("Enter/y", "yes"));

    let commands = press(&mut chair, &[Key::Enter], now);

    assert_eq!(commands, [Command::Control(start_build)]);
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(
        note(&chair),
        Some((Tone::Plain, "starting the build of 0.5.2…".to_owned()))
    );
}

#[test]
fn b_while_a_scheduler_runs_says_so_and_asks_nothing() {
    let mut chair = chair(SMALL);
    press(&mut chair, &[Key::Char('b')], start());
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(
        note(&chair).map(|(_, words)| words),
        Some("a scheduler already builds 0.5.2; B stops it after its tick".to_owned())
    );
}

#[test]
fn shift_b_needs_a_yes_and_enter_keeps_the_scheduler_running() {
    let mut chair = chair(SMALL);
    let now = start();

    press(&mut chair, &[Key::Char('B')], now);
    assert!(matches!(
        chair.mode,
        Mode::Ask(Ask::Confirm { yes: false, .. })
    ));
    assert_eq!(press(&mut chair, &[Key::Enter], now), []);
    assert_eq!(
        note(&chair).map(|(_, words)| words),
        Some("the scheduler was not stopped".to_owned())
    );

    let commands = press(&mut chair, &[Key::Char('B'), Key::Char('y')], now);
    assert_eq!(
        commands,
        [Command::Control(Control::StopBuild {
            root: root(),
            pid: 4242,
        })]
    );
}

#[test]
fn q_during_an_ask_does_not_quit_and_ctrl_c_does() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Char('B')], now);
    assert_eq!(press(&mut chair, &[Key::Char('q')], now), []);
    assert!(matches!(chair.mode, Mode::Ask(_)), "q is no answer");
    assert_eq!(press(&mut chair, &[Key::CtrlC], now), [Command::Quit]);
}

#[test]
fn t_asks_the_scheduler_for_a_tick_and_x_stops_the_run_selected_in_now() {
    let mut chair = chair(SMALL);
    let now = start();

    let ticked = press(&mut chair, &[Key::Char('t'), Key::Char('y')], now);
    assert_eq!(ticked, [Command::Control(Control::Tick { root: root() })]);

    chair.set_focus(PaneId::Now);
    let stopped = press(&mut chair, &[Key::Char('x'), Key::Char('y')], now);
    assert_eq!(
        stopped,
        [Command::Control(Control::StopTask {
            root: root(),
            task: TicketId::from(243),
        })]
    );

    chair.set_focus(PaneId::Epics);
    press(&mut chair, &[Key::Right], now);
    assert_eq!(press(&mut chair, &[Key::Char('x')], now), []);
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(
        note(&chair).map(|(_, words)| words),
        Some("#233 is not running (waiting): nothing to stop".to_owned())
    );
}

#[test]
fn the_palette_runs_the_controls_and_opens_an_epic_in_its_pane() {
    let mut chair = chair(SMALL);
    let now = start();
    let at = |name: &str| {
        palette::entries()
            .position(|entry| entry.name == name)
            .expect("listed")
    };

    chair.mode = Mode::Palette(at("Stop the build"));
    press(&mut chair, &[Key::Enter], now);
    assert!(matches!(chair.mode, Mode::Ask(_)), "{:?}", chair.mode);
    press(&mut chair, &[Key::Esc], now);

    chair.mode = Mode::Palette(at("Open the selected epic"));
    press(&mut chair, &[Key::Enter], now);
    assert_eq!(
        (chair.focus, chair.cursor.detail),
        (PaneId::Detail, DetailView::Read),
        "Enter in Epics, wherever the focus was"
    );
}

#[test]
fn enter_on_ready_work_no_scheduler_ticks_starts_the_build() {
    let mut chair = idle(SMALL);
    let mut state = chair.tabs[0].state.clone().expect("read");
    state.inbox.push(InboxItem {
        kind: NextKind::Unticked,
        title: "1 ready task in ways: no scheduler ticks".to_owned(),
        blocks: 1,
        how: String::new(),
        command: Some("cd /srv/ways && zaf spawn --watch --version <version>".to_owned()),
        link: None,
        number: None,
        question: None,
        begins: None,
    });
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair.cursor.inbox = 3;

    press(&mut chair, &[Key::Enter], start());

    assert!(
        matches!(
            &chair.mode,
            Mode::Ask(Ask::Confirm {
                control: Control::StartBuild { .. },
                ..
            })
        ),
        "{:?}",
        chair.mode
    );
}

#[test]
fn a_control_is_said_and_its_project_read_again() {
    let mut chair = chair(SMALL);
    let now = start();
    let tick = Control::Tick { root: root() };

    let commands = chair.controlled(&tick, &Ok("asked the running scheduler".to_owned()), now);
    assert_eq!(commands, [Command::ReadNow(0)]);
    assert_eq!(
        note(&chair),
        Some((Tone::Done, "asked the running scheduler".to_owned()))
    );

    chair.controlled(&tick, &Err("no tick was asked for: why".to_owned()), now);
    assert_eq!(
        note(&chair),
        Some((Tone::Failed, "no tick was asked for: why".to_owned()))
    );
}

#[test]
fn a_new_question_on_any_tab_rings_and_notifies() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    let mut asked = state.inbox[1].clone();
    asked.number = Some(TicketId::from(293));
    asked.title = "Question #293: Expand ~ in install.sh?".to_owned();
    state.inbox.push(asked);

    let commands = chair.gathered(0, GatherEvent::Gathered(Box::new(state)));

    assert_eq!(
        commands,
        [
            Command::Bell,
            Command::Notify(Notice {
                title: "zaf · ways".to_owned(),
                body: "Question #293: Expand ~ in install.sh?".to_owned(),
            }),
            Command::Look(0, Look::Ticket(TicketId::from(243))),
        ],
        "and Live reads its ticket again"
    );
}

/// The chair with epic #226 unsigned, the only epic of 0.5.2 that is, and Epics on it.
fn unsigned_226() -> Chair {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.epics[1].stage = EpicStage::Unsigned;
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair.set_focus(PaneId::Epics);
    chair.cursor.epic = 1;
    chair
}

#[test]
fn s_queues_the_signature_and_the_last_of_a_version_says_b_builds_it() {
    let mut chair = unsigned_226();
    let now = start();
    let epic = TicketId::from(226);

    press(&mut chair, &[Key::Char('s')], now);
    assert_eq!(chair.pending.len(), 1);
    assert!(chair.looks.signs(&root(), epic));
    press(&mut chair, &[Key::Char('s')], now);
    assert_eq!(chair.pending.len(), 1, "one signature an epic");
    assert!(note(&chair).is_some_and(|(_, words)| words.contains("already being signed")));

    let written: [Command; 1] = chair.tick(now + UNDO_WINDOW).try_into().expect("one write");
    let [Command::Write(action)] = written else {
        panic!("a write");
    };
    assert_eq!(
        action,
        ChairAction::Sign(Signing {
            root: root(),
            epic,
            version: Some(ReleaseVersion::from("0.5.2")),
            last: true,
        })
    );
    let commands = chair.written(
        &action,
        &Outcome::Signed {
            epic,
            already: false,
            builds: Some(ReleaseVersion::from("0.5.2")),
        },
        now,
    );
    assert_eq!(commands, [Command::ReadNow(0)]);
    assert!(note(&chair).is_some_and(|(tone, words)| tone == Tone::Done
        && words.contains("every epic of 0.5.2 is signed: b starts the build")));
    assert!(
        chair.looks.signs(&root(), epic),
        "signing until a read shows it signed"
    );

    chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));
    assert!(!chair.looks.signs(&root(), epic));
}

#[test]
fn u_takes_a_signature_back_and_an_epic_not_unsigned_is_not_signed() {
    let mut chair = unsigned_226();
    let now = start();

    press(&mut chair, &[Key::Char('s'), Key::Char('u')], now);

    assert!(chair.pending.is_empty());
    assert!(chair.looks.signing.is_empty());
    assert_eq!(
        note(&chair),
        Some((Tone::Undone, "epic #226 not signed".to_owned()))
    );
    assert!(
        chair.tick(now + UNDO_WINDOW * 2).is_empty(),
        "never written"
    );

    chair.cursor.epic = 0;
    press(&mut chair, &[Key::Char('s')], now);
    assert!(chair.pending.is_empty());
    assert_eq!(
        note(&chair),
        Some((Tone::Plain, "epic #224 is already signed".to_owned()))
    );
}

#[test]
fn a_refused_signature_stops_showing_as_signing() {
    let mut chair = unsigned_226();
    let now = start();
    press(&mut chair, &[Key::Char('s')], now);
    let written: [Command; 1] = chair.tick(now + UNDO_WINDOW).try_into().expect("one write");
    let [Command::Write(action)] = written else {
        panic!("a write");
    };

    chair.written(
        &action,
        &Outcome::NotSigned {
            epic: TicketId::from(226),
            why: "team instance 1 on #224 runs above this process".to_owned(),
        },
        now,
    );

    assert!(chair.looks.signing.is_empty());
    assert!(note(&chair).is_some_and(|(tone, _)| tone == Tone::Failed));
}

#[test]
fn a_sign_item_in_the_inbox_signs_its_epic() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.epics[1].stage = EpicStage::Unsigned;
    state.inbox.insert(
        0,
        crate::app::chair::InboxItem {
            kind: NextKind::Sign,
            title: "Sign epic #226".to_owned(),
            blocks: 2,
            how: "Read it, then sign it.".to_owned(),
            command: Some("zaf epic sign 226".to_owned()),
            link: None,
            number: Some(TicketId::from(226)),
            question: None,
            begins: None,
        },
    );
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    let now = start();

    press(&mut chair, &[Key::Enter], now);

    assert_eq!(chair.pending.len(), 1);
    assert!(chair.looks.signs(&root(), TicketId::from(226)));
}

#[test]
fn enter_on_an_epic_reads_it_on_the_gatherer_and_keeps_what_comes_back() {
    let mut chair = chair(SMALL);
    let now = start();
    chair.set_focus(PaneId::Epics);

    let commands = press(&mut chair, &[Key::Enter], now);

    let epic = TicketId::from(224);
    assert_eq!(commands, [Command::Look(0, Look::Epic(epic))]);
    assert_eq!(chair.looks.epic(epic), None, "reading");
    assert!(
        press(&mut chair, &[Key::Char('j')], now).is_empty(),
        "read once"
    );
    let read = |epic: u64| EpicRead {
        epic: TicketId::from(epic),
        body: Ok("The body.".to_owned()),
        release: None,
    };
    chair.looked(0, Looked::Epic(read(226)));
    assert_eq!(chair.looks.epic(epic), None, "not the epic asked for");
    chair.looked(1, Looked::Epic(read(224)));
    assert_eq!(chair.looks.epic(epic), None, "not this tab's");
    chair.looked(0, Looked::Epic(read(224)));
    assert_eq!(chair.looks.epic(epic), Some(&read(224)));

    press(&mut chair, &[Key::Esc], now);
    assert!(
        chair.looks.epic.is_none(),
        "left, it is read again next time"
    );
}

#[test]
fn live_follows_the_running_ticket_and_reads_it_again_with_each_read() {
    let mut chair = chair(SMALL);
    let ticket = TicketId::from(243);
    assert_eq!(chair.looks.ticket, Some((ticket, None)));

    let read = TicketRead {
        ticket,
        reports: Vec::new(),
        run: None,
    };
    chair.looked(0, Looked::Ticket(read.clone()));
    assert_eq!(chair.looks.ticket(ticket), Some(&read));

    let commands = chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));
    assert_eq!(commands, [Command::Look(0, Look::Ticket(ticket))]);
    assert_eq!(
        chair.looks.ticket(ticket),
        Some(&read),
        "the last read stays until the next comes"
    );

    chair.set_focus(PaneId::Epics);
    let commands = press(&mut chair, &[Key::Char('j'), Key::Char('l')], start());
    assert_eq!(chair.cursor.live, Some(TicketId::from(255)));
    assert_eq!(
        commands,
        [Command::Look(0, Look::Ticket(TicketId::from(255)))]
    );

    press(&mut chair, &[Key::Char('2')], start());
    assert_eq!((&chair.looks.epic, &chair.looks.ticket), (&None, &None));
}

#[test]
fn with_no_release_due_the_begin_offer_runs_nothing() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.inbox[2].begins = Some(ReleaseVersion::from("0.5.3"));
    state.inbox[2].command = Some("zaf begin --version 0.5.3".to_owned());
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    let now = start();
    assert_eq!(chair.badge(0), 2, "an offer waits on no one");

    chair.cursor.inbox = 2;
    press(&mut chair, &[Key::Enter], now);

    assert_eq!(
        note(&chair).map(|(_, words)| words),
        Some(
            "the phase pane runs zaf begin --version 0.5.3 once it comes (F8); nothing ran"
                .to_owned()
        )
    );
    assert!(chair.pending.is_empty());
}

#[test]
fn a_paste_into_the_answer_box_is_text_and_rules_nothing() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::CtrlU], now);

    // What once arrived as keys: every newline an Enter, then `q`.
    let commands = chair.paste(
        "Change nothing.\rand note: keep it small\r\nso be it\rq",
        now,
    );

    assert!(commands.is_empty(), "{commands:?}");
    assert!(chair.pending.is_empty(), "nothing is ruled");
    let Mode::Answer(answer) = &chair.mode else {
        panic!("the box stays open: {:?}", chair.mode);
    };
    assert_eq!(
        answer.text,
        "Change nothing.\nand note: keep it small\nso be it\nq"
    );
}

#[test]
fn a_paste_drops_control_characters_in_the_answer_box() {
    let mut chair = chair(SMALL);
    let now = start();
    press(&mut chair, &[Key::Enter, Key::CtrlU], now);

    chair.paste("Yes\x1b[2J\x07, keep it", now);

    let Mode::Answer(answer) = &chair.mode else {
        panic!("open");
    };
    assert_eq!(answer.text, "Yes[2J, keep it");
}

#[test]
fn a_paste_anywhere_else_does_nothing_and_says_so() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.epics[0].stage = EpicStage::Unsigned;
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    let now = start();

    let commands = chair.paste("s\rq\r", now);

    assert!(commands.is_empty(), "{commands:?}");
    assert!(chair.pending.is_empty(), "nothing signed or ruled");
    assert_eq!(chair.mode, Mode::Normal);
    assert_eq!(
        note(&chair),
        Some((
            Tone::Plain,
            "a paste goes only into the answer box or to Claude: nothing was done".to_owned()
        ))
    );
}

#[test]
fn a_fix_runs_in_the_tabs_own_project_whatever_its_read_says() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.root = std::path::PathBuf::from("/tmp/elsewhere");
    state.problems[0].fix = Some("cd /etc && zaf do ship".to_owned());
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair.set_focus(PaneId::Problems);

    let commands = press(&mut chair, &[Key::Enter], start());

    let [Command::Write(ChairAction::Fix(fix))] = commands.as_slice() else {
        panic!("a fix: {commands:?}");
    };
    assert_eq!(fix.root, root(), "the tab's root");
    assert_eq!(fix.args(), ["profile", "init"], "the verb, not the text");
}

#[test]
fn a_problem_with_no_verb_is_left_for_the_engineer() {
    let mut chair = chair(SMALL);
    let mut state = ways();
    state.problems[0].verb = None;
    state.problems[0].fix = Some("export FORGEJO_TOKEN=<token>".to_owned());
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair.set_focus(PaneId::Problems);

    assert!(press(&mut chair, &[Key::Enter], start()).is_empty());
    assert_eq!(
        note(&chair),
        Some((
            Tone::Plain,
            "run it by hand: export FORGEJO_TOKEN=<token>".to_owned()
        ))
    );
}

#[test]
fn a_second_fix_is_refused_while_one_runs_and_taken_once_it_ends() {
    let mut chair = chair(SMALL);
    chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));
    chair.set_focus(PaneId::Problems);
    let now = start();
    let commands = press(&mut chair, &[Key::Enter], now);
    let [Command::Write(action @ ChairAction::Fix(_))] = commands.as_slice() else {
        panic!("a fix: {commands:?}");
    };
    let action = action.clone();

    assert!(
        press(&mut chair, &[Key::Enter], now).is_empty(),
        "a second Enter starts nothing"
    );
    assert_eq!(
        note(&chair),
        Some((
            Tone::Plain,
            "zaf profile init still runs: nothing more starts until it ends".to_owned()
        ))
    );

    let killed = Outcome::failed(&action, "it ran past 60 s and was killed".to_owned());
    let _ = chair.written(&action, &killed, now);
    assert!(chair.fixing.is_none());
    assert!(
        matches!(
            press(&mut chair, &[Key::Enter], now).as_slice(),
            [Command::Write(ChairAction::Fix(_))]
        ),
        "once it has ended, Enter runs it again"
    );
}
