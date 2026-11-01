use std::fs;

use super::*;
use crate::adapters::design::DirDesign;
use crate::domain::PageStatus;

fn page_id(id: &str) -> PageId {
    id.parse().expect("a valid page id")
}

/// Three pages, as `(id, slug, title, text)`.
const PAGES: [(&str, &str, &str, &str); 3] = [
    (
        "1001",
        "export",
        "Export",
        "# Export\n\nAn empty list exports the header row only.\n",
    ),
    (
        "1002",
        "reading-lists",
        "Reading lists",
        "# Reading lists\n\nA list belongs to one account.\n",
    ),
    (
        "1003",
        "shared-lists",
        "Shared lists",
        "# Shared lists\n\nA list belongs to many accounts.\nSuperseded: 1002.\n",
    ),
];

/// The same three pages in a fake and in a directory.
fn both() -> (FakeDesign, tempfile::TempDir, DirDesign) {
    let fake = FakeDesign::default();
    let dir = tempfile::tempdir().expect("a temp dir");
    let mut index = String::from("# Pages\n\n");
    for (id, slug, title, text) in PAGES {
        fake.add_page(Page {
            id: page_id(id),
            title: title.to_owned(),
            status: PageStatus::Decided,
            text: text.to_owned(),
        });
        fs::write(dir.path().join(format!("{id}-{slug}.md")), text).expect("write");
        index.push_str(&format!("- {id} decided — {title}\n"));
    }
    fs::write(dir.path().join("index.md"), index).expect("write");
    let design = DirDesign::new(dir.path());
    (fake, dir, design)
}

#[test]
fn both_sources_answer_every_query_with_the_same_ids() {
    let (fake, _dir, directory) = both();
    let queries = [
        "header row",
        "HEADER ROW",
        "list",
        "A list belongs to",
        "belongs to many",
        "row header",
        "one account. a list",
        "Superseded: 1002",
        "# export",
        "reading",
        "calendar",
    ];
    for query in queries {
        let from_fake = fake.search(query);
        assert!(from_fake.is_ok(), "the fake answers `{query}`");
        assert_eq!(from_fake, directory.search(query), "query `{query}`");
    }
    assert_eq!(
        fake.search("list"),
        Ok(vec![page_id("1001"), page_id("1002"), page_id("1003")])
    );
    assert_eq!(fake.search("row header"), Ok(Vec::new()));
}

#[test]
fn a_query_does_not_reach_the_title_or_cross_a_line() {
    let (fake, _dir, directory) = both();
    for source in [&fake as &dyn Design, &directory] {
        assert_eq!(source.search("Reading lists"), Ok(vec![page_id("1002")]));
        assert_eq!(source.search("only. a list"), Ok(Vec::new()));
    }
}

#[test]
fn both_sources_reject_an_empty_query() {
    let (fake, _dir, directory) = both();
    for query in ["", "   "] {
        assert!(matches!(fake.search(query), Err(PortError::Rejected(_))));
        assert!(matches!(
            directory.search(query),
            Err(PortError::Rejected(_))
        ));
    }
}
