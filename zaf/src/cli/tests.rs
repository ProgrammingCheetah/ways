use clap::{CommandFactory, Parser};

use super::*;

#[test]
fn the_command_line_is_well_formed() {
    Cli::command().debug_assert();
}

#[test]
fn every_verb_is_listed() {
    let command = Cli::command();
    let verbs: Vec<&str> = command
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(
        verbs,
        [
            "board", "ask", "brief", "check", "changes", "verify", "slice", "spawn", "asklog",
            "status", "report", "guard", "init", "doctor", "setup", "profile", "do", "hub",
            "progress", "update", "epic",
        ]
    );
    let board = command.find_subcommand("board").expect("board");
    let board_actions: Vec<&str> = board
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(
        board_actions,
        [
            "ready", "claim", "report", "handoff", "question", "ruling", "done", "file"
        ]
    );
    let slice = command.find_subcommand("slice").expect("slice");
    let slice_actions: Vec<&str> = slice
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(slice_actions, ["open", "pr", "merge", "drop"]);
}

/// The verbs that are built: each exits by its own rules, so the sweep below skips them.
/// A verb joins this list in the same change that builds it.
const BUILT: &[&str] = &[
    "ask",
    "asklog",
    "board ready",
    "board claim",
    "board report",
    "board handoff",
    "board question",
    "board ruling",
    "board done",
    "brief",
    "changes",
    "check",
    "doctor",
    "guard",
    "init",
    "profile init",
    "report",
    "setup",
    "slice open",
    "slice pr",
    "slice merge",
    "slice drop",
    "spawn",
    "status",
    "verify",
];

#[test]
fn every_unbuilt_verb_exits_not_implemented() {
    let invocations: [&[&str]; 24] = [
        &["board", "ready"],
        &["board", "done", "2"],
        &["board", "claim", "#2"],
        &["board", "report", "2", "working"],
        &["board", "question", "2", "Which columns?", "--door"],
        &["board", "ruling", "7", "Title and author."],
        &["ask", "-t", "2", "rule: Is an empty list exported?"],
        &["brief", "2"],
        &["check", "2", "--status"],
        &["changes", "2", "--path", "src/export.rs"],
        &["verify", "2"],
        &["slice", "open", "2"],
        &["slice", "pr", "2"],
        &["slice", "merge", "2"],
        &["slice", "drop", "2"],
        &["spawn", "--once"],
        &["asklog"],
        &["status", "--json"],
        &["report", "--more"],
        &["guard"],
        &["init", "--from-teams"],
        &["doctor"],
        &["setup"],
        &["profile", "init"],
    ];
    for invocation in invocations {
        let args = std::iter::once("zaf").chain(invocation.iter().copied());
        let cli = <Cli as clap::Parser>::try_parse_from(args.clone()).expect("parses");
        if BUILT.contains(&cli.verb.path().as_str()) {
            continue;
        }
        assert_eq!(run(args), NOT_IMPLEMENTED, "zaf {}", invocation.join(" "));
    }
}

/// What `zaf board done` exits with for a ready task whose recorded pull request is at `pr`,
/// the forge failing the merged read when `forge_down`.
fn board_done_exit(pr: &str, forge_down: bool) -> u8 {
    use crate::app::actions::SliceRecords;
    use crate::domain::Ticket;
    use crate::domain::{TicketId, TicketState};
    use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
    use crate::ports::{BaseName, Board, SliceName};

    let dir = tempfile::tempdir().expect("tempdir");
    let task = TicketId::from(319);
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        id: task,
        epic: None,
        title: "Task".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
    });
    let forge = FakeForge::default();
    let request = PrRequest {
        slice: SliceName::from("feature/0.2.0-103-319"),
        base: BaseName::from("feature/0.2.0"),
        title: "Task".to_owned(),
        body: String::new(),
    };
    forge.open_pr(&request).expect("open pr");
    if forge_down {
        forge.fail_merged_reads();
    }
    let slices = dir.path().join(".zaf/slices");
    std::fs::create_dir_all(&slices).expect("mkdir");
    std::fs::write(slices.join("319.pr"), pr).expect("write");
    let records = SliceRecords::new(dir.path());
    let outcome = crate::app::board::done(&board, &forge, &FakeVcs::default(), &records, task);
    let error = outcome.expect_err("no task is done here");
    let ready = board.task(task).expect("task").state;
    assert_eq!(ready, Some(TicketState::Ready), "nothing changed");
    board::exit_code(&error)
}

#[test]
fn board_done_forge_failure_exits_2() {
    // The merged read fails.
    assert_eq!(board_done_exit("1", true), 2);
    // The phase read fails: the forge does not know the recorded pull request.
    assert_eq!(board_done_exit("404", false), 2);
    // An open pull request is a refusal, not a failure.
    assert_eq!(board_done_exit("1", false), 1);
}

#[test]
fn board_handoff_exits_0_written_1_without_a_pr_and_2_when_the_forge_fails() {
    use crate::app::actions::SliceRecords;
    use crate::domain::TicketId;
    use crate::fakes::{FakeForge, PrRequest};
    use crate::ports::{BaseName, SliceName};

    let task = TicketId::from(433);
    let forge = FakeForge::default();
    let request = PrRequest {
        slice: SliceName::from("feature/0.3.0-421-433"),
        base: BaseName::from("feature/0.3.0"),
        title: "Task".to_owned(),
        body: String::new(),
    };
    let pr = forge.open_pr(&request).expect("open pr");
    let dir = tempfile::tempdir().expect("tempdir");
    let records = SliceRecords::new(dir.path());
    let slices = dir.path().join(".zaf/slices");
    std::fs::create_dir_all(&slices).expect("mkdir");
    let exit = |forge: &FakeForge| {
        let result = crate::app::board::handoff(forge, &records, task, "text");
        result.map_or_else(|error| board::exit_code(&error), |_| 0)
    };
    assert_eq!(exit(&forge), 1, "no recorded pull request");
    std::fs::write(slices.join("433.pr"), pr.get().to_string()).expect("write");
    assert_eq!(exit(&forge), 0);
    forge.fail_handoffs();
    assert_eq!(exit(&forge), 2);
}

#[test]
fn verb_paths_name_the_action() {
    let cli = Cli::try_parse_from(["zaf", "board", "claim", "2"]).expect("parses");
    assert_eq!(cli.verb.path(), "board claim");
    let cli = Cli::try_parse_from(["zaf", "asklog"]).expect("parses");
    assert_eq!(cli.verb.path(), "asklog");
}

#[test]
fn usage_errors_and_help_keep_clap_codes() {
    assert_eq!(run(["zaf", "--help"]), 0);
    assert_eq!(run(["zaf", "fly"]), 2);
    assert_eq!(run(["zaf", "brief", "not-a-number"]), 2);
    let six_questions = ["zaf", "ask", "-t", "2", "a", "b", "c", "d", "e", "f"];
    assert_eq!(run(six_questions), 2);
}
