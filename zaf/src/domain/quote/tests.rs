use super::*;

const PAGE: &str = "## Renames\n\nA second rename within thirty days is refused\nwith a rate-limit error; the window is per account.";

#[test]
fn accepts_a_verbatim_quote() {
    let quote = Quote::from("A second rename within thirty days is refused");
    assert!(quote.is_found_in(PAGE));
}

#[test]
fn accepts_a_quote_with_whitespace_differences() {
    let quote = Quote::from("  refused   with a\trate-limit error;\nthe window ");
    assert!(quote.is_found_in(PAGE));
}

#[test]
fn rejects_a_changed_word() {
    let quote = Quote::from("A second rename within thirty days is rejected");
    assert!(!quote.is_found_in(PAGE));
}

#[test]
fn rejects_a_partial_word_and_an_empty_quote() {
    assert!(!Quote::from("econd rename").is_found_in(PAGE));
    assert!(!Quote::from("   ").is_found_in(PAGE));
}
