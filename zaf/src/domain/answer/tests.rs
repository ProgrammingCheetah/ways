use std::collections::BTreeMap;

use super::*;

/// The answer of the design page with neutral content, in canonical form.
const ANSWER_TEXT: &str = "\
q1: settled
    answer: An empty list exports the header row and nothing else.
    pages: 1001 (decided)
    quote: \"an empty list exports the header row only\"
q2: settled
    answer: A list a member keeps of books to read; it belongs to one account.
    pages: 1001 (decided), 1002 (current)
    quote: \"a reading list belongs to exactly one account\"
q3: not settled
    next: question ticket on #12 (door: no)
q4: conflict
    pages: 1001 (decided), 1002 (current)
    next: question ticket on #12 (door: yes)
";

fn page(id: &str) -> PageId {
    id.parse().expect("a valid page id")
}

fn library() -> BTreeMap<PageId, String> {
    let mut pages = BTreeMap::new();
    pages.insert(page("1001"), "## Export\n\nOn export, an empty list exports the header\n   row only. Columns follow the list.".to_owned());
    pages.insert(
        page("1002"),
        "Lists are private, and a reading list belongs to exactly\n\tone account.".to_owned(),
    );
    pages
}

fn parse(text: &str) -> Result<Answer, AnswerError> {
    text.parse::<Answer>()
}

#[test]
fn the_answer_format_parses() {
    let answer = parse(ANSWER_TEXT).expect("the answer parses");
    let expected_first = Verdict::Settled {
        answer: "An empty list exports the header row and nothing else.".to_owned(),
        pages: vec![Citation {
            page: page("1001"),
            status: PageStatus::Decided,
        }],
        quotes: vec![Quote::from("an empty list exports the header row only")],
    };
    let expected_third = Verdict::NotSettled {
        next: "question ticket on #12 (door: no)".to_owned(),
    };
    assert_eq!(answer.questions.len(), 4);
    assert_eq!(answer.questions[0].id, QuestionId::from(1));
    assert_eq!(answer.questions[0].verdict, expected_first);
    assert_eq!(answer.questions[2].verdict, expected_third);
    assert!(
        matches!(&answer.questions[3].verdict, Verdict::Conflict { pages, .. } if pages.len() == 2)
    );
}

#[test]
fn the_answer_round_trips() {
    let answer = parse(ANSWER_TEXT).expect("the answer parses");
    let rendered = answer.to_string();
    assert_eq!(rendered, ANSWER_TEXT);
    assert_eq!(parse(&rendered), Ok(answer));
}

#[test]
fn quote_check_accepts_verbatim_and_whitespace_different_quotes() {
    let pages = library();
    let answer = parse(ANSWER_TEXT).expect("the answer parses");
    assert_eq!(answer.check_quotes(|id| pages.get(id).cloned()), Ok(()));
}

#[test]
fn quote_check_rejects_a_changed_word() {
    let pages = library();
    let changed = ANSWER_TEXT.replace("exports the header row only", "exports the title row only");
    let answer = parse(&changed).expect("the changed answer parses");
    let quote_not_found_error = QuoteCheckError::QuoteNotFound {
        question: QuestionId::from(1),
        quote: Quote::from("an empty list exports the title row only"),
    };
    assert_eq!(
        answer.check_quotes(|id| pages.get(id).cloned()),
        Err(quote_not_found_error)
    );
}

#[test]
fn quote_check_reports_an_unreadable_page() {
    let answer = parse(ANSWER_TEXT).expect("the answer parses");
    let page_unavailable_error = QuoteCheckError::PageUnavailable {
        question: QuestionId::from(1),
        page: page("1001"),
    };
    assert_eq!(answer.check_quotes(|_| None), Err(page_unavailable_error));
}

#[test]
fn a_superseded_page_cannot_be_cited() {
    let superseded = ANSWER_TEXT.replacen("1001 (decided)", "1001 (superseded)", 1);
    let uncitable_error = AnswerError::UncitablePage {
        id: QuestionId::from(1),
        page: page("1001"),
        status: PageStatus::Superseded,
    };
    assert_eq!(parse(&superseded), Err(uncitable_error));
}

#[test]
fn verdict_rules_are_enforced() {
    let conflict_one_page = "q1: conflict\n    pages: 1001 (decided)\n    next: a ticket\n";
    assert_eq!(
        parse(conflict_one_page),
        Err(AnswerError::ConflictNeedsTwoPages(QuestionId::from(1)))
    );
    let not_settled_with_answer = "q1: not settled\n    answer: maybe\n    next: a ticket\n";
    let unexpected_answer_error = AnswerError::UnexpectedKey {
        id: QuestionId::from(1),
        key: "answer",
    };
    assert_eq!(parse(not_settled_with_answer), Err(unexpected_answer_error));
    let settled_without_quote = "q1: settled\n    answer: yes\n    pages: 1001 (decided)\n";
    let missing_quote_error = AnswerError::MissingKey {
        id: QuestionId::from(1),
        key: "quote",
    };
    assert_eq!(parse(settled_without_quote), Err(missing_quote_error));
    let long_answer = format!(
        "q1: settled\n    answer: {}\n    pages: 1001 (decided)\n    quote: \"x\"\n",
        vec!["word"; 61].join(" ")
    );
    let too_long_error = AnswerError::AnswerTooLong {
        id: QuestionId::from(1),
        words: 61,
    };
    assert_eq!(parse(&long_answer), Err(too_long_error));
}

#[test]
fn malformed_lines_are_rejected() {
    assert_eq!(parse(""), Err(AnswerError::Empty));
    let unknown_verdict = "q1: probably\n";
    assert!(matches!(
        parse(unknown_verdict),
        Err(AnswerError::Verdict { line: 1, .. })
    ));
    let unknown_key = "q1: not settled\n    hint: x\n";
    assert!(matches!(
        parse(unknown_key),
        Err(AnswerError::UnknownKey { line: 2, .. })
    ));
    let unquoted = "q1: settled\n    answer: a\n    pages: 1001 (decided)\n    quote: no quotes\n";
    assert!(matches!(
        parse(unquoted),
        Err(AnswerError::Quote { line: 4, .. })
    ));
    let orphan = "    next: x\n";
    assert!(matches!(
        parse(orphan),
        Err(AnswerError::Expected { line: 1, .. })
    ));
    let twice = "q1: not settled\n    next: a\nq1: not settled\n    next: b\n";
    assert_eq!(
        parse(twice),
        Err(AnswerError::DuplicateQuestion(QuestionId::from(1)))
    );
}
