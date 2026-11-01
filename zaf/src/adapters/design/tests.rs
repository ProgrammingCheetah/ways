use super::*;

fn page_id(id: &str) -> PageId {
    id.parse().expect("a valid page id")
}

/// Three pages and an index, the second page superseded by the third.
fn seeded() -> (tempfile::TempDir, DirDesign) {
    let dir = tempfile::tempdir().expect("a temp dir");
    let write = |name: &str, text: &str| fs::write(dir.path().join(name), text).expect("write");
    write(
        "index.md",
        "# Pages\n\n\
         - 1001 decided — Export\n\
         - 1002 superseded — Reading lists\n\
         - 1003 current — Shared lists\n",
    );
    write(
        "1001-export.md",
        "# Export\n\nAn empty list exports the header row only.\n",
    );
    write(
        "1002-reading-lists.md",
        "# Reading lists\n\nA list belongs to one account.\n",
    );
    write(
        "1003-shared-lists.md",
        "# Shared lists\n\nA list belongs to many accounts.\nSuperseded: 1002.\n",
    );
    let design = DirDesign::new(dir.path());
    (dir, design)
}

#[test]
fn page_is_found_by_id_with_title_status_and_text() {
    let (_dir, design) = seeded();
    let page = design.page(&page_id("1001")).expect("page");
    assert_eq!(page.id, page_id("1001"));
    assert_eq!(page.title, "Export");
    assert_eq!(page.status, PageStatus::Decided);
    assert_eq!(
        page.text,
        "# Export\n\nAn empty list exports the header row only.\n"
    );
}

#[test]
fn a_superseded_page_still_reads() {
    let (_dir, design) = seeded();
    let page = design.page(&page_id("1002")).expect("page");
    assert_eq!(page.status, PageStatus::Superseded);
}

#[test]
fn a_missing_id_is_not_found() {
    let (_dir, design) = seeded();
    assert!(matches!(
        design.page(&page_id("9999")),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        design.status(&page_id("9999")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn an_id_is_a_whole_prefix_not_a_partial_one() {
    let (_dir, design) = seeded();
    assert!(matches!(
        design.page(&page_id("100")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_page_missing_from_the_index_is_not_found() {
    let (dir, design) = seeded();
    fs::write(dir.path().join("1004-orphan.md"), "# Orphan\n").expect("write");
    assert!(matches!(
        design.page(&page_id("1004")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn search_returns_ids_in_order_ignoring_case() {
    let (_dir, design) = seeded();
    assert_eq!(design.search("HEADER row"), Ok(vec![page_id("1001")]));
    assert_eq!(
        design.search("a list belongs"),
        Ok(vec![page_id("1002"), page_id("1003")])
    );
}

#[test]
fn search_lines_name_the_matching_lines() {
    let (_dir, design) = seeded();
    let hits = design.search_lines("account").expect("search");
    let expected = vec![
        SearchHit {
            id: page_id("1002"),
            lines: vec!["A list belongs to one account.".to_owned()],
        },
        SearchHit {
            id: page_id("1003"),
            lines: vec!["A list belongs to many accounts.".to_owned()],
        },
    ];
    assert_eq!(hits, expected);
}

#[test]
fn search_does_not_look_in_the_index() {
    let (_dir, design) = seeded();
    assert_eq!(design.search("Pages"), Ok(Vec::new()));
}

#[test]
fn search_misses_return_nothing() {
    let (_dir, design) = seeded();
    assert_eq!(design.search("calendar"), Ok(Vec::new()));
}

#[test]
fn an_empty_query_is_rejected() {
    let (_dir, design) = seeded();
    assert!(matches!(design.search("  "), Err(PortError::Rejected(_))));
}

#[test]
fn status_reads_the_index() {
    let (_dir, design) = seeded();
    assert_eq!(design.status(&page_id("1001")), Ok(PageStatus::Decided));
    assert_eq!(design.status(&page_id("1002")), Ok(PageStatus::Superseded));
    assert_eq!(design.status(&page_id("1003")), Ok(PageStatus::Current));
}

#[test]
fn an_unknown_status_is_a_backend_failure() {
    let (dir, design) = seeded();
    fs::write(dir.path().join("index.md"), "- 1001 draft — Export\n").expect("write");
    assert!(matches!(
        design.status(&page_id("1001")),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn the_index_file_is_named_by_the_caller() {
    let (dir, _) = seeded();
    fs::rename(dir.path().join("index.md"), dir.path().join("INDEX.md")).expect("rename");
    let design = DirDesign::new(dir.path()).with_index("INDEX.md");
    assert_eq!(design.status(&page_id("1001")), Ok(PageStatus::Decided));
    assert_eq!(design.search("pages"), Ok(Vec::new()));
}

#[test]
fn a_missing_directory_is_unavailable() {
    let (dir, _) = seeded();
    let design = DirDesign::new(dir.path().join("absent"));
    assert!(matches!(
        design.page(&page_id("1001")),
        Err(PortError::Unavailable(_))
    ));
}

#[test]
fn index_lines_parse_and_others_do_not() {
    let entry = IndexEntry::parse("- 1001 decided — Export rules").expect("an entry");
    assert_eq!(entry.id, "1001");
    assert_eq!(entry.status, "decided");
    assert_eq!(entry.title, "Export rules");
    assert!(IndexEntry::parse("# Pages").is_none());
    assert!(IndexEntry::parse("- 1001 decided Export").is_none());
    assert!(IndexEntry::parse("").is_none());
}
