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
fn drop_task_closes_as_not_planned() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.drop_task(TicketId::from(2)).expect("drop");
    assert_eq!(board.closed_as(TicketId::from(2)), Some(false));
}

#[test]
fn dropped_task_is_not_done_for_a_blocker() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.drop_task(TicketId::from(2)).expect("drop");
    assert_eq!(
        board.standing(TicketId::from(2)),
        Ok(Standing::ClosedNotPlanned)
    );
}

#[test]
fn drop_task_on_a_closed_task_is_a_no_op() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.close(TicketId::from(2));
    assert_eq!(board.drop_task(TicketId::from(2)), Ok(()));
    assert_eq!(board.closed_as(TicketId::from(2)), Some(true));
    assert_eq!(board.standing(TicketId::from(2)), Ok(Standing::Done));
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
fn fake_board_opened_at() {
    use std::time::{Duration, SystemTime};

    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.add_ticket(ticket(3));
    let opened = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    board.set_opened_at(TicketId::from(2), opened);
    board.close(TicketId::from(2));

    assert_eq!(board.opened_at(TicketId::from(2)), Ok(Some(opened)));
    assert_eq!(board.opened_at(TicketId::from(3)), Ok(None), "none set");
    assert_eq!(
        board.opened_at(TicketId::from(9)),
        Ok(None),
        "no such issue"
    );
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

#[test]
fn the_fake_board_files_epics_from_many_threads_with_distinct_ids() {
    let board = FakeBoard::default();
    let ids: Vec<TicketId> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let board = &board;
                scope.spawn(move || {
                    board
                        .file_epic(&NewEpic {
                            title: format!("epic {n}").into(),
                            body: "".into(),
                        })
                        .expect("file epic")
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });
    let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(distinct.len(), 8);
    let epics: Vec<TicketId> = board
        .epics()
        .expect("epics")
        .iter()
        .map(|epic| epic.id)
        .collect();
    for id in &ids {
        assert!(epics.contains(id), "{id} reads back as an epic");
    }
}

#[test]
fn the_fake_board_files_tasks_from_many_threads_with_distinct_ids() {
    let board = FakeBoard::default();
    let epic = board
        .file_epic(&NewEpic {
            title: "epic".into(),
            body: "".into(),
        })
        .expect("file epic");
    let ids: Vec<TicketId> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let board = &board;
                scope.spawn(move || {
                    board
                        .file_task(&NewTask {
                            epic,
                            title: format!("task {n}").into(),
                            body: "".into(),
                            state: TicketState::Ready,
                            group: None,
                            kind: "Task".into(),
                        })
                        .expect("file task")
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });
    let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(distinct.len(), 8);
}

#[test]
fn fake_epic_task_carries_body_and_url() {
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(1), "epic");
    let mut task = ticket(2);
    task.epic = Some(TicketId::from(1));
    task.url = "https://board.invalid/tickets/2".into();
    board.add_ticket(task);
    let epics = board.epics().expect("epics");
    let read = &epics[0].tasks[0];
    assert_eq!(read.body, "Goal: export the list".into());
    assert_eq!(read.url, "https://board.invalid/tickets/2".into());
}

#[test]
fn fake_board_counts_calls_by_method() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.epics().expect("epics");
    board.epics().expect("epics");
    board.task(TicketId::from(2)).expect("task");
    assert_eq!(board.calls("epics"), 2);
    assert_eq!(board.calls("task"), 1);
    assert_eq!(board.calls("claims"), 0);
    // A scene's own arranging is no port call, and one port call counts once.
    board.add_epic(TicketId::from(5), "epic");
    board.epic(TicketId::from(5)).expect("epic");
    board.standing(TicketId::from(2)).expect("standing");
    assert_eq!(board.calls("epic"), 1);
    assert_eq!(board.calls("epics"), 2);
    assert_eq!(board.calls("standing"), 1);
    assert_eq!(board.calls("standing_at"), 0);
}

#[test]
fn fake_board_reset_clears_counts() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    board.epics().expect("epics");
    board.task(TicketId::from(2)).expect("task");
    board.ready().expect("ready");
    board.reset_calls();
    for method in ["epics", "task", "ready"] {
        assert_eq!(board.calls(method), 0, "{method}");
    }
}

#[test]
fn fake_board_counts_calls_from_threads() {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2));
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                for _ in 0..500 {
                    board.task(TicketId::from(2)).expect("task");
                }
            });
        }
    });
    assert_eq!(board.calls("task"), 1000);
}
