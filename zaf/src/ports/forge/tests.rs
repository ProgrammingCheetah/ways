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
