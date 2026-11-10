use std::time::UNIX_EPOCH;

use super::*;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName, Ticket, TicketState};
use crate::fakes::FakeBoard;

const EPIC: u64 = 10;
const AT: &str = "2026-11-09T08:00:00Z";

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(EPIC), "tickets");
    board.set_body(TicketId::from(EPIC), "Goal: one ticket shape.");
    board
}

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

fn launch(number: u32) -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(EPIC),
            number: InstanceNumber::from(number),
        },
        tasks: vec![TicketId::from(11)],
    }
}

fn all_alive(_: &Holder) -> bool {
    true
}

/// Signs as a process whose chain is `ancestors`, over the run folders in `dir`.
fn sign_as(board: &FakeBoard, dir: &Path, ancestors: &[Holder]) -> Result<SignOutcome, EpicError> {
    let guard = SignGuard {
        state_dir: dir,
        ancestors,
        alive: &all_alive,
    };
    sign(board, TicketId::from(EPIC), &guard, UNIX_EPOCH)
}

#[test]
fn the_engineer_signs_an_epic_once_per_body() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    let signed = sign_as(&board, dir.path(), &[holder(1)]).expect("signs");
    let SignOutcome::Signed(hash) = signed else {
        panic!("expected a signature, got {signed:?}");
    };
    let epic = board.epic(TicketId::from(EPIC)).expect("the epic");
    assert!(epic.signed());
    assert_eq!(epic.stage(), EpicStage::Signed);
    let again = sign_as(&board, dir.path(), &[holder(1)]).expect("signs again");
    assert_eq!(again, SignOutcome::AlreadySigned(hash.clone()));

    board.set_body(
        TicketId::from(EPIC),
        "Goal: one ticket shape, over Projects too.",
    );
    let lines = list(&board).expect("lists");
    assert_eq!(lines[0].stage, EpicStage::Unsigned);
    assert!(lines[0].signature_void);
    assert!(lines[0].render().contains("unsigned (edited since signed)"));
    let resigned = sign_as(&board, dir.path(), &[holder(1)]).expect("signs the edit");
    assert!(matches!(resigned, SignOutcome::Signed(new) if new != hash));
}

#[test]
fn an_instance_above_this_process_may_not_sign() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    runs::reserve(dir.path(), &launch(3), &holder(4242), AT).expect("reserves");
    let refused = sign_as(&board, dir.path(), &[holder(5000), holder(4242), holder(1)]);
    assert!(
        matches!(&refused, Err(EpicError::InsideInstance { instance }) if instance == "team instance 3 on #10"),
        "got {refused:?}"
    );
    assert!(
        board
            .epic(TicketId::from(EPIC))
            .expect("the epic")
            .signature
            .is_none()
    );
}

#[test]
fn a_launch_that_is_not_above_or_not_running_does_not_stop_a_sign() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    let ended = runs::reserve(dir.path(), &launch(3), &holder(4242), AT).expect("reserves");
    runs::reserve(dir.path(), &launch(4), &holder(7777), AT).expect("reserves");
    runs::finish(&ended, "0").expect("ends");
    let signed = sign_as(&board, dir.path(), &[holder(5000), holder(4242), holder(1)]);
    assert!(
        matches!(signed, Ok(SignOutcome::Signed(_))),
        "got {signed:?}"
    );
}

#[test]
fn a_ticket_that_is_no_open_epic_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    board.add_ticket(Ticket {
        id: TicketId::from(11),
        epic: Some(TicketId::from(EPIC)),
        title: "a task".into(),
        body: "Goal: g".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
    });
    let guard = SignGuard {
        state_dir: dir.path(),
        ancestors: &[],
        alive: &all_alive,
    };
    let refused = sign(&board, TicketId::from(11), &guard, UNIX_EPOCH);
    assert!(matches!(refused, Err(EpicError::NotAnEpic(id)) if id == TicketId::from(11)));
}

#[test]
fn the_list_shows_each_open_epic_with_its_stage_and_tally() {
    let board = board();
    let lines = list(&board).expect("lists");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].render(), "#10 unsigned 0 of 0  tickets");
    let printed = serde_json::to_value(&lines[0]).expect("serializes");
    assert_eq!(printed["number"], 10);
    assert_eq!(printed["stage"], "unsigned");
}

#[test]
fn show_lists_the_epics_tasks_closed_ones_too_in_id_order() {
    let board = board();
    for (id, title, state) in [
        (12, "second", TicketState::Blocked),
        (11, "first", TicketState::Ready),
    ] {
        board.add_ticket(Ticket {
            id: TicketId::from(id),
            epic: Some(TicketId::from(EPIC)),
            title: title.into(),
            body: "Goal: g".into(),
            state: Some(state),
            url: "".into(),
        });
    }
    board.set_group(TicketId::from(11), "export");
    board.close_not_planned(TicketId::from(12));
    let view = show(&board, TicketId::from(EPIC)).expect("shows");
    assert_eq!(
        view.render(),
        "#10 unsigned 0 of 1  tickets\n  #11 ready [export]  first\n  #12 closed not planned  second"
    );
    let printed = serde_json::to_value(&view).expect("serializes");
    assert_eq!(printed["number"], 10);
    assert_eq!(printed["tasks"][0]["group"], "export");
    assert_eq!(printed["tasks"][1]["closed"], "not_planned");
}

#[test]
fn show_refuses_a_ticket_that_is_no_open_epic() {
    let board = board();
    let refused = show(&board, TicketId::from(99));
    assert!(matches!(refused, Err(EpicError::NotAnEpic(id)) if id == TicketId::from(99)));
}
