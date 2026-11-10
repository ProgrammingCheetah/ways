use clap::Parser;

use super::*;
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
