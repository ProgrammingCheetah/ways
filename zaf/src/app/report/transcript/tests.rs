use super::*;

fn parse(lines: &[&str]) -> Option<Run> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.jsonl");
    std::fs::write(&path, lines.join("\n")).expect("write");
    parse_run(&path, &PriceTable::default())
}

fn turn(id: &str, model: &str, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":1,"output_tokens":{output}}}}}}}"#
    )
}

#[test]
fn a_file_without_a_turn_is_no_run() {
    assert!(parse(&[r#"{"type":"user","message":{"content":"hi"}}"#]).is_none());
    assert!(parse(&[]).is_none());
}

#[test]
fn a_message_without_usage_or_model_is_not_a_turn() {
    let run = parse(&[
        r#"{"type":"assistant","message":{"id":"a","model":"claude-opus-5-5","usage":{}}}"#,
        r#"{"type":"assistant","message":{"id":"b","usage":{"input_tokens":1}}}"#,
        &turn("c", "claude-opus-5-5", 7),
    ])
    .expect("one turn");
    assert_eq!((run.turns, run.output), (1, 7));
}

#[test]
fn the_run_model_is_the_most_common_and_the_first_wins_a_tie() {
    let run = parse(&[
        &turn("1", "claude-haiku-5-5", 1),
        &turn("2", "claude-sonnet-5-5", 1),
    ])
    .expect("run");
    assert_eq!(run.model, "claude-haiku-5-5");
    let run = parse(&[
        &turn("1", "claude-haiku-5-5", 1),
        &turn("2", "claude-sonnet-5-5", 1),
        &turn("3", "claude-sonnet-5-5", 1),
    ])
    .expect("run");
    assert_eq!(run.model, "claude-sonnet-5-5");
}

#[test]
fn the_prompt_skips_command_echoes_and_collapses_whitespace() {
    let run = parse(&[
        r#"{"type":"user","message":{"content":"<command-name>/x</command-name>"}}"#,
        r#"{"type":"user","message":{"content":"  one \n two\t three "}}"#,
        r#"{"type":"user","message":{"content":"second"}}"#,
        &turn("1", "claude-opus-5-5", 1),
    ])
    .expect("run");
    assert_eq!(run.first_prompt, " one two three ");
}

#[test]
fn a_long_prompt_is_cut_at_120_characters() {
    let long = "é".repeat(200);
    let line = format!(r#"{{"type":"user","message":{{"content":"{long}"}}}}"#);
    let run = parse(&[&line, &turn("1", "claude-opus-5-5", 1)]).expect("run");
    assert_eq!(run.first_prompt.chars().count(), 120);
}

#[test]
fn an_unknown_model_costs_the_fallback_rate() {
    let run = parse(&[&turn("1", "claude-future-9", 1_000_000)]).expect("run");
    assert!((run.cost - 20.000_004).abs() < 1e-9, "{}", run.cost);
}
