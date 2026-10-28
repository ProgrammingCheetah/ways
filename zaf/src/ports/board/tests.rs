use super::*;
use crate::domain::{InstanceNumber, RoleName};
use crate::fakes::FakeBoard;

fn ticket(number: u64, label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: Some(IssueNumber::from(1)),
        title: format!("task {number}"),
        body: String::new(),
        label: Some(label),
    }
}

fn instance(number: u32) -> Instance {
    Instance {
        role: RoleName::from("programmer"),
        epic: IssueNumber::from(1),
        number: InstanceNumber::from(number),
    }
}

fn seeded() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2, TaskLabel::Ready));
    board.add_ticket(ticket(3, TaskLabel::Blocked));
    board.add_ticket(ticket(4, TaskLabel::Ready));
    board
}

#[test]
fn ready_lists_only_ready_tasks_in_order() {
    let board = seeded();
    let port: &dyn Board = &board;
    let numbers: Vec<IssueNumber> = port
        .ready()
        .expect("ready")
        .iter()
        .map(|ticket| ticket.number)
        .collect();
    assert_eq!(numbers, [IssueNumber::from(2), IssueNumber::from(4)]);
    assert!(matches!(
        port.task(IssueNumber::from(9)),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn claims_are_idempotent_for_the_holder_and_exclusive_for_others() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = IssueNumber::from(2);
    assert_eq!(port.claim(task, &instance(1)), Ok(ClaimOutcome::Claimed));
    assert_eq!(
        port.claim(task, &instance(1)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    assert!(matches!(
        port.claim(task, &instance(2)),
        Err(PortError::Conflict(_))
    ));
    assert!(matches!(
        port.claim(IssueNumber::from(3), &instance(1)),
        Err(PortError::Rejected(_))
    ));
    assert_eq!(
        port.task(task).expect("task").label,
        Some(TaskLabel::Claimed)
    );
    assert!(
        port.ready()
            .expect("ready")
            .iter()
            .all(|ticket| ticket.number != task)
    );
}

#[test]
fn leases_beat_and_release_back_to_ready() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = IssueNumber::from(4);
    port.claim(task, &instance(1)).expect("claim");
    let first_beat = port.claims().expect("claims")[0].heartbeat;
    port.heartbeat(task, &instance(1)).expect("heartbeat");
    assert!(port.claims().expect("claims")[0].heartbeat >= first_beat);
    assert!(matches!(
        port.heartbeat(task, &instance(2)),
        Err(PortError::Conflict(_))
    ));
    port.release(task).expect("release");
    port.release(task).expect("a second release is quiet");
    assert!(port.claims().expect("claims").is_empty());
    assert_eq!(port.task(task).expect("task").label, Some(TaskLabel::Ready));
}

#[test]
fn labels_comments_and_one_report_per_instance() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = IssueNumber::from(2);
    port.label(task, TaskLabel::Done).expect("label");
    assert_eq!(port.task(task).expect("task").label, Some(TaskLabel::Done));
    port.comment(task, "first").expect("comment");
    port.comment(task, "second").expect("comment");
    assert_eq!(board.comments(task), ["first", "second"]);
    port.report(task, &instance(1), "working").expect("report");
    port.report(task, &instance(1), "done")
        .expect("report again");
    let expected_reports = vec![(instance(1), "done".to_owned())];
    assert_eq!(board.reports(task), expected_reports);
}

#[test]
fn questions_open_and_take_one_ruling() {
    let board = seeded();
    let port: &dyn Board = &board;
    let new_question = NewQuestion {
        on: IssueNumber::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: true,
    };
    let question = port.open_question(&new_question).expect("question");
    assert_eq!(question, IssueNumber::from(5));
    assert_eq!(
        port.task(question).expect("question ticket").label,
        Some(TaskLabel::Question)
    );
    port.record_ruling(question, "Title and author.")
        .expect("ruling");
    port.record_ruling(question, "Title and author.")
        .expect("the same ruling is quiet");
    assert!(matches!(
        port.record_ruling(question, "Title only."),
        Err(PortError::Conflict(_))
    ));
    let expected_question = Some((true, Some("Title and author.".to_owned())));
    assert_eq!(board.question(question), expected_question);
}

#[test]
fn open_questions_list_in_order_and_leave_the_waiting_task_out() {
    let board = seeded();
    let port: &dyn Board = &board;
    let ask = |on: u64, text: &str| NewQuestion {
        on: IssueNumber::from(on),
        text: text.to_owned(),
        door: false,
    };
    let first = ask(2, "Which columns?");
    let second = ask(4, "Which quoting?");
    let opened_first = port.open_question(&first).expect("first");
    let opened_second = port.open_question(&second).expect("second");
    port.label(IssueNumber::from(2), TaskLabel::Question)
        .expect("the task waits on its question");
    let listed: Vec<IssueNumber> = port
        .questions()
        .expect("questions")
        .iter()
        .map(|ticket| ticket.number)
        .collect();
    assert_eq!(
        listed,
        [opened_first, opened_second],
        "the waiting task is not a question"
    );
    port.record_ruling(opened_first, "Title and author.")
        .expect("ruling");
    let remaining = port.questions().expect("questions");
    assert_eq!(remaining.len(), 1);
    assert!(second.is_asked_by(&remaining[0]));
    assert!(!first.is_asked_by(&remaining[0]));
    let same_text_elsewhere = ask(3, "Which quoting?");
    assert!(!same_text_elsewhere.is_asked_by(&remaining[0]));
}
