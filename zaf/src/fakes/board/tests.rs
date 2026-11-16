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
        kind: None,
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

#[test]
fn open_tickets_lists_open_tasks_in_any_state_but_done_in_number_order() {
    let board = FakeBoard::default();
    for (number, state) in [
        (5, Some(TicketState::Question)),
        (2, Some(TicketState::Ready)),
        (3, Some(TicketState::Blocked)),
        (4, Some(TicketState::Done)),
        (6, None),
        (7, Some(TicketState::Claimed)),
    ] {
        board.add_ticket(Ticket {
            state,
            ..ticket(number)
        });
    }
    board.close(TicketId::from(7));
    board.add_epic(TicketId::from(1), "the epic");
    board.add_question(Ticket {
        state: None,
        ..ticket(8)
    });
    let listed: Vec<TicketId> = board
        .open_tickets()
        .expect("open tickets")
        .iter()
        .map(|open| open.ticket.id)
        .collect();
    assert_eq!(listed, [2, 3, 5, 6].map(TicketId::from));
}

#[test]
fn only_open_blocked_tasks_are_listed_in_number_order() {
    let board = FakeBoard::default();
    for (number, state) in [
        (5, TicketState::Blocked),
        (2, TicketState::Ready),
        (3, TicketState::Blocked),
        (4, TicketState::Blocked),
    ] {
        board.add_ticket(Ticket {
            state: Some(state),
            ..ticket(number)
        });
    }
    board.close(TicketId::from(4));
    let listed: Vec<TicketId> = board
        .blocked()
        .expect("blocked")
        .iter()
        .map(|ticket| ticket.id)
        .collect();
    assert_eq!(listed, [TicketId::from(3), TicketId::from(5)]);
}
