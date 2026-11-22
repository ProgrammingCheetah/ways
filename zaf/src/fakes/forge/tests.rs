use super::*;

#[test]
fn fake_forge_counts_calls_by_method() {
    let forge = FakeForge::default();
    let pr = forge
        .open_pr(&PrRequest {
            slice: SliceName::from("feature/x"),
            base: BaseName::from("main"),
            title: "t".to_owned(),
            body: "b".to_owned(),
        })
        .expect("open");
    forge.checks(pr).expect("checks");
    forge.checks(pr).expect("checks");
    forge.head(pr).expect("head");
    assert_eq!(forge.calls("checks"), 2);
    assert_eq!(forge.calls("head"), 1);
    assert_eq!(forge.calls("merged"), 0);
    forge.reset_calls();
    assert_eq!(forge.calls("checks"), 0);
    assert_eq!(forge.calls("head"), 0);
}
