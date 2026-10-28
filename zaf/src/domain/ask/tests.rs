use strum::VariantArray;

use super::*;

/// The ask of the design page with neutral content, annotations included.
const ANNOTATED_ASK: &str = "\
from: team#3 / programmer otter     # who, for the log
for: #12                            # scopes the search: the task's named pages first
context: Members export their reading list; implementing the CSV writer   # two lines at most
questions:
  q1: rule  | Is an empty list exported as a header row only?
  q2: term  | What is a reading list?
  q3: exists| Is there a decision on which columns are exported?
";

const CANONICAL_ASK: &str = "\
from: team#3 / programmer otter
for: #12
context: Members export their reading list; implementing the CSV writer
questions:
  q1: rule  | Is an empty list exported as a header row only?
  q2: term  | What is a reading list?
  q3: exists| Is there a decision on which columns are exported?
";

fn parse(text: &str) -> Result<Ask, AskError> {
    text.parse::<Ask>()
}

#[test]
fn the_ask_format_parses_with_annotations() {
    let ask = parse(ANNOTATED_ASK).expect("the annotated ask parses");
    let expected_first = Question {
        id: QuestionId::from(1),
        kind: AskKind::Rule,
        text: "Is an empty list exported as a header row only?".to_owned(),
    };
    assert_eq!(ask.from, Asker::from("team#3 / programmer otter"));
    assert_eq!(ask.for_task, IssueNumber::from(12));
    assert_eq!(
        ask.context,
        ["Members export their reading list; implementing the CSV writer"]
    );
    assert_eq!(ask.questions.len(), 3);
    assert_eq!(ask.questions[0], expected_first);
    assert_eq!(ask.questions[2].kind, AskKind::Exists);
}

#[test]
fn the_ask_renders_canonically_and_round_trips() {
    let ask = parse(ANNOTATED_ASK).expect("the annotated ask parses");
    let rendered = ask.to_string();
    assert_eq!(rendered, CANONICAL_ASK);
    assert_eq!(parse(&rendered), Ok(ask));
}

#[test]
fn context_takes_a_second_line_but_not_a_third() {
    let two_lines =
        CANONICAL_ASK.replace("CSV writer\n", "CSV writer\n         and its header row\n");
    let ask = parse(&two_lines).expect("two context lines parse");
    assert_eq!(ask.context.len(), 2);
    assert_eq!(parse(&ask.to_string()), Ok(ask));
    let three_lines = two_lines.replace("header row\n", "header row\n         and a third line\n");
    assert_eq!(parse(&three_lines), Err(AskError::ContextTooLong));
}

#[test]
fn questions_are_bounded() {
    let six = (1..=6)
        .map(|number| format!("  q{number}: fact | Question {number}?\n"))
        .collect::<String>();
    let too_many = format!("from: a\nfor: #1\ncontext: c\nquestions:\n{six}");
    assert_eq!(parse(&too_many), Err(AskError::QuestionCount(6)));
    let none = "from: a\nfor: #1\ncontext: c\nquestions:\n";
    assert_eq!(parse(none), Err(AskError::QuestionCount(0)));
    let long_question = format!("  q1: fact | {}?\n", vec!["word"; 30].join(" "));
    let too_long = format!("from: a\nfor: #1\ncontext: c\nquestions:\n{long_question}");
    let too_long_error = AskError::QuestionTooLong {
        id: QuestionId::from(1),
        words: 30,
    };
    assert_eq!(parse(&too_long), Err(too_long_error));
}

#[test]
fn bad_lines_are_rejected() {
    let unknown_kind = CANONICAL_ASK.replace("rule  |", "guess |");
    assert_eq!(
        parse(&unknown_kind),
        Err(AskError::Kind("guess".to_owned()))
    );
    let duplicate = CANONICAL_ASK.replace("q2:", "q1:");
    assert_eq!(
        parse(&duplicate),
        Err(AskError::DuplicateQuestion(QuestionId::from(1)))
    );
    let bad_task = CANONICAL_ASK.replace("for: #12", "for: twelve");
    assert_eq!(parse(&bad_task), Err(AskError::Task("twelve".to_owned())));
    let missing_from = CANONICAL_ASK.replace("from: ", "by: ");
    assert!(matches!(
        parse(&missing_from),
        Err(AskError::Expected { line: 1, .. })
    ));
    assert_eq!(
        "q0".parse::<QuestionId>(),
        Err(AskError::QuestionId("q0".to_owned()))
    );
}

#[test]
fn kind_tokens_are_pinned() {
    let tokens: Vec<String> = AskKind::VARIANTS.iter().map(ToString::to_string).collect();
    assert_eq!(tokens, ["fact", "rule", "term", "exists", "where"]);
}
