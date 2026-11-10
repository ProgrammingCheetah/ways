use super::*;
use crate::app::status::Launch;
use crate::domain::{EpicTask, Signature, Standing, TicketState};

fn task(number: u64, epic: u64, body: &str) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: Some(TicketId::from(epic)),
        title: format!("Task {number}").into(),
        body: body.into(),
        state: Some(TicketState::Ready),
        url: "".into(),
    }
}

/// An open epic in `stage`'s conditions, with `tasks` as its open tasks.
fn epic_in(number: u64, signed: bool, challenged: bool, tasks: &[u64]) -> Epic {
    let body: crate::domain::TicketBody = format!("Goal: epic {number}").into();
    Epic {
        id: TicketId::from(number),
        title: format!("Epic {number}").into(),
        signature: signed.then(|| Signature::of(&body, SystemTime::UNIX_EPOCH)),
        body,
        tasks: tasks
            .iter()
            .map(|task| EpicTask {
                number: TicketId::from(*task),
                title: format!("Task {task}").into(),
                state: Some(TicketState::Ready),
                standing: Standing::Open,
                group: None,
            })
            .collect(),
        challenged,
    }
}

/// The ready tasks, each of its epic signed and challenged, so the teams may build them.
fn snapshot(ready: Vec<Ticket>) -> Snapshot {
    let mut epics: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for ticket in &ready {
        if let Some(epic) = ticket.epic {
            epics.entry(epic.get()).or_default().push(ticket.id.get());
        }
    }
    Snapshot {
        ready,
        epics: epics
            .iter()
            .map(|(epic, tasks)| epic_in(*epic, true, true, tasks))
            .collect(),
        ..Snapshot::default()
    }
}

fn settings(slots: u32) -> Settings {
    Settings { slots, batch: 4 }
}

fn running(task: u64, instance: &str) -> Launch {
    Launch {
        task: Some(task),
        instance: instance.to_owned(),
        pid: Some(4242),
        started_at: None,
    }
}

fn numbers(values: &[u64]) -> Vec<TicketId> {
    values.iter().copied().map(TicketId::from).collect()
}

#[test]
fn no_free_slot_launches_nothing() {
    let state = SpawnState {
        launches: vec![running(30, "team instance 1 on #20")],
        ..SpawnState::default()
    };
    let plan = plan(
        &snapshot(vec![task(3, 2, "")]),
        &state,
        Level::Green,
        settings(1),
    );
    assert!(plan.launches.is_empty());
    assert_eq!(plan.slots.free, 0);
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::NoFreeSlot)]);
}

#[test]
fn a_blocked_task_is_skipped_with_the_blocker_named() {
    let mut board = snapshot(vec![
        task(3, 2, "Goal: rows\nBlocked by: #7, #8"),
        task(4, 2, "Blocked by: —"),
    ]);
    board.closed.insert(TicketId::from(8));
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(
        plan.tasks,
        [(TicketId::from(3), TaskSkip::Blocked(numbers(&[7])))]
    );
    assert_eq!(plan.launches.len(), 1);
    assert_eq!(plan.launches[0].tasks, numbers(&[4]));
}

#[test]
fn an_epic_whose_tasks_are_all_blocked_launches_nothing() {
    let board = snapshot(vec![task(3, 2, "Blocked by: #7")]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(
        plan.epics,
        [(TicketId::from(2), EpicSkip::NothingLaunchable)]
    );
}

#[test]
fn two_epics_and_one_slot_launch_one() {
    let board = snapshot(vec![task(3, 2, ""), task(13, 12, "")]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(1));
    assert_eq!(plan.launches.len(), 1);
    assert_eq!(plan.launches[0].instance.epic, TicketId::from(2));
    assert_eq!(plan.epics, [(TicketId::from(12), EpicSkip::NoFreeSlot)]);
}

#[test]
fn one_launch_per_epic_with_at_most_a_batch_of_tasks() {
    let ready = (3..=8).map(|number| task(number, 2, "")).collect();
    let state = SpawnState {
        next_instance: 12,
        ..SpawnState::default()
    };
    let plan = plan(&snapshot(ready), &state, Level::Green, settings(2));
    assert_eq!(plan.launches.len(), 1);
    let launch = &plan.launches[0];
    assert_eq!(launch.instance.to_string(), "team instance 12 on #2");
    assert_eq!(launch.tasks, numbers(&[3, 4, 5, 6]));
}

#[test]
fn the_guard_levels_change_the_slot_count() {
    assert_eq!(allowed_slots(4, Level::Green), 4);
    assert_eq!(allowed_slots(4, Level::Yellow), 2);
    assert_eq!(allowed_slots(1, Level::Yellow), 1);
    assert_eq!(allowed_slots(3, Level::Yellow), 1);
    assert_eq!(allowed_slots(4, Level::Orange), 1);
    assert_eq!(allowed_slots(4, Level::Red), 0);

    let board = snapshot(vec![task(3, 2, ""), task(13, 12, ""), task(23, 22, "")]);
    let launched = |level| {
        plan(&board, &SpawnState::default(), level, settings(4))
            .launches
            .len()
    };
    assert_eq!(launched(Level::Green), 3);
    assert_eq!(launched(Level::Yellow), 2);
    assert_eq!(launched(Level::Orange), 1);
    assert_eq!(launched(Level::Red), 0);
}

#[test]
fn a_red_guard_names_itself_as_the_reason() {
    let board = snapshot(vec![task(3, 2, "")]);
    let plan = plan(&board, &SpawnState::default(), Level::Red, settings(2));
    assert_eq!(plan.slots.allowed, 0);
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::GuardRed)]);
}

#[test]
fn a_claimed_task_is_never_launched() {
    let mut board = snapshot(vec![task(3, 2, ""), task(4, 2, "")]);
    board.claimed.insert(TicketId::from(3));
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(plan.tasks, [(TicketId::from(3), TaskSkip::Claimed)]);
    assert_eq!(plan.launches[0].tasks, numbers(&[4]));

    board.claimed.insert(TicketId::from(4));
    let plan = super::plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
}

#[test]
fn a_task_a_running_instance_carries_is_not_launched_again() {
    let state = SpawnState {
        launches: vec![running(3, "team instance 1 on #2")],
        ..SpawnState::default()
    };
    let board = snapshot(vec![task(3, 2, ""), task(4, 2, "")]);
    let plan = plan(&board, &state, Level::Green, settings(2));
    assert_eq!(
        plan.tasks,
        [(
            TicketId::from(3),
            TaskSkip::Running("team instance 1 on #2".to_owned())
        )]
    );
    assert_eq!(plan.launches[0].tasks, numbers(&[4]));
}

#[test]
fn a_batch_holds_one_slot() {
    let state = SpawnState {
        launches: vec![
            running(30, "team instance 1 on #20"),
            running(31, "team instance 1 on #20"),
        ],
        ..SpawnState::default()
    };
    let plan = plan(
        &snapshot(vec![task(3, 2, "")]),
        &state,
        Level::Green,
        settings(2),
    );
    assert_eq!(plan.slots.running, 1);
    assert_eq!(plan.launches.len(), 1);
}

#[test]
fn a_second_epic_launches_beside_a_running_two_task_instance() {
    let state = SpawnState {
        launches: vec![
            running(46, "team instance 3 on #37"),
            running(47, "team instance 3 on #37"),
        ],
        next_instance: 4,
    };
    let board = snapshot(vec![task(60, 50, "")]);
    let plan = plan(&board, &state, Level::Green, settings(2));
    assert_eq!(plan.slots.running, 1);
    assert_eq!(plan.slots.free, 1);
    assert_eq!(plan.launches.len(), 1);
    assert_eq!(
        plan.launches[0].instance.to_string(),
        "team instance 4 on #50"
    );
    assert!(plan.epics.is_empty());
}

#[test]
fn a_task_without_an_epic_is_skipped() {
    let mut orphan = task(3, 2, "");
    orphan.epic = None;
    let plan = plan(
        &snapshot(vec![orphan]),
        &SpawnState::default(),
        Level::Green,
        settings(2),
    );
    assert!(plan.launches.is_empty());
    assert_eq!(plan.tasks, [(TicketId::from(3), TaskSkip::NoEpic)]);
}

#[test]
fn tick_says_in_a_blocker_cycle() {
    let plan = plan(
        &snapshot(vec![
            task(3, 2, "Blocked by: #4"),
            task(4, 2, "Blocked by: #3"),
            task(5, 2, "Blocked by: —"),
        ]),
        &SpawnState::default(),
        Level::Green,
        settings(2),
    );
    assert_eq!(
        plan.tasks,
        [
            (TicketId::from(3), TaskSkip::InCycle(numbers(&[3, 4]))),
            (TicketId::from(4), TaskSkip::InCycle(numbers(&[3, 4]))),
        ]
    );
    assert_eq!(
        plan.launches[0].tasks,
        numbers(&[5]),
        "the rest still launch"
    );
    let text = plan.render_text();
    assert!(text.contains("#3 skipped: in a blocker cycle (#3 → #4 → #3)\n"));
    assert!(text.contains("#4 skipped: in a blocker cycle (#3 → #4 → #3)\n"));
    assert!(
        !text.contains("blocker cycle: "),
        "a cycle whose tasks are skipped is named once, on them"
    );
}

#[test]
fn a_task_whose_blocker_is_in_a_cycle_says_so() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7 #9")]);
    board.open.insert(TicketId::from(7), numbers(&[8]));
    board.open.insert(TicketId::from(8), numbers(&[7]));
    board.open.insert(TicketId::from(9), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(
        plan.tasks,
        [(TicketId::from(3), TaskSkip::Blocked(numbers(&[7, 9])))]
    );
    assert_eq!(plan.cycles, [numbers(&[7, 8])]);
    let text = plan.render_text();
    assert!(text.contains("#3 skipped: blocked by #7 (in a blocker cycle) #9\n"));
    assert!(text.contains("blocker cycle: #7 → #8 → #7\n"));
}

#[test]
fn a_plain_open_blocker_reads_blocked_by_alone() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7")]);
    board.open.insert(TicketId::from(7), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.cycles.is_empty());
    assert!(plan.dead.is_empty());
    assert!(plan.render_text().contains("#3 skipped: blocked by #7\n"));
}

#[test]
fn the_plan_names_dead_blockers_in_text_and_json() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7 #8")]);
    board.dead.insert(TicketId::from(7), DeadReason::NotAnIssue);
    board
        .dead
        .insert(TicketId::from(8), DeadReason::ClosedNotPlanned);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    let text = plan.render_text();
    assert!(text.contains("dead blocker #7 on #3: not an issue\n"));
    assert!(text.contains("dead blocker #8 on #3: closed as not planned\n"));
    let json = plan.to_json(&[], &[]);
    assert_eq!(
        json["dead_blockers"],
        serde_json::json!([
            { "blocker": 7, "on": 3, "reason": "not an issue" },
            { "blocker": 8, "on": 3, "reason": "closed as not planned" },
        ])
    );
    assert_eq!(json["cycles"], serde_json::json!([]));
}

#[test]
fn the_json_carries_the_cycles_and_marks_a_task_in_one() {
    let plan = plan(
        &snapshot(vec![
            task(3, 2, "Blocked by: #4"),
            task(4, 2, "Blocked by: #5"),
            task(5, 2, "Blocked by: #3"),
        ]),
        &SpawnState::default(),
        Level::Green,
        settings(2),
    );
    let json = plan.to_json(&[], &[]);
    assert_eq!(json["cycles"], serde_json::json!([[3, 4, 5]]));
    assert_eq!(
        json["skipped"]["tasks"][0],
        serde_json::json!({ "task": 3, "reason": "in_cycle", "detail": [3, 4, 5] })
    );
}

fn past(role: &str, epic: u64, number: u32, at: u64) -> PastLaunch {
    PastLaunch {
        instance: format!("{role} instance {number} on #{epic}")
            .parse()
            .expect("an instance"),
        reserved_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(at),
    }
}

fn board_of(epics: Vec<Epic>) -> Snapshot {
    Snapshot {
        epics,
        ..Snapshot::default()
    }
}

fn launched(plan: &Plan) -> Vec<String> {
    plan.launches
        .iter()
        .map(|launch| launch.instance.to_string())
        .collect()
}

#[test]
fn an_unsigned_epic_launches_nothing_whatever_its_tasks() {
    let mut board = snapshot(vec![task(3, 2, "")]);
    board.epics = vec![epic_in(2, false, true, &[3])];
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::Unsigned)]);
}

#[test]
fn a_signed_epic_with_no_task_gets_one_planning_instance_per_signature() {
    let board = board_of(vec![epic_in(2, true, false, &[])]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(launched(&plan), ["planning instance 1 on #2"]);
    assert!(plan.launches[0].tasks.is_empty());

    // The signature is at the epoch: a planning run reserved after it has answered it.
    let mut planned = board.clone();
    planned.launched = vec![past(PLANNING_ROLE, 2, 1, 60)];
    let plan = super::plan(&planned, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::PlannedNothing)]);
}

#[test]
fn a_planned_epic_gets_the_challenger_once() {
    let board = board_of(vec![epic_in(2, true, false, &[3])]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(launched(&plan), ["challenger instance 1 on #2"]);

    let running_state = SpawnState {
        launches: vec![Launch {
            task: None,
            instance: "challenger instance 1 on #2".to_owned(),
            pid: Some(4242),
            started_at: None,
        }],
        ..SpawnState::default()
    };
    let plan = super::plan(&board, &running_state, Level::Green, settings(2));
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::ChallengerRuns)]);

    let mut ended = board.clone();
    ended.launched = vec![past(CHALLENGER_ROLE, 2, 1, 60)];
    let plan = super::plan(&ended, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::ChallengerEnded)]);
}

#[test]
fn a_running_planning_instance_holds_its_epic_whatever_its_stage() {
    let board = board_of(vec![epic_in(2, true, false, &[3])]);
    let state = SpawnState {
        launches: vec![Launch {
            task: None,
            instance: "planning instance 1 on #2".to_owned(),
            pid: Some(4242),
            started_at: None,
        }],
        ..SpawnState::default()
    };
    let plan = plan(&board, &state, Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::PlanningRuns)]);
    assert_eq!(plan.slots.running, 1, "a launch with no task holds a slot");
}

#[test]
fn a_question_on_the_epic_holds_every_launch() {
    let mut board = snapshot(vec![task(3, 2, "")]);
    board.questioned.insert(TicketId::from(2));
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::Questioned)]);
}

#[test]
fn a_done_epic_launches_nothing() {
    let mut epic = epic_in(2, true, true, &[3]);
    epic.tasks[0].standing = Standing::Done;
    let plan = plan(
        &board_of(vec![epic]),
        &SpawnState::default(),
        Level::Green,
        settings(2),
    );
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::Done)]);
}

#[test]
fn a_ready_task_of_no_open_epic_is_left_out() {
    let mut board = snapshot(vec![task(3, 2, "")]);
    board.epics.clear();
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.tasks, [(TicketId::from(3), TaskSkip::NoEpic)]);
}

#[test]
fn each_epic_takes_the_launch_its_stage_calls_for_in_one_tick() {
    let mut board = snapshot(vec![task(9, 8, "")]);
    board.epics.extend([
        epic_in(2, true, false, &[]),
        epic_in(4, true, false, &[5]),
        epic_in(6, false, false, &[]),
    ]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(4));
    assert_eq!(
        launched(&plan),
        [
            "planning instance 1 on #2",
            "challenger instance 2 on #4",
            "team instance 3 on #8",
        ]
    );
    assert_eq!(plan.epics, [(TicketId::from(6), EpicSkip::Unsigned)]);
}
