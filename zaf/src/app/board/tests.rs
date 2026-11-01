use super::*;
use crate::domain::{InstanceNumber, RoleName};
use crate::fakes::FakeBoard;

fn ticket(number: u64, label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: Some(IssueNumber::from(1)),
        title: format!("Task {number}"),
        body: String::new(),
        label: Some(label),
    }
}

fn instance(number: u32) -> Instance {
    Instance {
        role: RoleName::from("team"),
        epic: IssueNumber::from(1),
        number: InstanceNumber::from(number),
    }
}

fn board_with(tickets: &[(u64, TaskLabel)]) -> FakeBoard {
    let board = FakeBoard::default();
    for (number, label) in tickets {
        board.add_ticket(ticket(*number, *label));
    }
    board
}

fn claim_command(task: u64, by: u32, batch: u32) -> ClaimCommand {
    ClaimCommand {
        task: IssueNumber::from(task),
        instance: instance(by),
        batch,
    }
}

#[test]
fn ready_lists_only_ready_tasks_in_issue_order() {
    let board = board_with(&[
        (4, TaskLabel::Ready),
        (2, TaskLabel::Ready),
        (3, TaskLabel::Claimed),
        (5, TaskLabel::Done),
    ]);
    let numbers: Vec<IssueNumber> = ready(&board)
        .expect("ready")
        .into_iter()
        .map(|ticket| ticket.number)
        .collect();
    assert_eq!(numbers, [IssueNumber::from(2), IssueNumber::from(4)]);
}

#[test]
fn a_claim_takes_a_ready_task_and_a_repeat_is_quiet() {
    let board = board_with(&[(2, TaskLabel::Ready)]);
    let command = claim_command(2, 1, 1);
    assert_eq!(claim(&board, &command), Ok(ClaimOutcome::Claimed));
    assert_eq!(claim(&board, &command), Ok(ClaimOutcome::AlreadyHeld));
    let task = board.task(IssueNumber::from(2)).expect("task");
    assert_eq!(task.label, Some(TaskLabel::Claimed));
}

#[test]
fn claims_stop_at_the_batch_size() {
    let board = board_with(&[
        (2, TaskLabel::Ready),
        (3, TaskLabel::Ready),
        (4, TaskLabel::Ready),
    ]);
    claim(&board, &claim_command(2, 1, 2)).expect("first");
    claim(&board, &claim_command(3, 1, 2)).expect("second");
    let refused = claim(&board, &claim_command(4, 1, 2));
    let Err(BoardError::BatchFull { batch, held, .. }) = refused else {
        panic!("expected a full batch, got {refused:?}");
    };
    assert_eq!(batch, 2);
    assert_eq!(held, [IssueNumber::from(2), IssueNumber::from(3)]);
    let untouched = board.task(IssueNumber::from(4)).expect("task");
    assert_eq!(untouched.label, Some(TaskLabel::Ready));
    // A repeat of a held claim never counts against the batch.
    assert_eq!(
        claim(&board, &claim_command(3, 1, 2)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    // Another instance's claims are not this one's batch.
    assert_eq!(
        claim(&board, &claim_command(4, 2, 1)),
        Ok(ClaimOutcome::Claimed)
    );
}

#[test]
fn a_claim_refuses_a_task_held_by_another_or_not_ready() {
    let board = board_with(&[(2, TaskLabel::Ready), (3, TaskLabel::Blocked)]);
    claim(&board, &claim_command(2, 1, 4)).expect("claim");
    assert!(matches!(
        claim(&board, &claim_command(2, 2, 4)),
        Err(BoardError::Port(PortError::Conflict(_)))
    ));
    assert!(matches!(
        claim(&board, &claim_command(3, 2, 4)),
        Err(BoardError::Port(PortError::Rejected(_)))
    ));
    assert!(matches!(
        claim(&board, &claim_command(9, 2, 4)),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn a_report_replaces_the_last_and_touches_a_held_claim() {
    let board = board_with(&[(1, TaskLabel::Claimed), (2, TaskLabel::Ready)]);
    claim(&board, &claim_command(2, 1, 4)).expect("claim");
    let before = board.claims().expect("claims")[0].heartbeat;
    let command = ReportCommand {
        task: IssueNumber::from(2),
        instance: instance(1),
        body: "working".to_owned(),
    };
    let outcome = report(&board, &command).expect("report");
    assert_eq!(outcome, ReportOutcome { heartbeat: true });
    assert!(board.claims().expect("claims")[0].heartbeat >= before);
    let again = ReportCommand {
        body: "tests green".to_owned(),
        ..command
    };
    report(&board, &again).expect("a second report");
    report(&board, &again).expect("the same report again");
    let reports = board.reports(IssueNumber::from(2));
    assert_eq!(reports, [(instance(1), "tests green".to_owned())]);
}

#[test]
fn a_report_on_an_unclaimed_issue_writes_the_report_only() {
    let board = board_with(&[(1, TaskLabel::Ready)]);
    let command = ReportCommand {
        task: IssueNumber::from(1),
        instance: instance(1),
        body: "epic done".to_owned(),
    };
    let outcome = report(&board, &command).expect("report");
    assert_eq!(outcome, ReportOutcome { heartbeat: false });
    assert_eq!(board.reports(IssueNumber::from(1)).len(), 1);
    let missing = ReportCommand {
        task: IssueNumber::from(9),
        ..command
    };
    assert!(matches!(
        report(&board, &missing),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn a_question_opens_a_ticket_and_the_issue_waits() {
    let board = board_with(&[(2, TaskLabel::Claimed)]);
    let command = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns?".to_owned(),
        door: true,
    };
    let number = question(&board, &command).expect("question");
    assert_eq!(board.question(number), Some((true, None)));
    let waiting = board.task(IssueNumber::from(2)).expect("task");
    assert_eq!(waiting.label, Some(TaskLabel::Question));
    let nowhere = NewQuestion {
        on: IssueNumber::from(9),
        ..command
    };
    assert!(matches!(
        question(&board, &nowhere),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn asking_the_same_question_again_reuses_the_open_one() {
    let board = board_with(&[(2, TaskLabel::Claimed), (3, TaskLabel::Ready)]);
    let asked = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let first = question(&board, &asked).expect("question");
    let again = question(&board, &asked).expect("the same question");
    assert_eq!(again, first, "no second ticket");
    assert_eq!(board.questions().expect("questions").len(), 1);
    let waiting = board.task(IssueNumber::from(2)).expect("task");
    assert_eq!(waiting.label, Some(TaskLabel::Question));

    let elsewhere = NewQuestion {
        on: IssueNumber::from(3),
        ..asked.clone()
    };
    let other = question(&board, &elsewhere).expect("same text, other issue");
    assert_ne!(other, first, "another issue is another question");
    let reworded = NewQuestion {
        text: "Which rows?".to_owned(),
        ..asked.clone()
    };
    let third = question(&board, &reworded).expect("other text");
    assert_ne!(third, first);

    ruling(
        &board,
        &RulingCommand {
            question: first,
            ruling: "Title and author.".to_owned(),
        },
    )
    .expect("ruling");
    let reasked = question(&board, &asked).expect("asked after the ruling");
    assert_ne!(
        reasked, first,
        "a ruled question is not open, so it is asked anew"
    );
}

#[test]
fn a_ruling_closes_the_question_once() {
    let board = board_with(&[(2, TaskLabel::Claimed)]);
    let asked = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let number = question(&board, &asked).expect("question");
    let command = RulingCommand {
        question: number,
        ruling: "Title and author.".to_owned(),
    };
    ruling(&board, &command).expect("ruling");
    ruling(&board, &command).expect("the same ruling again is quiet");
    assert_eq!(
        board.question(number),
        Some((false, Some("Title and author.".to_owned())))
    );
    let different = RulingCommand {
        ruling: "Title only.".to_owned(),
        ..command
    };
    assert!(matches!(
        ruling(&board, &different),
        Err(BoardError::Port(PortError::Conflict(_)))
    ));
}
