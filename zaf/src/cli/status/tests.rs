use clap::Parser;

use super::*;
use crate::cli::screen;
use crate::domain::screen::Meaning;
use crate::domain::{Ticket, TicketId, TicketState};
use crate::fakes::FakeBoard;

const MANIFEST: &str = "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n[spawn]\nslots = 2\n";

fn project() -> (tempfile::TempDir, Composition) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ZAF.toml"), MANIFEST).expect("manifest");
    let composition = Composition::load_with_home(dir.path(), None).expect("loads");
    (dir, composition)
}

fn cli(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("zaf").chain(args.iter().copied())).expect("parses")
}

#[test]
fn the_screen_exits_ok_over_a_fake_board_and_a_scripted_spawn_state() {
    let (dir, composition) = project();
    std::fs::create_dir_all(dir.path().join(".zaf")).expect("state dir");
    std::fs::write(
        dir.path().join(".zaf/spawn.state"),
        r#"{"launches":[{"task":3,"instance":"programmer instance 1 on #2"}],"questions":[9]}"#,
    )
    .expect("spawn.state");
    let board = FakeBoard::default();
    for (number, label) in [(3, TicketState::Ready), (9, TicketState::Question)] {
        board.add_ticket(Ticket {
            id: TicketId::from(number),
            epic: None,
            title: format!("ticket {number}").into(),
            body: "".into(),
            state: Some(label),
            url: "".into(),
            kind: None,
        });
    }
    assert_eq!(print_status(&cli(&["status"]), &composition, &board), OK);
    assert_eq!(
        print_status(&cli(&["--json", "status"]), &composition, &board),
        OK
    );
}

#[test]
fn the_screen_lists_a_live_instance_and_none_of_its_other_variables() {
    let (dir, composition) = project();
    let run = dir.path().join("elsewhere/.zaf/runs/team-instance-1-on-2");
    let mut stub = std::process::Command::new("sleep")
        .arg("30")
        .env("ZAF_RUN", &run)
        .env("ZAF_INSTANCE", "team instance 1 on #2")
        .env_remove("ZAF_SIZE")
        .env("GH_TOKEN", "ghp_not_a_real_token_413")
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("runs sleep");

    let read = screen(&composition, &FakeBoard::default());
    let _ = stub.kill();
    let _ = stub.wait();

    let screen = read.expect("status");
    let json = screen.to_json();
    let text = screen.render_text();
    let root = dir.path().join("elsewhere");
    assert!(
        screen.machine.instances.iter().any(|line| line.root == root
            && line.instance.as_deref() == Some("team instance 1 on #2")
            && line.size.is_none()),
        "{json}"
    );
    assert!(text.contains("machine: "), "{text}");
    for output in [&json, &text] {
        assert!(!output.contains("ghp_not_a_real_token_413"), "{output}");
        assert!(!output.contains("GH_TOKEN"), "{output}");
    }
}

#[test]
fn a_garbled_spawn_state_exits_backend() {
    let (dir, composition) = project();
    std::fs::create_dir_all(dir.path().join(".zaf")).expect("state dir");
    std::fs::write(dir.path().join(".zaf/spawn.state"), "{").expect("spawn.state");
    let board = FakeBoard::default();
    assert_eq!(
        print_status(&cli(&["status"]), &composition, &board),
        BACKEND
    );
}

const PHASED: &str = "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n[forge]\nkind = \"github\"\nrepo = \"owner/demo\"\n[phases]\nTask = [\"implement\"]\n[spawn]\nslots = 2\n";

/// A project whose manifest has `[phases]` and a forge, so the screen places its tickets too.
fn phased_project() -> (tempfile::TempDir, Composition) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".zaf.toml"), PHASED).expect("manifest");
    // The phase's action is the project's own, so the screen places the tickets the same way
    // whatever actions the machine's zaf home holds, and with none at all.
    let actions = dir.path().join(".zaf/actions");
    std::fs::create_dir_all(&actions).expect("actions dir");
    std::fs::write(actions.join("implement"), IMPLEMENT).expect("action");
    std::fs::set_permissions(
        actions.join("implement"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("mode");
    let composition = Composition::load_with_home(dir.path(), None).expect("loads");
    (dir, composition)
}

/// The `implement` phase action of the phased project: a model phase that ends at its pull request.
const IMPLEMENT: &str = "#!/bin/sh\n# zaf description: phase implement\n# zaf 1: refused\n# zaf 2: a backend failed\n# zaf mode: 755\n# zaf target: task\n# zaf kind: model\n# zaf ends: pr-open\nexit 0\n";

/// A board with a signed epic, a ready task under it and a question open on the task.
fn board_with_a_question() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(100), "The epic");
    board.add_ticket(Ticket {
        id: TicketId::from(101),
        epic: Some(TicketId::from(100)),
        title: "the task".into(),
        body: concat!(
            "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n",
            "Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n",
            "Model: sonnet\nSize: S\nBlocked by: —\n",
        )
        .into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    board
        .open_question(&crate::ports::NewQuestion {
            on: TicketId::from(101),
            text: "why?".to_owned(),
            door: false,
        })
        .expect("opens");
    board.reset_calls();
    board
}

#[test]
fn status_reads_epics_once() {
    for (_dir, composition) in [project(), phased_project()] {
        let board = board_with_a_question();
        screen(&composition, &board).expect("status");
        assert_eq!(board.calls("epics"), 1);
    }
}

#[test]
fn status_reads_questions_once() {
    for (_dir, composition) in [project(), phased_project()] {
        let board = board_with_a_question();
        screen(&composition, &board).expect("status");
        assert_eq!(board.calls("questions"), 1);
    }
}

#[test]
fn status_output_unchanged() {
    let (_dir, composition) = phased_project();
    let board = board_with_a_question();
    let screen = screen(&composition, &board).expect("status");
    let text = screen.render_text();
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with("machine:"))
        .collect();
    assert_eq!(
        kept.join("\n"),
        "questions 1 (#102)\n\
         question  #102 why?\n\
         running   0\n\
         ready     0\n\
         waiting   1 (#101)\n\
         claimed   0\n\
         epic      #100 challenged 0 of 1  The epic\n\
         slots     2 free of 2\n\
         budget: no machine cap\n\
         guard     not computed (run `zaf guard`)"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    assert_eq!(
        json["epics"].to_string(),
        r#"[{"done":0,"number":100,"signature_void":false,"stage":"challenged","title":"The epic","total":1}]"#
    );
    assert_eq!(
        json["questions"].to_string(),
        r#"[{"epic":null,"number":102,"state":"question","title":"why?","type":null,"url":"https://board.invalid/tickets/102"}]"#
    );
    assert_eq!(json["lane"].to_string(), "[]");
}

const ESC: char = '\x1b';

fn coloured() -> Screen {
    Screen {
        colour: true,
        live: true,
        rows: 24,
        columns: 80,
    }
}

fn plain() -> Screen {
    Screen {
        colour: false,
        live: false,
        rows: 24,
        columns: 80,
    }
}

/// A status with a ready task, a question whose title carries escapes, an orphaned claim and a
/// due release.
fn busy_status() -> Status {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    let ticket = |number: u64, title: &str, state: TicketState| Ticket {
        id: TicketId::from(number),
        epic: None,
        title: title.into(),
        body: "".into(),
        state: Some(state),
        url: "".into(),
        kind: None,
    };
    board.add_ticket(ticket(3, "Export rows", TicketState::Ready));
    board.add_question(ticket(
        9,
        "Which columns\x1b]52;c;ZXZpbA==\x07 are exported?",
        TicketState::Question,
    ));
    let mut status = status::status_listing(&board, dir.path(), 2, &|_| false).expect("status");
    status.orphaned.push(status::ClaimLine {
        task: 5,
        instance: "team instance 1 on #2".to_owned(),
    });
    status.releases.push(status::ReleaseLine {
        version: "0.5.3".to_owned(),
        phase: "design".to_owned(),
        command: "zaf begin --version 0.5.3".to_owned(),
        way_on: crate::app::release::WAY_ON.to_owned(),
    });
    status
}

#[test]
fn the_coloured_status_stripped_is_the_plain_status() {
    let status = busy_status();
    for more in [false, true] {
        let painted = draw(&coloured(), &status, more);
        assert!(painted.contains(ESC), "{painted:?}");
        assert_eq!(
            screen::strip_escapes(&painted),
            draw(&plain(), &status, more),
            "more = {more}"
        );
        assert_eq!(draw(&plain(), &status, more), status.render(more));
    }
}

#[test]
fn a_question_is_lavender_and_an_orphan_red() {
    let painted = draw(&coloured(), &busy_status(), false);
    let line_with = |word: &str| {
        painted
            .lines()
            .find(|line| line.contains(word))
            .unwrap_or_else(|| panic!("no {word} line in {painted:?}"))
    };
    let waiting = screen::code(Meaning::Waiting);
    let failed = screen::code(Meaning::Failed);
    assert!(
        line_with("question  ").contains(&format!("{ESC}[{waiting}mquestion  ")),
        "{painted:?}"
    );
    assert!(
        line_with("orphaned").contains(&format!("{ESC}[{failed}morphaned  ")),
        "{painted:?}"
    );
}

#[test]
fn board_text_cannot_carry_an_escape_into_the_painted_status() {
    let painted = draw(&coloured(), &busy_status(), true);
    let stripped = screen::strip_escapes(&painted);
    assert!(!stripped.contains(ESC), "{stripped:?}");
    assert!(!painted.contains('\x07'), "{painted:?}");
}

#[test]
fn without_colour_the_status_holds_no_escape_byte() {
    let status = busy_status();
    for more in [false, true] {
        assert!(!draw(&plain(), &status, more).contains(ESC));
    }
}

#[test]
fn a_red_guard_reads_as_failed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let red = crate::app::guard::assess(
        Some(100.0),
        99.0,
        &[70, 85, 95],
        std::time::SystemTime::UNIX_EPOCH,
    );
    crate::app::guard::write(dir.path(), &red).expect("guard.json");
    let status =
        status::status_listing(&FakeBoard::default(), dir.path(), 1, &|_| false).expect("status");
    let painted = draw(&coloured(), &status, false);
    let failed = screen::code(Meaning::Failed);
    let guard_line = painted
        .lines()
        .find(|line| line.contains("guard"))
        .expect("guard line");
    assert!(
        guard_line.contains(&format!("{ESC}[{failed}mred")),
        "{guard_line:?}"
    );
}
