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
            "status", "report", "guard", "init", "doctor", "setup", "profile", "do",
        ]
    );
    let board = command.find_subcommand("board").expect("board");
    let board_actions: Vec<&str> = board
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(
        board_actions,
        ["ready", "claim", "report", "question", "ruling", "done"]
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
    use crate::domain::{IssueNumber, TaskLabel};
    use crate::fakes::{FakeBoard, FakeForge, FakeVcs};
    use crate::ports::{BaseName, Board, Forge, PrRequest, SliceName, Ticket};

    let dir = tempfile::tempdir().expect("tempdir");
    let task = IssueNumber::from(319);
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        number: task,
        epic: None,
        title: "Task".to_owned(),
        body: String::new(),
        label: Some(TaskLabel::Ready),
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
    let ready = board.task(task).expect("task").label;
    assert_eq!(ready, Some(TaskLabel::Ready), "nothing changed");
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
