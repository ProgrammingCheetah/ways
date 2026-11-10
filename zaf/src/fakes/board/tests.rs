use super::*;
use crate::domain::{InstanceNumber, RoleName};
use crate::ports::NewQuestion;

fn ticket(number: u64) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: None,
        title: format!("task {number}").into(),
        body: "Goal: export the list".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
    }
}

#[test]
fn a_ruling_keeps_the_question_label_and_closes_the_issue() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    let new_question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: false,
    };
    let question = board.open_question(&new_question).expect("question");
    assert!(!board.is_closed(question));
    board
        .record_ruling(question, "Title and author.")
        .expect("ruling");
    assert!(board.is_closed(question));
    let label = board.task(question).expect("question ticket").state;
    assert_eq!(label, Some(TicketState::Question));
    assert!(!board.is_closed(TicketId::from(2)));
}

#[test]
fn a_ruling_returns_a_waiting_task_to_ready_and_frees_its_claim() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    let instance = Instance {
        role: RoleName::from("programmer"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(1),
    };
    board.claim(TicketId::from(2), &instance).expect("claim");
    let new_question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: false,
    };
    let question = board.open_question(&new_question).expect("question");
    board
        .set_state(TicketId::from(2), TicketState::Question)
        .expect("the task waits");
    let returned = board.record_ruling(question, "Title and author.");
    assert_eq!(returned, Ok(Some(TicketId::from(2))));
    let task = board.task(TicketId::from(2)).expect("task");
    assert_eq!(task.state, Some(TicketState::Ready));
    assert!(board.claims().expect("claims").is_empty());
    assert_eq!(board.record_ruling(question, "Title and author."), Ok(None));
}

#[test]
fn a_ruling_leaves_a_done_task_alone() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    let new_question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: false,
    };
    let question = board.open_question(&new_question).expect("question");
    board
        .set_state(TicketId::from(2), TicketState::Done)
        .expect("done");
    assert_eq!(board.record_ruling(question, "Title."), Ok(None));
    let task = board.task(TicketId::from(2)).expect("task");
    assert_eq!(task.state, Some(TicketState::Done));
}

#[test]
fn only_open_question_tickets_are_listed() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.add_question(Ticket {
        id: TicketId::from(8),
        title: "Seeded question".into(),
        ..ticket(8)
    });
    let new_question = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: false,
    };
    let opened = board.open_question(&new_question).expect("question");
    board
        .set_state(TicketId::from(2), TicketState::Question)
        .expect("the task waits");
    let listed: Vec<TicketId> = board
        .questions()
        .expect("questions")
        .iter()
        .map(|ticket| ticket.id)
        .collect();
    assert_eq!(listed, [TicketId::from(8), opened]);
    board.record_ruling(opened, "Title.").expect("ruling");
    assert_eq!(board.questions().expect("questions").len(), 1);
    let seeded = board.task(TicketId::from(8)).expect("seeded").state;
    assert_eq!(
        seeded,
        Some(TicketState::Question),
        "add_question labels it"
    );
}
