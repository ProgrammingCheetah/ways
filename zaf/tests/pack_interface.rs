//! The interface page gives a row to every verb of the `plan` tool (I9), and its `board file`
//! row says the verb is refused under an epic with a plan. The page is pasted unchanged into the
//! six role files (`pack_briefs.rs` holds that).

use std::path::Path;

fn page() -> String {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(crate_dir.join("assets/interface.md"))
        .expect("the interface page reads")
}

fn row<'a>(page: &'a str, start: &str) -> &'a str {
    page.lines()
        .find(|line| line.starts_with(start))
        .unwrap_or_else(|| panic!("the page has no row starting {start}"))
}

#[test]
fn interface_page_lists_every_plan_verb() {
    let page = page();
    for (verb, needle) in [
        ("check", "`plan` `[\"check\", \"<epic>\"]`"),
        ("file", "`plan` `[\"file\", \"<epic>\"]`"),
        ("show", "`plan` `[\"show\", \"<epic>\"]`"),
        ("verify", "`plan` `[\"verify\", \"<epic>\"]`"),
    ] {
        let line = row(&page, &format!("| {needle}"));
        assert!(!line.is_empty(), "plan {verb} has no row");
    }
    assert!(
        row(&page, "| `plan` `[\"file\"").contains("empty `stdin` finishes an interrupted filing"),
        "the plan file row must say empty stdin finishes an interrupted filing"
    );
    assert!(
        row(&page, "| `plan` `[\"show\"").contains("`--before`"),
        "the plan show row must name --before"
    );
    assert!(
        row(&page, "| `board` `[\"file\"").contains("refused under an epic with a plan"),
        "the board file row must say it is refused under an epic with a plan"
    );
}
