use super::*;
use crate::domain::{InstanceNumber, RoleName};
use crate::fakes::FakeBoard;

fn ticket(number: u64, label: TicketState) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: Some(TicketId::from(1)),
        title: format!("task {number}").into(),
        body: "".into(),
        state: Some(label),
        url: "".into(),
        kind: None,
    }
}

fn instance(number: u32) -> Instance {
    Instance {
        role: RoleName::from("programmer"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(number),
    }
}

fn seeded() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(2, TicketState::Ready));
    board.add_ticket(ticket(3, TicketState::Blocked));
    board.add_ticket(ticket(4, TicketState::Ready));
    board
}

#[test]
fn ready_lists_only_ready_tasks_in_order() {
    let board = seeded();
    let port: &dyn Board = &board;
    let numbers: Vec<TicketId> = port
        .ready()
        .expect("ready")
        .iter()
        .map(|ticket| ticket.id)
        .collect();
    assert_eq!(numbers, [TicketId::from(2), TicketId::from(4)]);
    assert!(matches!(
        port.task(TicketId::from(9)),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn claims_are_idempotent_for_the_holder_and_exclusive_for_others() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = TicketId::from(2);
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
        port.claim(TicketId::from(3), &instance(1)),
        Err(PortError::Rejected(_))
    ));
    assert_eq!(
        port.task(task).expect("task").state,
        Some(TicketState::Claimed)
    );
    assert!(
        port.ready()
            .expect("ready")
            .iter()
            .all(|ticket| ticket.id != task)
    );
}

#[test]
fn leases_beat_and_release_back_to_ready() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = TicketId::from(4);
    port.claim(task, &instance(1)).expect("claim");
    let first_beat = port.claims().expect("claims")[0].heartbeat;
    port.heartbeat(task, &instance(1)).expect("heartbeat");
    assert!(port.claims().expect("claims")[0].heartbeat >= first_beat);
    assert!(matches!(
        port.heartbeat(task, &instance(2)),
        Err(PortError::Conflict(_))
    ));
    port.release(task, &instance(2))
        .expect("a release by a non-holder is quiet");
    assert_eq!(port.claims().expect("claims").len(), 1);
    assert_eq!(
        port.task(task).expect("task").state,
        Some(TicketState::Claimed)
    );
    assert!(
        board.comments(task).is_empty(),
        "a non-holder writes nothing"
    );
    port.release(task, &instance(1)).expect("release");
    port.release(task, &instance(1))
        .expect("a second release is quiet");
    assert!(port.claims().expect("claims").is_empty());
    assert_eq!(
        port.task(task).expect("task").state,
        Some(TicketState::Ready)
    );
    let comments = board.comments(task);
    assert_eq!(comments.len(), 1, "only the first release comments");
    assert!(
        comments[0].starts_with("released from programmer instance 1 on #1 at "),
        "{comments:?}"
    );
}

#[test]
fn a_release_keeps_question_and_done_and_adds_no_ready() {
    let board = seeded();
    let port: &dyn Board = &board;
    for (task, label) in [(2, TicketState::Question), (4, TicketState::Done)] {
        let task = TicketId::from(task);
        port.claim(task, &instance(1)).expect("claim");
        port.set_state(task, label).expect("label");
        port.release(task, &instance(1)).expect("release");
        assert_eq!(port.task(task).expect("task").state, Some(label));
        assert!(port.claims().expect("claims").is_empty());
        assert_eq!(board.comments(task).len(), 1);
    }
    assert!(port.ready().expect("ready").is_empty());
}

#[test]
fn labels_comments_and_one_report_per_instance() {
    let board = seeded();
    let port: &dyn Board = &board;
    let task = TicketId::from(2);
    port.set_state(task, TicketState::Done).expect("label");
    assert_eq!(
        port.task(task).expect("task").state,
        Some(TicketState::Done)
    );
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
        on: TicketId::from(2),
        text: "Which columns does the export carry?".to_owned(),
        door: true,
    };
    let question = port.open_question(&new_question).expect("question");
    assert_eq!(question, TicketId::from(5));
    assert_eq!(
        port.task(question).expect("question ticket").state,
        Some(TicketState::Question)
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
fn a_question_body_names_the_issue_it_was_raised_on() {
    let asked = NewQuestion {
        on: TicketId::from(12),
        text: "Which columns?".to_owned(),
        door: false,
    };
    assert_eq!(
        NewQuestion::raised_on(&asked.body()),
        Some(TicketId::from(12))
    );
    let with_newline = format!("{}\n", asked.body());
    assert_eq!(
        NewQuestion::raised_on(&with_newline),
        Some(TicketId::from(12))
    );
    assert_eq!(NewQuestion::raised_on(""), None);
    assert_eq!(NewQuestion::raised_on("Goal: export the list"), None);
    assert_eq!(NewQuestion::raised_on("Raised on #x."), None);
}

#[test]
fn a_question_body_followed_by_its_text_still_names_the_issue() {
    let asked = NewQuestion {
        on: TicketId::from(12),
        text: "Which columns?\nAnd which order?".to_owned(),
        door: false,
    };
    assert_eq!(
        asked.body(),
        "Raised on #12.\n\nWhich columns?\nAnd which order?"
    );
    assert_eq!(
        NewQuestion::raised_on(&asked.body()),
        Some(TicketId::from(12))
    );
    let ticket = Ticket {
        id: TicketId::from(30),
        epic: None,
        title: "Which columns?".into(),
        body: asked.body().into(),
        state: None,
        url: "".into(),
        kind: None,
    };
    assert_eq!(NewQuestion::text_of(&ticket), asked.text);
    assert!(asked.is_asked_by(&ticket));
}

#[test]
fn a_question_with_no_word_break_is_cut_at_the_limit() {
    let asked = NewQuestion {
        on: TicketId::from(12),
        text: "x".repeat(QUESTION_TITLE_LIMIT + 50),
        door: false,
    };
    assert_eq!(asked.title().chars().count(), QUESTION_TITLE_LIMIT);
}

#[test]
fn a_ruling_returns_the_waiting_task_to_ready_and_names_it() {
    let board = seeded();
    let port: &dyn Board = &board;
    let ask = |on: u64| NewQuestion {
        on: TicketId::from(on),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let waiting = port.open_question(&ask(2)).expect("question");
    port.set_state(TicketId::from(2), TicketState::Question)
        .expect("the task waits");
    let finished = port.open_question(&ask(4)).expect("question");
    port.set_state(TicketId::from(4), TicketState::Done)
        .expect("the task is done");
    assert_eq!(
        port.record_ruling(waiting, "Title."),
        Ok(Some(TicketId::from(2)))
    );
    assert_eq!(
        port.task(TicketId::from(2)).expect("task").state,
        Some(TicketState::Ready)
    );
    assert_eq!(
        port.record_ruling(waiting, "Title."),
        Ok(None),
        "the same ruling again changes nothing"
    );
    assert_eq!(port.record_ruling(finished, "Title."), Ok(None));
    assert_eq!(
        port.task(TicketId::from(4)).expect("task").state,
        Some(TicketState::Done)
    );
}

#[test]
fn open_questions_list_in_order_and_leave_the_waiting_task_out() {
    let board = seeded();
    let port: &dyn Board = &board;
    let ask = |on: u64, text: &str| NewQuestion {
        on: TicketId::from(on),
        text: text.to_owned(),
        door: false,
    };
    let first = ask(2, "Which columns?");
    let second = ask(4, "Which quoting?");
    let opened_first = port.open_question(&first).expect("first");
    let opened_second = port.open_question(&second).expect("second");
    port.set_state(TicketId::from(2), TicketState::Question)
        .expect("the task waits on its question");
    let listed: Vec<TicketId> = port
        .questions()
        .expect("questions")
        .iter()
        .map(|ticket| ticket.id)
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

#[test]
fn the_fake_tells_the_four_issue_states_apart() {
    let board = seeded();
    board.add_ticket(ticket(5, TicketState::Done));
    board.add_ticket(ticket(6, TicketState::Blocked));
    board.close_not_planned(TicketId::from(6));
    board.add_ticket(ticket(7, TicketState::Blocked));
    board.close(TicketId::from(7));
    let port: &dyn Board = &board;
    let state = |number: u64| port.standing(TicketId::from(number));

    assert_eq!(state(3), Ok(Standing::Open));
    assert_eq!(state(5), Ok(Standing::Done));
    assert_eq!(state(6), Ok(Standing::ClosedNotPlanned));
    assert_eq!(
        state(7),
        Ok(Standing::Done),
        "closed as completed, no label"
    );
    assert_eq!(state(99), Ok(Standing::Missing));
}

/// A board that implements only what [`Board::standing`]'s default reads: `task`.
struct TaskOnly(Vec<Ticket>);

impl Board for TaskOnly {
    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        match number.get() {
            13 => Err(PortError::Backend("gh: 502".to_owned())),
            _ => self
                .0
                .iter()
                .find(|ticket| ticket.id == number)
                .cloned()
                .ok_or_else(|| PortError::NotFound(number.to_string())),
        }
    }

    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        unreachable!("not read")
    }

    fn claim(&self, _: TicketId, _: &Instance) -> Result<ClaimOutcome, PortError> {
        unreachable!("not read")
    }

    fn heartbeat(&self, _: TicketId, _: &Instance) -> Result<(), PortError> {
        unreachable!("not read")
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        unreachable!("not read")
    }

    fn release(&self, _: TicketId, _: &Instance) -> Result<(), PortError> {
        unreachable!("not read")
    }

    fn set_state(&self, _: TicketId, _: TicketState) -> Result<(), PortError> {
        unreachable!("not read")
    }

    fn comment(&self, _: TicketId, _: &str) -> Result<(), PortError> {
        unreachable!("not read")
    }

    fn report(&self, _: TicketId, _: &Instance, _: &str) -> Result<(), PortError> {
        unreachable!("not read")
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        unreachable!("not read")
    }

    fn open_question(&self, _: &NewQuestion) -> Result<TicketId, PortError> {
        unreachable!("not read")
    }

    fn record_ruling(&self, _: TicketId, _: &str) -> Result<Option<TicketId>, PortError> {
        unreachable!("not read")
    }
}

#[test]
fn the_default_issue_state_reads_the_task_and_never_calls_a_missing_one_closed() {
    let board = TaskOnly(vec![
        ticket(3, TicketState::Blocked),
        ticket(4, TicketState::Done),
    ]);

    assert_eq!(board.standing(TicketId::from(3)), Ok(Standing::Open));
    assert_eq!(board.standing(TicketId::from(4)), Ok(Standing::Done));
    assert_eq!(board.standing(TicketId::from(9)), Ok(Standing::Missing));
    assert_eq!(
        board.standing(TicketId::from(13)),
        Err(PortError::Backend("gh: 502".to_owned())),
        "a backend failure is not a dead blocker"
    );
}
