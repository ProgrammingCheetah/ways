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
            "status", "report", "guard", "init", "doctor", "setup", "profile",
        ]
    );
    let board = command.find_subcommand("board").expect("board");
    let board_actions: Vec<&str> = board
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(
        board_actions,
        ["ready", "claim", "report", "question", "ruling"]
    );
    let slice = command.find_subcommand("slice").expect("slice");
    let slice_actions: Vec<&str> = slice
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect();
    assert_eq!(slice_actions, ["open", "pr", "merge", "drop"]);
}

#[test]
fn every_verb_exits_not_implemented() {
    let invocations: [&[&str]; 23] = [
        &["board", "ready"],
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
        assert_eq!(run(args), NOT_IMPLEMENTED, "zaf {}", invocation.join(" "));
    }
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
