use super::*;

/// The ruling on question 30 as the board received it, the backticked words eaten.
const EATEN: &str = "I agree on everything, except the name.  makes the rule seem like a living entity. Make it  instead.";

fn gap(text: &str) -> RulingGap {
    first_gap(text).expect("a gap")
}

#[test]
fn a_gap_in_the_middle_of_a_line_is_found_at_its_first_space() {
    let found = gap("Make it  instead.");
    assert_eq!((found.line, found.column), (1, 8));
    assert_eq!(found.excerpt, "Make it  instead.");
    assert_eq!(
        check_typed_ruling("Make it  instead.", false),
        Err(TypedRulingError::Gap(found))
    );
}

#[test]
fn the_ruling_a_shell_ate_is_refused_at_its_first_gap() {
    let found = gap(EATEN);
    assert_eq!((found.line, found.column), (1, 40));
    assert_eq!(found.excerpt, "…except the name.  makes the rule s…");
    let refusal = check_typed_ruling(EATEN, false).expect_err("refused");
    assert_eq!(
        refusal.to_string(),
        "the answer has a gap at column 40 (\"…except the name.  makes the rule s…\"): \
         a shell eats text in backticks and after $ inside double quotes; nothing was written"
    );
    assert_eq!(
        refusal.way_on(),
        Some("quote the answer in single quotes, or pass --as-written to send it exactly as it is")
    );
}

#[test]
fn a_gap_after_a_full_stop_counts() {
    assert_eq!(gap("Title only.  Author later.").column, 12);
}

#[test]
fn single_spaces_are_no_gap() {
    assert_eq!(first_gap("Title and author."), None);
    assert_eq!(check_typed_ruling("Title and author.", false), Ok(()));
}

#[test]
fn leading_indentation_and_trailing_spaces_are_no_gap() {
    assert_eq!(first_gap("    indented"), None);
    assert_eq!(first_gap("Title only.   "), None);
    assert_eq!(first_gap("One:\n    - two\n\t  three  \n"), None);
}

#[test]
fn a_gap_on_the_second_line_reports_that_line_and_its_column() {
    let found = gap("Agreed.\n  Call it  instead.\n");
    assert_eq!((found.line, found.column), (2, 10));
    let refusal = TypedRulingError::Gap(found).to_string();
    assert!(
        refusal.starts_with("the answer has a gap at line 2, column 10 (\"  Call it  instead.\")"),
        "{refusal}"
    );
}

#[test]
fn a_column_counts_characters_not_bytes() {
    assert_eq!(gap("é à  ü").column, 4);
}

#[test]
fn an_empty_or_whitespace_only_ruling_is_refused_as_empty() {
    for text in ["", "   ", " \n\t\n"] {
        assert_eq!(
            check_typed_ruling(text, false),
            Err(TypedRulingError::Empty),
            "{text:?}"
        );
        assert_eq!(
            check_typed_ruling(text, true),
            Err(TypedRulingError::Empty),
            "as written: {text:?}"
        );
    }
    assert_eq!(TypedRulingError::Empty.way_on(), None);
}

#[test]
fn as_written_sends_a_ruling_with_a_gap() {
    assert_eq!(check_typed_ruling(EATEN, true), Ok(()));
}
