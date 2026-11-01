use super::*;

#[test]
fn counts_words_across_any_whitespace() {
    assert_eq!(word_count("  one two\tthree\nfour "), 4);
    assert_eq!(word_count(""), 0);
}

#[test]
fn normalizes_runs_of_whitespace() {
    assert_eq!(normalize_whitespace("  a \n b\t\tc "), "a b c");
}

#[test]
fn strips_only_spaced_comments() {
    assert_eq!(strip_comment("for: #12"), "for: #12");
    assert_eq!(
        strip_comment("for: #12     # scopes the search"),
        "for: #12"
    );
    assert_eq!(
        strip_comment("from: team#3 / programmer otter  # who"),
        "from: team#3 / programmer otter"
    );
}
