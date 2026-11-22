//! Each epic's line in the state: its tasks, their sizes, blockers, questions and phases.

use super::*;
use crate::app::schedule::gather::Gathered;
use crate::domain::schedule::{
    ActionName, ExitCode, HeadEnd, Phase, Place, RunExit, RunRecord, TicketFacts,
};
use crate::domain::{PhaseEnd, TaskSize};
use crate::ports::NewQuestion;

/// A task body that follows the template, with `size` and the `Blocked by:` line given.
fn sized(size: &str, blocked_by: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
         Model: sonnet\nSize: {size}\nBlocked by: {blocked_by}\n"
    )
}

/// A task of the epic with `body`.
fn with_body(number: u64, title: &str, state: TicketState, body: String) -> Ticket {
    Ticket {
        body: body.into(),
        ..task(number, title, state)
    }
}

/// The tasks of the only epic.
fn line_of(state: &ProjectState) -> &[LineTask] {
    &state.epics[0].tasks
}

#[test]
fn line_tasks_in_id_order_with_sizes() {
    let world = World::new();
    world.board.add_ticket(with_body(
        3,
        "Serve",
        TicketState::Ready,
        "not a task".to_owned(),
    ));
    world
        .board
        .add_ticket(with_body(2, "Write", TicketState::Ready, sized("M", "—")));
    world
        .board
        .add_ticket(with_body(1, "Read", TicketState::Ready, sized("XS", "—")));

    let state = world.gather(at(1_000));

    let shown: Vec<(u64, Option<&str>)> = line_of(&state)
        .iter()
        .map(|task| (task.number, task.size))
        .collect();
    assert_eq!(shown, [(1, Some("XS")), (2, Some("M")), (3, None)]);
    let first = &line_of(&state)[0];
    assert_eq!(first.title, "Read");
    assert_eq!(
        first.issue_url.as_deref(),
        Some("https://board.invalid/tickets/1")
    );
    // Nothing is placed here, so the line reads its label.
    assert_eq!(first.state, LineState::NotStarted);
    assert_eq!((first.phases_done, first.phases_total), (0, 0));
    assert_eq!(first.phase, None);
}

#[test]
fn line_tasks_leave_out_not_planned() {
    let world = World::new();
    world.board.add_ticket(task(1, "Kept", TicketState::Ready));
    world
        .board
        .add_ticket(task(2, "Dropped", TicketState::Ready));
    world.board.close_not_planned(TicketId::from(2));

    let state = world.gather(at(1_000));

    let numbers: Vec<u64> = line_of(&state).iter().map(|task| task.number).collect();
    assert_eq!(numbers, [1]);
}

#[test]
fn line_task_names_its_blockers() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Ready));
    world.board.add_ticket(with_body(
        2,
        "Write",
        TicketState::Blocked,
        sized("S", "#1, #9"),
    ));

    let state = world.gather(at(1_000));

    assert_eq!(
        line_of(&state)[1].blocked_by,
        [
            LineBlocker {
                number: 1,
                title: Some("Read".to_owned()),
                state: LineState::NotStarted,
            },
            LineBlocker {
                number: 9,
                title: None,
                state: LineState::NotStarted,
            },
        ]
    );
    assert_eq!(line_of(&state)[1].state, LineState::Blocked);
}

#[test]
fn line_task_carries_its_question() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Ready));
    world.board.add_ticket(task(2, "Write", TicketState::Ready));
    let asked = world
        .board
        .open_question(&NewQuestion {
            on: TicketId::from(2),
            text: "Which colour?".to_owned(),
            door: false,
        })
        .expect("question");

    let state = world.gather(at(1_000));

    assert_eq!(line_of(&state)[0].question, None);
    assert_eq!(
        line_of(&state)[1].question,
        Some(LineQuestion {
            number: asked.get(),
            text: "Which colour?".to_owned(),
        })
    );
    assert_eq!(state.epics[0].line_question, None);
}

#[test]
fn line_epic_question_colours_the_epic_alone() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Ready));
    let asked = world
        .board
        .open_question(&NewQuestion {
            on: TicketId::from(EPIC),
            text: "Drop the old page?".to_owned(),
            door: true,
        })
        .expect("question");

    let state = world.gather(at(1_000));

    let epic = &state.epics[0];
    assert_eq!(epic.line_state, LineState::Question);
    assert_eq!(
        epic.line_question,
        Some(LineQuestion {
            number: asked.get(),
            text: "Drop the old page?".to_owned(),
        })
    );
    assert_eq!(line_of(&state)[0].state, LineState::NotStarted);
    assert_eq!(line_of(&state)[0].question, None);
}

#[test]
fn line_epic_state_follows_its_tasks() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Done));
    world.board.add_ticket(task(2, "Write", TicketState::Ready));
    world
        .board
        .add_ticket(task(3, "Dropped", TicketState::Ready));
    world.board.close_not_planned(TicketId::from(3));

    // One done and one never started: the epic has begun.
    assert_eq!(
        world.gather(at(1_000)).epics[0].line_state,
        LineState::WaitingTick
    );

    // Closed as completed: every task that counts is done.
    world.board.close(TicketId::from(2));
    assert_eq!(world.gather(at(1_000)).epics[0].line_state, LineState::Done);

    let idle = World::new();
    idle.board.add_ticket(task(1, "Read", TicketState::Ready));
    assert_eq!(
        idle.gather(at(1_000)).epics[0].line_state,
        LineState::NotStarted
    );
    let working = World::new();
    working
        .board
        .add_ticket(task(1, "Read", TicketState::Claimed));
    assert_eq!(
        working.gather(at(1_000)).epics[0].line_state,
        LineState::Working
    );
}

#[test]
fn line_task_counts_phases_ended() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Ready));
    let phase = |name: &str, end| Phase {
        name: ActionName::from(name),
        ends: vec![end],
        on_fail: None,
    };
    let implemented = RunRecord {
        action: ActionName::from("implement"),
        head: None,
        head_end: HeadEnd::Left(None),
        started_at: at(5),
        alive: false,
        exit: Some(RunExit {
            code: ExitCode::from(0),
            at: at(6),
        }),
        questions: Default::default(),
        last_line: None,
    };
    let facts = TicketFacts {
        ticket: TicketId::from(1),
        epic: None,
        open_questions: Default::default(),
        dependencies: Vec::new(),
        pull: None,
        runs: vec![implemented],
        phases: vec![
            phase("implement", PhaseEnd::Exit),
            phase("review", PhaseEnd::ReviewPosted),
            phase("merge", PhaseEnd::Merged),
        ],
        size: TaskSize::S,
        priority: None,
        ruled: Vec::new(),
        ready_since: None,
        merging: Default::default(),
        no_check_ruled: None,
        trees: Default::default(),
    };
    let gathered = Gathered {
        tickets: vec![facts],
        ..Gathered::default()
    };
    let places: Places = [(
        TicketId::from(1),
        Place::InLane {
            next: ActionName::from("review"),
            since: at(6),
        },
    )]
    .into();

    let state = gather_state(&StateInputs {
        places: Some(&places),
        gathered: Some(&gathered),
        ..world.inputs(at(1_000))
    });

    let task = &line_of(&state)[0];
    assert_eq!((task.phases_done, task.phases_total), (1, 3));
    assert_eq!(task.phase.as_deref(), Some("review"));
    // A phase has ended and nothing runs: it waits for the next tick.
    assert_eq!(task.state, LineState::WaitingTick);
}

#[test]
fn line_task_done_keeps_its_size() {
    let world = World::new();
    world
        .board
        .add_ticket(with_body(1, "Read", TicketState::Done, sized("L", "—")));
    let gathered = Gathered {
        phase_totals: [("Task".to_owned(), 4)].into(),
        ..Gathered::default()
    };

    let state = gather_state(&StateInputs {
        gathered: Some(&gathered),
        ..world.inputs(at(1_000))
    });

    let task = &line_of(&state)[0];
    assert_eq!(task.size, Some("L"));
    assert_eq!(task.state, LineState::Done);
    assert_eq!((task.phases_done, task.phases_total), (4, 4));
    assert_eq!(task.phase, None);
}

/// A second epic, #190, challenged, with one done task #191.
fn finished_epic(world: &World) {
    let epic = TicketId::from(190);
    world.board.add_challenged_epic(epic, "The finished epic");
    world.board.add_ticket(Ticket {
        epic: Some(epic),
        ..task(191, "Built", TicketState::Done)
    });
}

#[test]
fn hub_line_clears_finished_epic_blocker() {
    let world = World::new();
    finished_epic(&world);
    world.board.add_ticket(with_body(
        2,
        "Write",
        TicketState::Blocked,
        sized("S", "#190"),
    ));

    let state = world.gather(at(1_000));

    let write = state.epics[0]
        .tasks
        .iter()
        .find(|task| task.number == 2)
        .expect("the task is on its line");
    assert_eq!(
        write.blocked_by,
        [LineBlocker {
            number: 190,
            title: Some("The finished epic".to_owned()),
            state: LineState::Done,
        }],
        "an open epic whose tasks are all done is not waited on"
    );
    assert!(
        state.epics.iter().any(|epic| epic.number == 190),
        "the epic stays open"
    );
}

#[test]
fn hub_line_keeps_an_unfinished_epic_blocker_waiting() {
    let world = World::new();
    finished_epic(&world);
    world.board.add_ticket(Ticket {
        epic: Some(TicketId::from(190)),
        ..task(192, "Still open", TicketState::Ready)
    });
    world.board.add_ticket(with_body(
        2,
        "Write",
        TicketState::Blocked,
        sized("S", "#190"),
    ));

    let state = world.gather(at(1_000));

    let write = state.epics[0]
        .tasks
        .iter()
        .find(|task| task.number == 2)
        .expect("the task is on its line");
    assert_eq!(write.blocked_by[0].state, LineState::NotStarted);
}

/// A second open epic, #190, with the tasks #191 to #193.
fn second_epic(world: &World) {
    let epic = TicketId::from(190);
    world.board.add_challenged_epic(epic, "The second epic");
    for number in 191..=193 {
        world.board.add_ticket(Ticket {
            epic: Some(epic),
            ..with_body(number, "Other", TicketState::Ready, sized("S", "—"))
        });
    }
}

#[test]
fn line_reads_no_task_for_epic_tasks() {
    let world = World::new();
    second_epic(&world);
    for number in 1..=3 {
        world.board.add_ticket(with_body(
            number,
            "Mine",
            TicketState::Ready,
            sized("M", "#191"),
        ));
    }
    world.board.reset_calls();

    let state = world.gather(at(1_000));

    assert_eq!(state.epics.len(), 2);
    let tasks: usize = state.epics.iter().map(|epic| epic.tasks.len()).sum();
    assert_eq!(tasks, 6);
    assert_eq!(world.board.calls("task"), 0);
}

#[test]
fn line_reads_outside_blocker_once() {
    let world = World::new();
    // #900 is no task of any epic.
    world.board.add_ticket(Ticket {
        epic: None,
        ..task(900, "Outside", TicketState::Done)
    });
    for number in 1..=2 {
        world.board.add_ticket(with_body(
            number,
            "Mine",
            TicketState::Ready,
            sized("S", "#900"),
        ));
    }
    world.board.reset_calls();

    let state = world.gather(at(1_000));

    // Asked once for where it stands, never as a task.
    assert_eq!(world.board.calls("standing"), 1);
    assert_eq!(world.board.calls("task"), 0);
    let blocker = &line_of(&state)[0].blocked_by[0];
    assert_eq!((blocker.number, blocker.state), (900, LineState::Done));
}

#[test]
fn line_task_fields_unchanged() {
    let world = World::new();
    world
        .board
        .add_ticket(with_body(1, "Read", TicketState::Ready, sized("XS", "—")));
    world.board.add_ticket(with_body(
        2,
        "Write",
        TicketState::Blocked,
        sized("L", "#1, #9"),
    ));
    world.board.add_ticket(with_body(
        3,
        "Serve",
        TicketState::Ready,
        "not a task".to_owned(),
    ));

    let state = world.gather(at(1_000));

    let shown: Vec<_> = line_of(&state)
        .iter()
        .map(|task| {
            let blockers: Vec<u64> = task.blocked_by.iter().map(|b| b.number).collect();
            (task.number, task.size, blockers, task.issue_url.clone())
        })
        .collect();
    let link = |number: u64| Some(format!("https://board.invalid/tickets/{number}"));
    assert_eq!(
        shown,
        [
            (1, Some("XS"), vec![], link(1)),
            (2, Some("L"), vec![1, 9], link(2)),
            (3, None, vec![], link(3)),
        ]
    );
}
