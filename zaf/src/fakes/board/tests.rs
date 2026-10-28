use super::*;
use crate::ports::NewQuestion;

fn ticket(number: u64) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: None,
        title: format!("task {number}"),
        body: "Goal: export the list".to_owned(),
        label: Some(TaskLabel::Ready),
    }
}

#[test]
fn a_ruling_keeps_the_question_label_and_closes_the_issue() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    let new_question = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: false,
    };
    let question = board.open_question(&new_question).expect("question");
    assert!(!board.is_closed(question));
    board
        .record_ruling(question, "Title and author.")
        .expect("ruling");
    assert!(board.is_closed(question));
    let label = board.task(question).expect("question ticket").label;
    assert_eq!(label, Some(TaskLabel::Question));
    assert!(!board.is_closed(IssueNumber::from(2)));
}
