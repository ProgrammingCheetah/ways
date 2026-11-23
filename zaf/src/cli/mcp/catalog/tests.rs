use clap::CommandFactory;

use super::*;
use crate::cli::Cli;
use crate::domain::ToolCall;

fn specs() -> Vec<ToolSpec> {
    Catalog::from_command(&Cli::command())
}

fn board() -> ToolSpec {
    specs()
        .into_iter()
        .find(|spec| spec.name == "board")
        .expect("board is a visible verb")
}

fn call(args: &[&str]) -> ToolCall {
    ToolCall {
        tool: "board".into(),
        args: args.iter().map(|word| (*word).to_owned()).collect(),
        ..ToolCall::default()
    }
}

fn strings(line: &[std::ffi::OsString]) -> Vec<String> {
    line.iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn names_are_the_visible_verbs() {
    let mut command = Cli::command();
    command.build();
    let expected: Vec<String> = command
        .get_subcommands()
        .filter(|verb| !verb.is_hide_set() && verb.get_name() != "help")
        .map(|verb| verb.get_name().to_owned())
        .collect();
    let names: Vec<String> = specs().into_iter().map(|spec| spec.name).collect();
    assert!(!names.is_empty());
    assert_eq!(names, expected);
}

#[test]
fn board_claim_line_parses() {
    let line = strings(&board().command_line(&call(&["claim", "12"])));
    assert_eq!(line, ["zaf", "board", "claim", "12"]);
    let catalog = Catalog::new(Cli::command());
    assert_eq!(
        catalog.check(&line),
        Ok(crate::domain::CommandPath::new(["board", "claim"]))
    );
    let matches = Cli::command().try_get_matches_from(&line).expect("parses");
    let (verb, sub) = matches.subcommand().expect("a verb");
    assert_eq!(verb, "board");
    let (action, claim) = sub.subcommand().expect("an action");
    assert_eq!(action, "claim");
    assert_eq!(
        claim
            .get_one::<crate::domain::TicketId>("task")
            .map(ToString::to_string),
        Some("#12".into())
    );
}

#[test]
fn global_flags_lead() {
    let mut call = call(&["ready"]);
    call.json = true;
    call.more = true;
    let line = strings(&board().command_line(&call));
    assert_eq!(line, ["zaf", "--json", "--more", "board", "ready"]);
    assert_eq!(
        Catalog::new(Cli::command()).check(&line),
        Ok(crate::domain::CommandPath::new(["board", "ready"]))
    );
}

#[test]
fn the_checked_path_names_the_action_of_do_and_the_flags() {
    let check = |words: &[&str]| {
        let argv: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        Catalog::new(Cli::command()).check(&argv).expect("parses")
    };
    let land = check(&["zaf", "do", "land", "7"]);
    assert_eq!(land.words, ["do", "land"]);
    assert_eq!(check(&["zaf", "epic", "sign", "1"]).words, ["epic", "sign"]);
    assert!(check(&["zaf", "spawn", "--watch"]).watch);
}

#[test]
fn usage_error_is_clap_message() {
    let argv: Vec<String> = ["zaf", "board", "frobnicate"].map(str::to_owned).to_vec();
    let expected = Cli::command()
        .try_get_matches_from(&argv)
        .expect_err("clap refuses it")
        .to_string();
    let message = Catalog::new(Cli::command())
        .check(&argv)
        .expect_err("the checker refuses it");
    assert_eq!(message, expected);
    assert!(message.contains("frobnicate"), "{message}");
}

#[test]
fn input_schema_fields() {
    for spec in specs() {
        let schema = &spec.input_schema;
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(properties["args"]["type"], "array", "{}", spec.name);
        assert_eq!(
            properties["args"]["items"]["type"], "string",
            "{}",
            spec.name
        );
        assert_eq!(properties["stdin"]["type"], "string", "{}", spec.name);
        assert_eq!(properties["json"]["type"], "boolean", "{}", spec.name);
        assert_eq!(properties["more"]["type"], "boolean", "{}", spec.name);
        let required = schema["required"].as_array().expect("required");
        assert!(required.is_empty(), "{} requires {required:?}", spec.name);
    }
}

#[test]
fn description_has_help_then_usage_lines() {
    let board = board();
    assert!(board.description.starts_with("Read and write the board"));
    assert!(
        board.description.contains("Usage: zaf board claim"),
        "{}",
        board.description
    );
}
