use super::*;
use crate::fakes::FakeForge;

fn request(slice: &str) -> PrRequest {
    PrRequest {
        slice: SliceName::from(slice),
        base: BaseName::from("feature/v1"),
        title: "Add the CSV writer".to_owned(),
        body: "Closes #2".to_owned(),
    }
}

#[test]
fn a_slice_has_one_open_pull_request() {
    let forge = FakeForge::default();
    let port: &dyn Forge = &forge;
    let first = port.open_pr(&request("feature/v1-export-2")).expect("open");
    let again = port
        .open_pr(&request("feature/v1-export-2"))
        .expect("open again");
    let other = port
        .open_pr(&request("feature/v1-export-3"))
        .expect("open other");
    assert_eq!(first, again);
    assert_ne!(first, other);
    assert_eq!(forge.request(first), Some(request("feature/v1-export-2")));
}

#[test]
fn checks_and_review_comments_read_back() {
    let forge = FakeForge::default();
    let port: &dyn Forge = &forge;
    let pr = port.open_pr(&request("feature/v1-export-2")).expect("open");
    assert_eq!(port.checks(pr), Ok(Vec::new()));
    let checks = vec![
        Check {
            name: CheckName::from("ci"),
            state: CheckState::Passed,
        },
        Check {
            name: CheckName::from("scanner"),
            state: CheckState::Pending,
        },
    ];
    forge
        .set_checks(pr, checks.clone())
        .expect("arrange checks");
    assert_eq!(port.checks(pr), Ok(checks));
    let comment = ReviewComment {
        author: Author::from("reviewer-bot"),
        body: "This branch never closes the file.".to_owned(),
        location: Some(Location {
            path: "src/export.rs".to_owned(),
            line: Some(40),
        }),
        head: Some("abc123".to_owned()),
    };
    forge
        .add_review_comment(pr, comment.clone())
        .expect("arrange comment");
    assert_eq!(port.review_comments(pr), Ok(vec![comment]));
    assert!(matches!(
        port.checks(PrNumber::from(99)),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn scanning_alerts_read_back_per_pull_request() {
    let forge = FakeForge::default();
    let port: &dyn Forge = &forge;
    let pr = port.open_pr(&request("feature/v1-export-2")).expect("open");
    assert_eq!(port.scanning_alerts(pr), Ok(Vec::new()));
    let alerts = vec![
        ScanningAlert {
            rule: RuleId::from("js/sql-injection"),
            location: Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(40),
            }),
        },
        ScanningAlert {
            rule: RuleId::from("js/unused-import"),
            location: None,
        },
    ];
    forge
        .set_scanning_alerts(pr, alerts.clone())
        .expect("arrange alerts");
    assert_eq!(port.scanning_alerts(pr), Ok(alerts));
    assert!(matches!(
        port.scanning_alerts(PrNumber::from(99)),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn merge_is_idempotent() {
    let forge = FakeForge::default();
    let port: &dyn Forge = &forge;
    let pr = port.open_pr(&request("feature/v1-export-2")).expect("open");
    assert_eq!(port.merge(pr), Ok(PrMerge::Merged));
    assert_eq!(port.merge(pr), Ok(PrMerge::AlreadyMerged));
    let reopened = port
        .open_pr(&request("feature/v1-export-2"))
        .expect("a merged slice opens anew");
    assert_ne!(reopened, pr);
}

#[test]
fn a_pull_request_number_gives_its_bare_value() {
    assert_eq!(PrNumber::from(7).get(), 7);
    assert_eq!(PrNumber::from(7).to_string(), "#7");
}
