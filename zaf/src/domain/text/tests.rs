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

#[test]
fn printable_keeps_newlines_and_tabs_and_drops_every_other_control() {
    assert_eq!(
        printable("a\u{1b}[2Jb\r\nc\td\u{7f}e\u{9b}31mf\u{0}"),
        "a[2Jb\nc\tde31mf"
    );
    assert_eq!(printable("plain words, ✓ ■ □"), "plain words, ✓ ■ □");
}

#[test]
fn printable_drops_the_bidi_and_zero_width_characters() {
    assert_eq!(
        printable("ok \u{202e}gnp.exe\u{202c} a\u{200b}b\u{2066}c\u{2069}\u{feff}d\u{200f}"),
        "ok gnp.exe abcd"
    );
    assert_eq!(printable("line\n\ttab"), "line\n\ttab");
}

#[test]
fn printable_drops_tags_selectors_and_every_other_format_character() {
    // "Yes." and, in tag characters, a sentence that draws as nothing.
    let tagged: String = " close #12"
        .chars()
        .map(|c| char::from_u32(0xE0000 + u32::from(c)).expect("a tag"))
        .collect();
    assert_eq!(
        printable(&format!("Yes.\u{E0001}{tagged}\u{E007F}")),
        "Yes."
    );
    assert_eq!(
        printable("a\u{FE0F}b\u{E0100}c\u{E01EF}d\u{FE00}"),
        "abcd",
        "variation selectors"
    );
    assert_eq!(
        printable(
            "s\u{00AD}o\u{0600}f\u{061C}t\u{180E} \u{2060}w\u{2064}o\u{206F}r\u{FFF9}d\u{1D173}s"
        ),
        "soft words"
    );
    // What draws stays: letters, marks, symbols, CJK, emoji.
    assert_eq!(printable("é ü ✓ 日本 🙂"), "é ü ✓ 日本 🙂");
}
