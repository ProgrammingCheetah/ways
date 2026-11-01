use super::*;
use crate::fakes::FakeDesign;

fn page_id(id: &str) -> PageId {
    id.parse().expect("a valid page id")
}

fn seeded() -> FakeDesign {
    let design = FakeDesign::default();
    design.add_page(Page {
        id: page_id("1001"),
        title: "Export".to_owned(),
        status: PageStatus::Decided,
        text: "An empty list exports the header row only.".to_owned(),
    });
    design.add_page(Page {
        id: page_id("1002"),
        title: "Reading lists".to_owned(),
        status: PageStatus::Superseded,
        text: "A list belongs to one account.".to_owned(),
    });
    design
}

#[test]
fn pages_read_by_id_with_their_status() {
    let design = seeded();
    let port: &dyn Design = &design;
    let page = port.page(&page_id("1001")).expect("page");
    assert_eq!(page.title, "Export");
    assert_eq!(port.status(&page_id("1002")), Ok(PageStatus::Superseded));
    assert!(matches!(
        port.page(&page_id("9999")),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        port.status(&page_id("9999")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn search_matches_every_word_in_title_or_text() {
    let design = seeded();
    let port: &dyn Design = &design;
    assert_eq!(port.search("HEADER row"), Ok(vec![page_id("1001")]));
    assert_eq!(
        port.search("list"),
        Ok(vec![page_id("1001"), page_id("1002")])
    );
    assert_eq!(port.search("calendar"), Ok(Vec::new()));
}
