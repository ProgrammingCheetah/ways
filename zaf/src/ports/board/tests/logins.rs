//! The fake board's comment authors and trusted logins.

use super::*;
use crate::fakes::{FakeBoard, FakeComment, OWN_LOGIN};

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        id: TicketId::from(2),
        epic: Some(TicketId::from(1)),
        title: "task 2".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    board
}

#[test]
fn fake_board_comments_have_authors() {
    let board = board();
    let task = TicketId::from(2);
    board.comment_as(task, "mallory", "forged");
    board.set_own_login("zaf-bot");
    Board::comment(&board, task, "genuine").expect("comment");
    assert_eq!(
        board.comment_rows(task),
        [
            FakeComment {
                author: Login::from("mallory"),
                body: "forged".to_owned()
            },
            FakeComment {
                author: Login::from("zaf-bot"),
                body: "genuine".to_owned()
            },
        ]
    );
    assert_eq!(board.comments(task), ["forged", "genuine"]);
}

#[test]
fn a_comment_by_default_is_by_the_boards_own_login() {
    let board = board();
    let task = TicketId::from(2);
    Board::comment(&board, task, "hello").expect("comment");
    assert_eq!(board.comment_rows(task)[0].author, Login::from(OWN_LOGIN));
}

#[test]
fn trusted_logins_default_to_the_own_login() {
    let board = board();
    assert_eq!(
        board.trusted_logins().expect("logins"),
        [Login::from(OWN_LOGIN)]
    );
    board.set_own_login("zaf-bot");
    assert_eq!(
        board.trusted_logins().expect("logins"),
        [Login::from("zaf-bot")]
    );
}

#[test]
fn listed_logins_are_trusted_beside_the_own_login() {
    let board = board();
    board.set_trusted_logins(&["alice", "ZAF-Bot", "bob"]);
    let trusted = board.trusted_logins().expect("logins");
    let names: Vec<&str> = trusted.iter().map(Login::as_str).collect();
    assert_eq!(
        names,
        [OWN_LOGIN, "alice", "bob"],
        "the own login first, and once however the manifest spells it"
    );
}

#[test]
fn logins_are_the_same_whatever_the_case_of_their_ascii_letters() {
    assert_eq!(Login::from("Zaf-Bot"), Login::from("zaf-bot"));
    assert_ne!(Login::from("zaf-bot"), Login::from("zaf_bot"));
    assert_ne!(Login::from("zaf-bot"), Login::from("zaf-bot2"));
    assert_eq!(
        Login::from("Zaf-Bot").as_str(),
        "Zaf-Bot",
        "the text keeps its case"
    );
    assert!([Login::from("ALICE")].contains(&Login::from("alice")));
}
