use super::*;

#[test]
fn a_known_model_is_priced_from_the_table() {
    let table = PriceTable::default();
    let cost = table
        .rates("claude-sonnet-5-5")
        .cost(1_000_000, 1_000_000, 1_000_000, 1_000_000);
    assert!((cost - 14.7).abs() < 1e-9, "{cost}");
}

#[test]
fn an_unknown_model_takes_the_fallback() {
    let table = PriceTable::default();
    assert_eq!(
        table.rates("claude-future-9"),
        table.rates("claude-opus-5-5")
    );
}

#[test]
fn local_toml_overrides_one_model_and_leaves_the_rest() {
    let text =
        "[budget]\nweekly_usd = 100\n[report.prices]\n\"claude-sonnet-5-5\" = [1, 2, 3, 4.5]\n";
    let table = PriceTable::from_local_toml(text).expect("parses");
    assert_eq!(*table.rates("claude-sonnet-5-5"), rates(1.0, 2.0, 3.0, 4.5));
    assert_eq!(
        table.rates("claude-haiku-5-5"),
        PriceTable::default().rates("claude-haiku-5-5")
    );
}

#[test]
fn local_toml_without_prices_is_the_defaults() {
    assert_eq!(
        PriceTable::from_local_toml("").expect("empty"),
        PriceTable::default()
    );
    assert_eq!(
        PriceTable::from_local_toml("[budget]\nweekly_usd = 5\n").expect("budget only"),
        PriceTable::default()
    );
}

#[test]
fn a_malformed_price_row_is_named() {
    let error = PriceTable::from_local_toml("[report.prices]\nx = [1, 2]\n").expect_err("short");
    assert!(error.to_string().contains("`x`"), "{error}");
    assert!(PriceTable::from_local_toml("[report").is_err());
}
