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
        kind: None,
    }
}

/// An open epic in `stage`'s conditions, with `tasks` as its open tasks.
fn epic_in(number: u64, signed: bool, challenged: bool, tasks: &[u64]) -> Epic {
    let body: crate::domain::TicketBody = format!("Goal: epic {number}").into();
    Epic {
        id: TicketId::from(number),
        title: format!("Epic {number}").into(),
        signature: signed.then(|| Signature::of(&body, SystemTime::UNIX_EPOCH)),
        planned_at: None,
        body,
        tasks: tasks
            .iter()
            .map(|task| EpicTask {
                number: TicketId::from(*task),
                title: format!("Task {task}").into(),
                state: Some(TicketState::Ready),
                standing: Standing::Open,
                group: None,
                kind: None,
            })
            .collect(),
        challenged,
    }
}

/// The ready tasks, each of its epic signed and challenged, so their phases may build them.
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
    Settings { slots }
}

/// A launch that holds a slot: its process runs, whatever its role.
fn running(instance: &str) -> Launch {
    Launch {
        task: None,
        instance: instance.to_owned(),
        pid: Some(4242),
        started_at: None,
    }
}

fn numbers(values: &[u64]) -> Vec<TicketId> {
    values.iter().copied().map(TicketId::from).collect()
}

/// A board with a signed epic that has no task yet, for each of `epics`: planning's to launch.
fn unplanned(epics: &[u64]) -> Snapshot {
    board_of(
        epics
            .iter()
            .map(|epic| epic_in(*epic, true, false, &[]))
            .collect(),
    )
}

#[test]
fn no_free_slot_launches_nothing() {
    let state = SpawnState {
        launches: vec![running("planning instance 1 on #20")],
        ..SpawnState::default()
    };
    let plan = plan(&unplanned(&[2]), &state, Level::Green, settings(1));
    assert!(plan.launches.is_empty());
    assert_eq!(plan.slots.free, 0);
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::NoFreeSlot)]);
}

#[test]
fn no_team_launch_for_ready_tasks() {
    // Ready, unblocked, unclaimed tasks of challenged epics, with every slot free: the tasks'
    // phases build them, so the tick launches no instance on their account.
    let ready = vec![
        task(3, 2, "Goal: rows\nBlocked by: —"),
        task(4, 2, "Goal: columns\nBlocked by: —"),
        task(5, 2, ""),
        task(13, 12, "Goal: other epic\nBlocked by: —"),
    ];
    let state = SpawnState {
        next_instance: 12,
        ..SpawnState::default()
    };
    let plan = plan(&snapshot(ready), &state, Level::Green, settings(4));
    assert!(
        plan.launches.is_empty(),
        "ready tasks launch nothing: {:?}",
        launched(&plan)
    );
    assert_eq!(
        plan.epics,
        [
            (TicketId::from(2), EpicSkip::BuiltByPhases),
            (TicketId::from(12), EpicSkip::BuiltByPhases),
        ]
    );
    assert_eq!(plan.slots.free, 4, "no slot is taken for them either");
    assert_eq!(epic_candidates(&snapshot(vec![task(3, 2, "")]), &state), []);
    let text = plan.render_text();
    assert!(!text.contains("launching"), "{text}");
    assert!(!text.contains("team"), "{text}");

    // Whatever the stage mix, a launch is planning's or the challenger's, never a team's.
    let mut mixed = snapshot(vec![task(9, 8, "")]);
    mixed
        .epics
        .extend([epic_in(2, true, false, &[]), epic_in(4, true, false, &[5])]);
    let plan = super::plan(&mixed, &SpawnState::default(), Level::Green, settings(8));
    assert!(
        plan.launches.iter().all(|launch| {
            matches!(
                launch.instance.role.as_str(),
                PLANNING_ROLE | CHALLENGER_ROLE
            )
        }),
        "{:?}",
        launched(&plan)
    );
}

#[test]
fn planning_still_launches() {
    // An epic in its signed stage with no task gets a planning instance, even with another
    // epic's ready tasks on the board and the counter past zero.
    let mut board = snapshot(vec![task(9, 8, "Blocked by: —")]);
    board.epics.push(epic_in(2, true, false, &[]));
    let state = SpawnState {
        next_instance: 7,
        ..SpawnState::default()
    };
    let plan = plan(&board, &state, Level::Green, settings(2));
    assert_eq!(launched(&plan), ["planning instance 7 on #2"]);
    assert_eq!(plan.launches[0].instance.role.as_str(), PLANNING_ROLE);
    assert_eq!(plan.epics, [(TicketId::from(8), EpicSkip::BuiltByPhases)]);
    assert_eq!(
        plan.slots.free, 2,
        "the slots are counted before the launch"
    );

    // Once planning has answered the signature it is not launched again.
    board.launched = vec![past(PLANNING_ROLE, 2, 7, 60)];
    let plan = super::plan(&board, &state, Level::Green, settings(2));
    assert!(plan.launches.is_empty());
}

#[test]
fn two_epics_and_one_slot_launch_one() {
    let plan = plan(
        &unplanned(&[2, 12]),
        &SpawnState::default(),
        Level::Green,
        settings(1),
    );
    assert_eq!(plan.launches.len(), 1);
    assert_eq!(plan.launches[0].instance.epic, TicketId::from(2));
    assert_eq!(plan.epics, [(TicketId::from(12), EpicSkip::NoFreeSlot)]);
}

#[test]
fn the_guard_levels_change_the_slot_count() {
    assert_eq!(allowed_slots(4, Level::Green), 4);
    assert_eq!(allowed_slots(4, Level::Yellow), 2);
    assert_eq!(allowed_slots(1, Level::Yellow), 1);
    assert_eq!(allowed_slots(3, Level::Yellow), 1);
    assert_eq!(allowed_slots(4, Level::Orange), 1);
    assert_eq!(allowed_slots(4, Level::Red), 0);

    let board = unplanned(&[2, 12, 22]);
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
    let plan = plan(
        &unplanned(&[2]),
        &SpawnState::default(),
        Level::Red,
        settings(2),
    );
    assert_eq!(plan.slots.allowed, 0);
    assert_eq!(plan.epics, [(TicketId::from(2), EpicSkip::GuardRed)]);
}

#[test]
fn a_second_epic_launches_beside_a_running_instance() {
    let state = SpawnState {
        launches: vec![running("planning instance 3 on #37")],
        next_instance: 4,
    };
    let plan = plan(&unplanned(&[50]), &state, Level::Green, settings(2));
    assert_eq!(plan.slots.running, 1);
    assert_eq!(plan.slots.free, 1);
    assert_eq!(launched(&plan), ["planning instance 4 on #50"]);
    assert!(plan.epics.is_empty());
}

#[test]
fn a_blocker_cycle_among_ready_tasks_is_named_once() {
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
    assert_eq!(plan.cycles, [numbers(&[3, 4])]);
    assert!(plan.launches.is_empty());
    assert!(plan.render_text().contains("blocker cycle: #3 → #4 → #3\n"));
}

#[test]
fn a_blocker_cycle_an_open_task_leads_to_is_named() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7 #9")]);
    board.open.insert(TicketId::from(7), numbers(&[8]));
    board.open.insert(TicketId::from(8), numbers(&[7]));
    board.open.insert(TicketId::from(9), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(plan.cycles, [numbers(&[7, 8])]);
    assert!(plan.render_text().contains("blocker cycle: #7 → #8 → #7\n"));
}

#[test]
fn a_plain_open_blocker_is_no_cycle_and_no_dead_one() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7")]);
    board.open.insert(TicketId::from(7), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.cycles.is_empty());
    assert!(plan.dead.is_empty());
    assert!(!plan.render_text().contains("blocker cycle"));
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
fn the_json_carries_the_cycles_and_no_skipped_tasks() {
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
    assert!(
        json["skipped"].get("tasks").is_none(),
        "no task is skipped: none is launched"
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

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
}

#[test]
fn a_signed_epic_waits_from_its_signature() {
    let mut epic = epic_in(2, true, false, &[]);
    epic.signature = Some(Signature::of(&epic.body, at(100)));
    let board = board_of(vec![epic]);
    assert_eq!(
        epic_candidates(&board, &SpawnState::default()),
        [EpicCandidate {
            epic: TicketId::from(2),
            role: RoleName::from(PLANNING_ROLE),
            since: at(100),
        }]
    );
}

#[test]
fn a_planned_epic_waits_from_its_planning_exit() {
    let mut epic = epic_in(2, true, false, &[3]);
    epic.signature = Some(Signature::of(&epic.body, at(100)));
    epic.planned_at = Some(at(500));
    let board = board_of(vec![epic.clone()]);
    assert_eq!(
        epic_candidates(&board, &SpawnState::default()),
        [EpicCandidate {
            epic: TicketId::from(2),
            role: RoleName::from(CHALLENGER_ROLE),
            since: at(500),
        }],
        "the challenger waits from the planning exit, not from the signature"
    );

    // No exit marker on the epic: the signature stands in.
    epic.planned_at = None;
    let board = board_of(vec![epic]);
    assert_eq!(
        epic_candidates(&board, &SpawnState::default())
            .first()
            .map(|candidate| candidate.since),
        Some(at(100))
    );

    // A challenged epic and an unsigned one are not epic launches.
    let idle = board_of(vec![
        epic_in(4, true, true, &[5]),
        epic_in(6, false, false, &[]),
    ]);
    assert_eq!(epic_candidates(&idle, &SpawnState::default()), []);
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
    assert_eq!(plan.slots.running, 1, "a launch holds a slot");
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
        ["planning instance 1 on #2", "challenger instance 2 on #4",]
    );
    assert_eq!(
        plan.epics,
        [
            (TicketId::from(6), EpicSkip::Unsigned),
            (TicketId::from(8), EpicSkip::BuiltByPhases),
        ]
    );
}
