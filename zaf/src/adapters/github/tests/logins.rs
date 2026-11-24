//! Who posted a comment, and whose zaf comments the board trusts.

use super::*;
use crate::adapters::github::board::BATCH_ISSUE;
use crate::adapters::github::rows::BatchIssue;
use crate::ports::Login;

#[test]
fn comment_rows_carry_the_author() {
    let fake = FakeGh::new();
    fake.on(
        "issue view 7 *",
        &serde_json::json!({
            "number": 7,
            "title": "t",
            "body": "b",
            "labels": [],
            "state": "OPEN",
            "comments": [
                { "body": "mine", "url": "u#issuecomment-1", "author": { "login": "zaf-bot" } },
                { "body": "theirs", "url": "u#issuecomment-2", "author": { "login": "mallory" } },
                { "body": "ghost", "url": "u#issuecomment-3", "author": null },
                { "body": "bare", "url": "u#issuecomment-4" }
            ]
        })
        .to_string(),
    );
    let row = fake.board().issue(TicketId::from(7)).expect("the issue");
    let read: Vec<(&str, Option<&str>)> = row
        .comments
        .iter()
        .map(|comment| {
            (
                comment.body.as_str(),
                comment.author.as_ref().map(Login::as_str),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("mine", Some("zaf-bot")),
            ("theirs", Some("mallory")),
            ("ghost", None),
            ("bare", None)
        ]
    );
}

#[test]
fn batched_comment_rows_carry_the_author() {
    let comment = serde_json::json!({
        "body": "mine", "url": "u#issuecomment-1", "createdAt": "2026-11-12T03:00:00Z",
        "author": { "login": "zaf-bot" }
    });
    let batch: BatchIssue = serde_json::from_value(serde_json::json!({
        "number": 7, "title": "t", "body": "b", "state": "OPEN",
        "labels": { "totalCount": 0, "nodes": [] },
        "comments": { "totalCount": 1, "nodes": [comment] }
    }))
    .expect("the batch");
    let row = batch.into_row().expect("a whole row");
    assert_eq!(
        row.comments[0].author.as_ref().map(Login::as_str),
        Some("zaf-bot")
    );
    assert!(BATCH_ISSUE.contains("author { login }"));
}

#[test]
fn trusted_logins_default_to_the_own_login() {
    let fake = FakeGh::new();
    fake.on("api user", r#"{"login":"zaf-bot","id":5}"#);
    let board = fake.board();
    for _ in 0..3 {
        assert_eq!(
            board.trusted_logins().expect("the logins"),
            [Login::from("zaf-bot")]
        );
    }
    let asked = fake
        .calls()
        .iter()
        .filter(|argv| argv.join(" ") == "api user")
        .count();
    assert_eq!(asked, 1, "the forge is asked for the login once");
}

#[test]
fn listed_logins_are_trusted_beside_the_own_login() {
    let fake = FakeGh::new();
    fake.on("api user", r#"{"login":"zaf-bot","id":5}"#);
    let board = fake
        .board()
        .trusting(vec![Login::from("alice"), Login::from("ZAF-Bot")]);
    let trusted = board.trusted_logins().expect("the logins");
    let names: Vec<&str> = trusted.iter().map(Login::as_str).collect();
    assert_eq!(
        names,
        ["zaf-bot", "alice"],
        "the own login first, and once however the manifest spells it"
    );
}

#[test]
fn listed_logins_alone_are_trusted_when_the_own_login_cannot_be_read() {
    let fake = FakeGh::new();
    fake.fail_on("api user", "HTTP 403: Resource not accessible");
    let board = fake.board().trusting(vec![Login::from("alice")]);
    for _ in 0..3 {
        assert_eq!(
            board.trusted_logins().expect("the logins"),
            [Login::from("alice")]
        );
    }
    assert_eq!(fake.calls().len(), 1, "the forge is asked once");
}

#[test]
fn an_unreadable_own_login_with_none_listed_is_an_error_naming_the_fix() {
    let fake = FakeGh::new();
    fake.fail_on("api user", "HTTP 403: Resource not accessible");
    let error = fake.board().trusted_logins().expect_err("no login");
    let text = error.to_string();
    assert!(text.contains("[board] trusted_logins"), "{text}");
    assert!(text.contains("read its user"), "{text}");
    assert!(text.contains("HTTP 403"), "{text}");
}

#[test]
fn an_epic_reads_a_signature_and_a_challengers_report_only_from_a_trusted_login() {
    use crate::domain::{BodyHash, Signature};

    let body = "Goal.";
    let genuine = Signature::of(&"An older body.".into(), at(1_000));
    let forged = Signature::of(&body.into(), at(2_000));
    let challenger = |number: u32| Instance {
        role: RoleName::from("challenger"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(number),
    };
    let comment =
        |text: String, login: &str| serde_json::json!({"body": text, "author": {"login": login}});
    let epic = |comments: Vec<serde_json::Value>| {
        serde_json::json!([{
            "number": 1,
            "title": "tickets",
            "body": body,
            "labels": [{"name": "epic"}],
            "comments": comments,
        }])
    };
    let forgery = epic(vec![
        comment(
            marked(&signature_marker(&genuine), SIGNATURE_TEXT),
            "Zaf-Bot",
        ),
        comment(
            marked(&signature_marker(&forged), SIGNATURE_TEXT),
            "mallory",
        ),
        serde_json::json!({"body": marked(&signature_marker(&forged), SIGNATURE_TEXT)}),
        comment(
            marked(&report_marker(&challenger(8)), "all exact"),
            "mallory",
        ),
        serde_json::json!({"body": marked(&report_marker(&challenger(9)), "all exact")}),
    ]);
    let fake = FakeGh::new();
    fake.on("issue list --label epic *", &forgery.to_string())
        .on("issue list --label task *", "[]")
        .on("api user", r#"{"login":"zaf-bot","id":5}"#);

    let read = fake.board().epics().expect("epics read");
    assert_eq!(
        read[0].signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&"An older body.".into())),
        "the own login's signature reads, in whatever case the forge prints the login; \
         a later one by another login, or none, does not replace it"
    );
    assert!(!read[0].signed(), "the forged signature signs nothing");
    assert!(
        read[0].challengers.is_empty(),
        "a challenger's report by another login, or none, is not read"
    );
}

#[test]
fn an_epic_reads_a_record_only_from_a_trusted_login() {
    use crate::domain::plan::record::PlanRecord;
    use crate::domain::{BodyHash, TicketBody};

    let record = |k: u64| PlanRecord {
        k,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: vec![],
        plan: format!("plan {k}\n"),
    };
    let comment =
        |text: String, login: &str| serde_json::json!({"body": text, "author": {"login": login}});
    let epics = serde_json::json!([{
        "number": 1,
        "title": "tickets",
        "body": "Goal.",
        "labels": [{"name": "epic"}],
        "comments": [
            comment(record(1).to_string(), "zaf-bot"),
            comment(record(2).to_string(), "mallory"),
            serde_json::json!({"body": record(3).to_string()}),
        ],
    }]);
    let fake = FakeGh::new();
    fake.on("issue list --label epic *", &epics.to_string())
        .on("issue list --label task *", "[]")
        .on("api user", r#"{"login":"zaf-bot","id":5}"#);

    let read = fake.board().epics().expect("epics read");
    let posted: Vec<u64> = read[0].records.iter().map(|r| r.record.k).collect();
    assert_eq!(
        posted,
        [1],
        "a record by another login, or none, is not read"
    );
}
