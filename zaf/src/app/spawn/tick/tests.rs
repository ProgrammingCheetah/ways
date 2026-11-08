use super::*;
use crate::app::status::Launch;
use crate::domain::TaskLabel;

fn task(number: u64, epic: u64, body: &str) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: Some(IssueNumber::from(epic)),
        title: format!("Task {number}"),
        body: body.to_owned(),
        label: Some(TaskLabel::Ready),
    }
}

fn snapshot(ready: Vec<Ticket>) -> Snapshot {
    Snapshot {
        ready,
        ..Snapshot::default()
    }
}

fn settings(slots: u32) -> Settings {
    Settings { slots, batch: 4 }
}

fn running(task: u64, instance: &str) -> Launch {
    Launch {
        task,
        instance: instance.to_owned(),
        pid: Some(4242),
        started_at: None,
    }
}

fn numbers(values: &[u64]) -> Vec<IssueNumber> {
    values.iter().copied().map(IssueNumber::from).collect()
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
    assert_eq!(plan.epics, [(IssueNumber::from(2), EpicSkip::NoFreeSlot)]);
}

#[test]
fn a_blocked_task_is_skipped_with_the_blocker_named() {
    let mut board = snapshot(vec![
        task(3, 2, "Goal: rows\nBlocked by: #7, #8"),
        task(4, 2, "Blocked by: —"),
    ]);
    board.closed.insert(IssueNumber::from(8));
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(
        plan.tasks,
        [(IssueNumber::from(3), TaskSkip::Blocked(numbers(&[7])))]
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
        [(IssueNumber::from(2), EpicSkip::NothingLaunchable)]
    );
}

#[test]
fn two_epics_and_one_slot_launch_one() {
    let board = snapshot(vec![task(3, 2, ""), task(13, 12, "")]);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(1));
    assert_eq!(plan.launches.len(), 1);
    assert_eq!(plan.launches[0].instance.epic, IssueNumber::from(2));
    assert_eq!(plan.epics, [(IssueNumber::from(12), EpicSkip::NoFreeSlot)]);
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
    assert_eq!(plan.epics, [(IssueNumber::from(2), EpicSkip::GuardRed)]);
}

#[test]
fn a_claimed_task_is_never_launched() {
    let mut board = snapshot(vec![task(3, 2, ""), task(4, 2, "")]);
    board.claimed.insert(IssueNumber::from(3));
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(plan.tasks, [(IssueNumber::from(3), TaskSkip::Claimed)]);
    assert_eq!(plan.launches[0].tasks, numbers(&[4]));

    board.claimed.insert(IssueNumber::from(4));
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
            IssueNumber::from(3),
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
        ..SpawnState::default()
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
    assert_eq!(plan.tasks, [(IssueNumber::from(3), TaskSkip::NoEpic)]);
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
            (IssueNumber::from(3), TaskSkip::InCycle(numbers(&[3, 4]))),
            (IssueNumber::from(4), TaskSkip::InCycle(numbers(&[3, 4]))),
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
    board.open.insert(IssueNumber::from(7), numbers(&[8]));
    board.open.insert(IssueNumber::from(8), numbers(&[7]));
    board.open.insert(IssueNumber::from(9), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert_eq!(
        plan.tasks,
        [(IssueNumber::from(3), TaskSkip::Blocked(numbers(&[7, 9])))]
    );
    assert_eq!(plan.cycles, [numbers(&[7, 8])]);
    let text = plan.render_text();
    assert!(text.contains("#3 skipped: blocked by #7 (in a blocker cycle) #9\n"));
    assert!(text.contains("blocker cycle: #7 → #8 → #7\n"));
}

#[test]
fn a_plain_open_blocker_reads_blocked_by_alone() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7")]);
    board.open.insert(IssueNumber::from(7), Vec::new());
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    assert!(plan.cycles.is_empty());
    assert!(plan.dead.is_empty());
    assert!(plan.render_text().contains("#3 skipped: blocked by #7\n"));
}

#[test]
fn the_plan_names_dead_blockers_in_text_and_json() {
    let mut board = snapshot(vec![task(3, 2, "Blocked by: #7 #8")]);
    board
        .dead
        .insert(IssueNumber::from(7), DeadReason::NotAnIssue);
    board
        .dead
        .insert(IssueNumber::from(8), DeadReason::ClosedWithoutDone);
    let plan = plan(&board, &SpawnState::default(), Level::Green, settings(2));
    let text = plan.render_text();
    assert!(text.contains("dead blocker #7 on #3: not an issue\n"));
    assert!(text.contains("dead blocker #8 on #3: closed without done\n"));
    let json = plan.to_json(&[], &[]);
    assert_eq!(
        json["dead_blockers"],
        serde_json::json!([
            { "blocker": 7, "on": 3, "reason": "not an issue" },
            { "blocker": 8, "on": 3, "reason": "closed without done" },
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
