use super::{fit, pad, width, wrap};

#[test]
fn fit_cuts_with_an_ellipsis_by_cells() {
    assert_eq!(fit("short", 10), "short");
    assert_eq!(fit("a longer line", 8), "a longe…");
    assert_eq!(
        fit("漢字漢字", 5),
        "漢字…",
        "a wide character takes two cells"
    );
    assert_eq!(fit("anything", 0), "");
    assert_eq!(width(&fit("■□?✗ squares", 6)), 6);
}

#[test]
fn pad_fills_to_the_width() {
    assert_eq!(pad("ab", 4), "ab  ");
    assert_eq!(pad("abcdef", 4), "abc…");
}

#[test]
fn wrap_breaks_at_spaces_and_keeps_lines_apart() {
    assert_eq!(
        wrap("one two three four\n\nfive", 9),
        ["one two", "three", "four", "", "five"]
    );
    assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap("", 4), [""]);
    assert!(wrap("a\tb c", 3).iter().all(|line| width(line) <= 3));
}
