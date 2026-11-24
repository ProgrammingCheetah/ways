//! Who posted a comment, and whose zaf comments the board trusts.

use super::*;
use crate::ports::Login;

#[test]
fn comment_rows_carry_the_author() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/7/comments*"),
        200,
        &json!([
            { "id": 2, "body": "second", "created_at": "2026-11-12T03:00:00-07:00", "user": { "login": "mallory" } },
            { "id": 1, "body": "first", "created_at": "2026-11-12T02:00:00-07:00", "user": { "login": "zaf-bot" } },
            { "id": 3, "body": "ghost", "created_at": "2026-11-12T04:00:00-07:00", "user": null },
            { "id": 4, "body": "bare", "created_at": "2026-11-12T05:00:00-07:00" },
        ]),
    );
    let rows = fake.board().comments(TicketId::from(7)).expect("comments");
    let read: Vec<(&str, Option<&str>)> = rows
        .iter()
        .map(|row| (row.body.as_str(), row.author.as_ref().map(Login::as_str)))
        .collect();
    assert_eq!(
        read,
        [
            ("first", Some("zaf-bot")),
            ("second", Some("mallory")),
            ("ghost", None),
            ("bare", None)
        ]
    );
}

#[test]
fn trusted_logins_default_to_the_own_login() {
    let fake = FakeCurl::new();
    fake.on(
        "GET */api/v1/user",
        200,
        &json!({ "id": 5, "login": "zaf-bot" }),
    );
    let board = fake.board();
    for _ in 0..3 {
        assert_eq!(
            board.trusted_logins().expect("the logins"),
            [Login::from("zaf-bot")]
        );
    }
    let asked = fake
        .asked()
        .into_iter()
        .filter(|asked| asked.ends_with("/api/v1/user"))
        .count();
    assert_eq!(asked, 1, "the forge is asked for the login once");
}

#[test]
fn listed_logins_are_trusted_beside_the_own_login() {
    let fake = FakeCurl::new();
    fake.on(
        "GET */api/v1/user",
        200,
        &json!({ "id": 5, "login": "zaf-bot" }),
    );
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
    let fake = FakeCurl::new();
    fake.on(
        "GET */api/v1/user",
        403,
        &json!({ "message": "token does not have at least one of required scope(s): [read:user]" }),
    );
    let board = fake.board().trusting(vec![Login::from("alice")]);
    for _ in 0..3 {
        assert_eq!(
            board.trusted_logins().expect("the logins"),
            [Login::from("alice")]
        );
    }
    let asked = fake
        .asked()
        .into_iter()
        .filter(|asked| asked.ends_with("/api/v1/user"))
        .count();
    assert_eq!(asked, 1, "the forge is asked once");
}

#[test]
fn an_unreadable_own_login_with_none_listed_is_an_error_naming_the_fix() {
    let fake = FakeCurl::new();
    fake.on(
        "GET */api/v1/user",
        403,
        &json!({ "message": "token does not have at least one of required scope(s): [read:user]" }),
    );
    let error = fake.board().trusted_logins().expect_err("no login");
    let text = error.to_string();
    assert!(text.contains("[board] trusted_logins"), "{text}");
    assert!(text.contains("read its user"), "{text}");
    assert!(text.contains("read:user"), "{text}");
}

#[test]
fn an_epic_reads_a_signature_and_a_challengers_report_only_from_a_trusted_login() {
    let body = "An epic.";
    let genuine = Signature::of(&"An older body.".into(), at(1_700_000_000));
    let forged = Signature::of(&body.into(), at(1_800_000_000));
    let report = |number: u32| {
        let challenger: Instance = format!("challenger instance {number} on #1")
            .parse()
            .expect("an instance");
        marked(&report_marker(&challenger), "Every task is exact.")
    };
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=epic*"),
        200,
        &json!([issue_with_body(1, &["epic"], "open", body)]),
    );
    fake.on(
        &format!("GET {API}/issues/1/comments"),
        200,
        &json!([
            { "id": 1, "body": marked(&signature_marker(&genuine), "Signed."), "user": { "login": "Zaf-Bot" } },
            { "id": 2, "body": marked(&signature_marker(&forged), "Signed."), "user": { "login": "mallory" } },
            { "id": 3, "body": marked(&signature_marker(&forged), "Signed.") },
            { "id": 4, "body": report(8), "user": { "login": "mallory" } },
            { "id": 5, "body": report(9) },
        ]),
    );
    fake.on(
        &format!("GET {API}/issues/1/dependencies?*"),
        200,
        &json!([]),
    );
    fake.on(
        "GET */api/v1/user",
        200,
        &json!({ "id": 5, "login": "zaf-bot" }),
    );

    let epics = fake.board().epics().expect("epics read");
    assert_eq!(
        epics[0].signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&"An older body.".into())),
        "the own login's signature reads, in whatever case the forge prints the login; \
         a later one by another login, or none, does not replace it"
    );
    assert!(!epics[0].signed(), "the forged signature signs nothing");
    assert!(
        epics[0].challengers.is_empty(),
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
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=epic*"),
        200,
        &json!([issue(1, &["epic"], "open")]),
    );
    fake.on(
        &format!("GET {API}/issues/1/comments"),
        200,
        &json!([
            { "id": 1, "body": record(1).to_string(), "user": { "login": "zaf-bot" } },
            { "id": 2, "body": record(2).to_string(), "user": { "login": "mallory" } },
            { "id": 3, "body": record(3).to_string() },
        ]),
    );
    fake.on(
        &format!("GET {API}/issues/1/dependencies?*"),
        200,
        &json!([]),
    );
    fake.on(
        "GET */api/v1/user",
        200,
        &json!({ "id": 5, "login": "zaf-bot" }),
    );

    let epics = fake.board().epics().expect("epics read");
    let posted: Vec<u64> = epics[0].records.iter().map(|r| r.record.k).collect();
    assert_eq!(
        posted,
        [1],
        "a record by another login, or none, is not read"
    );
}

#[test]
fn a_failed_login_read_is_an_error_and_is_asked_again() {
    let fake = FakeCurl::new();
    fake.once(
        "GET */api/v1/user",
        401,
        &json!({ "message": "token is required" }),
    );
    fake.on("GET */api/v1/user", 200, &json!({ "login": "zaf-bot" }));
    let board = fake.board();
    assert!(board.trusted_logins().is_err());
    assert_eq!(
        board.trusted_logins().expect("the logins"),
        [Login::from("zaf-bot")]
    );
}
