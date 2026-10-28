use super::*;
use crate::app::check::step::tests::{ForgePorts, context, forge_with_pr, located, located_on};
use crate::app::check::{Engine, EngineSettings, StepStatus, Verdict};
use crate::domain::Location;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn source() -> FindingSource {
    FindingSource::from("bot:review-bot")
}

fn location(path: &str, line: u32) -> Option<Location> {
    Some(Location {
        path: path.to_owned(),
        line: Some(line),
    })
}

#[test]
fn a_bot_that_has_not_reviewed_times_out_and_fails_the_step() {
    let (forge, pr) = forge_with_pr();
    let other = located("someone-else", "Looks fine.", "src/export.rs", 1);
    forge.add_review_comment(pr, other).expect("comment");
    let ports = ForgePorts(Arc::clone(&forge));
    let login = Author::from("review-bot");
    let outcome = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_millis(40)),
    );
    assert!(
        matches!(outcome, Err(PortError::Unavailable(_))),
        "{outcome:?}"
    );

    let steps = ["bot:review-bot".parse().expect("step")];
    let mut settings = EngineSettings::new(PathBuf::from("."), PathBuf::from("."));
    settings.timeout = Duration::from_millis(100);
    settings.poll = Duration::from_millis(5);
    let report = Engine::new(Arc::new(ports), settings).run(&steps, pr, false);
    // The step's own deadline and the engine's are the same instant, so either
    // may be first to notice; both end the check as an incomplete one.
    assert!(matches!(
        report.steps[0].status,
        StepStatus::Failed(_) | StepStatus::TimedOut
    ));
    assert_eq!(report.verdict(), Verdict::StepFailed);
}

#[test]
fn a_review_gives_one_must_fix_and_one_note() {
    let (forge, pr) = forge_with_pr();
    let must =
        "_Potential issue_ | _Major_\n\n**The file is never closed.**\n\nWrap it in a guard.";
    let note = "**Name this binding**\n\nA nit.";
    let summary = ReviewComment {
        author: Author::from("review-bot"),
        body: "Actionable comments posted: 2".to_owned(),
        location: None,
        head: None,
    };
    for comment in [
        located("review-bot", must, "src/export.rs", 40),
        located(
            "another-person",
            "Potential issue: ignored.",
            "src/export.rs",
            41,
        ),
        summary,
        located("review-bot", note, "src/export.rs", 12),
    ] {
        forge.add_review_comment(pr, comment).expect("comment");
    }
    let ports = ForgePorts(forge);
    let login = Author::from("review-bot");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(1)),
    )
    .expect("step");
    let expected = vec![
        Finding {
            source: source(),
            severity: Severity::MustFix,
            message: "The file is never closed.".to_owned(),
            location: location("src/export.rs", 40),
        },
        Finding {
            source: source(),
            severity: Severity::Advisory,
            message: "Name this binding".to_owned(),
            location: location("src/export.rs", 12),
        },
    ];
    assert_eq!(found, expected);
}

#[test]
fn a_security_marker_is_a_must_fix_and_the_login_ignores_case() {
    let (forge, pr) = forge_with_pr();
    let body = "**Token written to the log**\n\n_Security_ | _Critical_";
    forge
        .add_review_comment(pr, located("Review-Bot", body, "src/auth.rs", 7))
        .expect("comment");
    let ports = ForgePorts(forge);
    let login = Author::from("review-bot");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(1)),
    )
    .expect("step");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::MustFix);
    assert_eq!(found[0].message, "Token written to the log");
}

#[test]
fn a_review_that_lands_later_is_waited_for() {
    let (forge, pr) = forge_with_pr();
    let ports = ForgePorts(Arc::clone(&forge));
    let late = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        let comment = located("review-bot", "**Late nit**", "src/export.rs", 3);
        forge.add_review_comment(pr, comment).expect("comment");
    });
    let login = Author::from("review-bot");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(5)),
    )
    .expect("step");
    late.join().expect("late thread");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "Late nit");
}

#[test]
fn a_review_of_an_older_head_is_not_waited_on_when_the_head_is_known() {
    let (forge, pr) = forge_with_pr();
    forge.set_head(pr, Some("new")).expect("head");
    let stale = located_on("review-bot", "**Old nit**", "src/export.rs", 3, "old");
    forge.add_review_comment(pr, stale).expect("comment");
    let ports = ForgePorts(Arc::clone(&forge));
    let login = Author::from("review-bot");
    let outcome = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_millis(40)),
    );
    assert!(
        matches!(outcome, Err(PortError::Unavailable(_))),
        "{outcome:?}"
    );

    let fresh = located_on("review-bot", "**New nit**", "src/export.rs", 5, "new");
    forge.add_review_comment(pr, fresh).expect("comment");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(1)),
    )
    .expect("step");
    let messages: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
    assert_eq!(messages, ["New nit"], "only the review of the current head");
}

#[test]
fn the_head_is_read_again_each_round() {
    let (forge, pr) = forge_with_pr();
    forge.set_head(pr, Some("old")).expect("head");
    let comment = located_on("review-bot", "**Nit**", "src/export.rs", 3, "new");
    forge.add_review_comment(pr, comment).expect("comment");
    let ports = ForgePorts(Arc::clone(&forge));
    let pusher = {
        let forge = Arc::clone(&forge);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            forge.set_head(pr, Some("new")).expect("head");
        })
    };
    let login = Author::from("review-bot");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(5)),
    )
    .expect("step");
    pusher.join().expect("pusher thread");
    assert_eq!(
        found.len(),
        1,
        "waited until the head caught up with the review"
    );
}

#[test]
fn without_a_reported_head_any_comment_by_the_login_counts() {
    let (forge, pr) = forge_with_pr();
    let comment = located_on("review-bot", "**Any nit**", "src/export.rs", 3, "whatever");
    forge.add_review_comment(pr, comment).expect("comment");
    let ports = ForgePorts(forge);
    let login = Author::from("review-bot");
    let found = run(
        &source(),
        &login,
        &ports,
        &context(pr, Duration::from_secs(1)),
    )
    .expect("step");
    assert_eq!(found.len(), 1);
}
