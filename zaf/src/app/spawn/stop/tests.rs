use std::cell::RefCell;
use std::collections::BTreeSet;

use super::*;
use crate::domain::{Instance, InstanceNumber, RoleName, Ticket, TicketState};
use crate::fakes::FakeBoard;

const TASK: u64 = 13;
const INSTANCE: &str = "implement instance 3 on #1";

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

fn instance() -> Instance {
    Instance {
        role: RoleName::from("implement"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(3),
    }
}

/// Processes held in memory: the pids alive, whether they shrug off `SIGTERM`, and every signal
/// sent, with the pauses taken.
struct Table {
    alive: RefCell<BTreeSet<u32>>,
    stubborn: bool,
    sent: RefCell<Vec<(u32, Option<u32>, Signal)>>,
    paused: RefCell<Duration>,
}

impl Table {
    fn with(pids: &[u32], stubborn: bool) -> Self {
        Self {
            alive: RefCell::new(pids.iter().copied().collect()),
            stubborn,
            sent: RefCell::default(),
            paused: RefCell::default(),
        }
    }
}

impl Processes for Table {
    fn alive(&self, holder: &Holder) -> bool {
        self.alive.borrow().contains(&holder.pid)
    }

    fn group_of(&self, holder: &Holder) -> Option<u32> {
        self.alive(holder).then_some(900)
    }

    fn signal(&self, holder: &Holder, group: Option<u32>, signal: Signal) {
        self.sent.borrow_mut().push((holder.pid, group, signal));
        if signal == Signal::Kill || !self.stubborn {
            self.alive.borrow_mut().remove(&holder.pid);
        }
    }

    fn pause(&self, time: Duration) {
        *self.paused.borrow_mut() += time;
    }
}

/// A state folder with a phase run folder for `task`, held by `pid`, with `exit` recorded or not.
fn state_with_run(task: u64, pid: u32, exit: Option<&str>) -> tempfile::TempDir {
    let state = tempfile::tempdir().expect("temp dir");
    let run = state
        .path()
        .join(PHASES_DIR)
        .join("implement-instance-3-on-1-task-13");
    std::fs::create_dir_all(&run).expect("run folder");
    std::fs::write(run.join("task"), format!("{task}\n")).expect("task");
    std::fs::write(run.join("instance"), INSTANCE).expect("instance");
    std::fs::write(run.join("pid"), holder(pid).to_string()).expect("pid");
    if let Some(code) = exit {
        std::fs::write(run.join("exit"), code).expect("exit");
    }
    state
}

fn ticket() -> Ticket {
    Ticket {
        id: TicketId::from(TASK),
        epic: Some(TicketId::from(1)),
        title: "A task".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    }
}

/// A board with the task claimed by the run's instance.
fn claimed_board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket());
    board
        .claim(TicketId::from(TASK), &instance())
        .expect("claim");
    board
}

fn ready(board: &FakeBoard) -> Vec<TicketId> {
    board
        .ready()
        .expect("ready")
        .iter()
        .map(|ticket| ticket.id)
        .collect()
}

#[test]
fn ends_process_and_releases() {
    let state = state_with_run(TASK, 700, None);
    let board = claimed_board();
    let table = Table::with(&[700], false);
    assert!(ready(&board).is_empty(), "claimed, so not ready");

    let stopped = stop(TicketId::from(TASK), state.path(), &board, &table, GRACE).expect("stops");

    assert!(!table.alive(&holder(700)), "the process is gone");
    assert_eq!(stopped.runs.len(), 1);
    assert_eq!(stopped.runs[0].pid, 700);
    assert_eq!(stopped.runs[0].instance, INSTANCE);
    assert!(!stopped.runs[0].killed, "it ended on its own");
    assert_eq!(
        stopped.released,
        vec![Released {
            task: TicketId::from(TASK),
            instance: instance(),
        }]
    );
    assert_eq!(ready(&board), vec![TicketId::from(TASK)], "ready again");
    assert!(board.claims().expect("claims").is_empty());
    // The group is signalled with the holder.
    assert_eq!(
        table.sent.borrow().first(),
        Some(&(700, Some(900), Signal::Terminate))
    );
}

#[test]
fn no_instance_refused() {
    let board = claimed_board();

    // No run folder at all.
    let empty = tempfile::tempdir().expect("temp dir");
    let error = stop(
        TicketId::from(TASK),
        empty.path(),
        &board,
        &Table::with(&[700], false),
        GRACE,
    )
    .expect_err("nothing runs it");
    assert!(matches!(error, StopError::NoInstance(task) if task == TicketId::from(TASK)));
    assert!(
        error.to_string().contains("no instance is running"),
        "{error}"
    );

    // A run of another task, one that ended, and one whose process is gone.
    for (state, table) in [
        (state_with_run(99, 700, None), Table::with(&[700], false)),
        (
            state_with_run(TASK, 700, Some("0\n")),
            Table::with(&[700], false),
        ),
        (state_with_run(TASK, 700, None), Table::with(&[], false)),
    ] {
        let error = stop(TicketId::from(TASK), state.path(), &board, &table, GRACE)
            .expect_err("nothing live runs it");
        assert!(matches!(error, StopError::NoInstance(_)), "{error}");
        assert!(table.sent.borrow().is_empty(), "nothing was signalled");
    }
    // Nothing changed on the board: still claimed.
    assert!(ready(&board).is_empty());
    assert_eq!(board.claims().expect("claims").len(), 1);
}

#[test]
fn grace_then_kill() {
    let state = state_with_run(TASK, 700, None);
    let board = claimed_board();
    let table = Table::with(&[700], true);

    let stopped = stop(
        TicketId::from(TASK),
        state.path(),
        &board,
        &table,
        Duration::from_secs(1),
    )
    .expect("stops");

    assert!(stopped.runs[0].killed, "it ignored the terminate signal");
    let signals: Vec<Signal> = table.sent.borrow().iter().map(|sent| sent.2).collect();
    assert_eq!(signals.first(), Some(&Signal::Terminate));
    assert_eq!(signals.last(), Some(&Signal::Kill));
    // The grace was waited out before the kill, and no longer.
    assert_eq!(*table.paused.borrow(), Duration::from_secs(1));
    assert_eq!(ready(&board), vec![TicketId::from(TASK)]);
}

#[test]
fn a_survivor_keeps_the_claim() {
    struct Immortal;
    impl Processes for Immortal {
        fn alive(&self, _: &Holder) -> bool {
            true
        }
        fn group_of(&self, _: &Holder) -> Option<u32> {
            None
        }
        fn signal(&self, _: &Holder, _: Option<u32>, _: Signal) {}
        fn pause(&self, _: Duration) {}
    }
    let state = state_with_run(TASK, 700, None);
    let board = claimed_board();

    let error = stop(TicketId::from(TASK), state.path(), &board, &Immortal, GRACE)
        .expect_err("it cannot be ended");

    assert!(
        matches!(error, StopError::Survived { pid: 700, .. }),
        "{error}"
    );
    assert_eq!(
        board.claims().expect("claims").len(),
        1,
        "the claim is kept"
    );
}

#[test]
fn an_unclaimed_task_is_stopped_and_releases_nothing() {
    let state = state_with_run(TASK, 700, None);
    let board = FakeBoard::default();
    board.add_ticket(ticket());
    let table = Table::with(&[700], false);

    let stopped = stop(TicketId::from(TASK), state.path(), &board, &table, GRACE).expect("stops");

    assert!(stopped.released.is_empty());
    assert_eq!(stopped.runs.len(), 1);
}
