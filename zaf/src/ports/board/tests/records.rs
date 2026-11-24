//! An epic reads its plan records and filing marks from its comments.

use super::*;
use crate::domain::plan::record::{FilingMark, GraphHash, PlanRecord};
use crate::domain::{BodyHash, EpicRecord, TicketBody};
use crate::fakes::FakeBoard;

const EPIC: u64 = 1;

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    let epic = NewEpic {
        title: "tickets".into(),
        body: "Release: 0.6.0\n\nGoal.".into(),
    };
    assert_eq!(board.file_epic(&epic), Ok(TicketId::from(EPIC)));
    board
}

fn record(k: u64) -> PlanRecord {
    PlanRecord {
        k,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: vec![TicketId::from(41)],
        plan: format!("plan {k}\n"),
    }
}

fn mark(k: u64) -> FilingMark {
    FilingMark {
        k,
        graph: "ab".repeat(32).parse::<GraphHash>().expect("a digest"),
    }
}

fn read(board: &FakeBoard) -> Vec<EpicRecord> {
    board
        .epic(TicketId::from(EPIC))
        .expect("the epic reads")
        .records
}

#[test]
fn epic_reads_records_and_marks_in_order() {
    let board = board();
    let epic = TicketId::from(EPIC);
    Board::comment(&board, epic, &record(1).to_string()).expect("comment");
    Board::comment(&board, epic, &mark(1).to_string()).expect("comment");
    Board::comment(&board, epic, &record(2).to_string()).expect("comment");

    assert_eq!(
        read(&board),
        [
            EpicRecord {
                record: record(1),
                posted_at: None,
                mark: Some(mark(1)),
            },
            EpicRecord {
                record: record(2),
                posted_at: None,
                mark: None,
            },
        ]
    );
}

#[test]
fn epic_ignores_a_record_from_another_author() {
    let board = board();
    let epic = TicketId::from(EPIC);
    Board::comment(&board, epic, &record(1).to_string()).expect("comment");
    board.comment_as(epic, "mallory", &record(2).to_string());
    board.comment_as(epic, "mallory", &mark(1).to_string());

    assert_eq!(
        read(&board),
        [EpicRecord {
            record: record(1),
            posted_at: None,
            mark: None,
        }],
        "a forged record is not read, and a forged mark does not file a real record"
    );
}

#[test]
fn a_listed_login_is_trusted_beside_the_own_login_whatever_its_case() {
    let board = board();
    let epic = TicketId::from(EPIC);
    board.set_trusted_logins(&["Alice"]);
    board.comment_as(epic, "alice", &record(1).to_string());
    Board::comment(&board, epic, &record(2).to_string()).expect("comment");
    board.comment_as(epic, "mallory", &record(3).to_string());

    let posted: Vec<u64> = read(&board).iter().map(|read| read.record.k).collect();
    assert_eq!(
        posted,
        [1, 2],
        "zaf's own record is read beside a listed one"
    );
}

#[test]
fn epic_ignores_a_signature_from_another_author() {
    let board = board();
    let epic = TicketId::from(EPIC);
    let signature = Signature::of(&"Release: 0.6.0\n\nGoal.".into(), SystemTime::UNIX_EPOCH);
    board.sign_as(epic, "mallory", &signature);
    let read = board.epic(epic).expect("the epic reads");
    assert_eq!(read.signature, None);
    assert!(!read.signed(), "a forged signature signs nothing");

    let older = Signature::of(&"An older body.".into(), SystemTime::UNIX_EPOCH);
    board.sign(epic, &older).expect("sign");
    board.sign_as(epic, "mallory", &signature);
    let read = board.epic(epic).expect("the epic reads");
    assert_eq!(
        read.signature,
        Some(older),
        "a later forged signature does not replace the board's own"
    );

    board.set_trusted_logins(&["Mallory"]);
    let read = board.epic(epic).expect("the epic reads");
    assert!(read.signed(), "a listed login's signature signs");
}

#[test]
fn epic_ignores_a_challengers_report_from_another_author() {
    let board = board();
    let epic = TicketId::from(EPIC);
    let challenger: Instance = format!("challenger instance 1 on #{EPIC}")
        .parse()
        .expect("an instance");
    board.report_as(epic, "mallory", &challenger, "Every task is exact.");
    let read = board.epic(epic).expect("the epic reads");
    assert!(read.challengers.is_empty(), "a forged report is not read");

    let second: Instance = format!("challenger instance 2 on #{EPIC}")
        .parse()
        .expect("an instance");
    board
        .report(epic, &second, "Every task is exact.")
        .expect("the challenger reports");
    let read = board.epic(epic).expect("the epic reads");
    assert_eq!(read.challengers, [None], "the board's own report is read");
}

#[test]
fn epic_without_records_reads_none() {
    let board = board();
    let epic = TicketId::from(EPIC);
    let signature = Signature::of(&"Release: 0.6.0\n\nGoal.".into(), SystemTime::UNIX_EPOCH);
    board.sign(epic, &signature).expect("sign");
    Board::comment(&board, epic, "a plain remark").expect("comment");

    let read = board.epic(epic).expect("the epic reads");
    assert!(read.records.is_empty());
    assert!(read.signed(), "its signature still holds");
}
