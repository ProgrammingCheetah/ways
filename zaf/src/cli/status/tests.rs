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
