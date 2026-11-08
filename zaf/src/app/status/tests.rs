use super::*;
use crate::domain::{Instance, InstanceNumber, IssueNumber, RoleName, TaskLabel};
use crate::fakes::FakeBoard;

fn ticket(number: u64, title: &str, label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: None,
        title: title.to_owned(),
        body: String::new(),
        label: Some(label),
    }
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(3, "Export rows", TaskLabel::Ready));
    board.add_ticket(ticket(4, "Export header", TaskLabel::Ready));
    board.add_ticket(ticket(5, "Import rows", TaskLabel::Ready));
    board.add_question(ticket(
        9,
        "Which columns are exported?",
        TaskLabel::Question,
    ));
    board.add_question(ticket(10, "Quoting rule?", TaskLabel::Question));
    board
        .record_ruling(IssueNumber::from(10), "Always quote.")
        .expect("a ruled question closes");
    board.add_ticket(ticket(6, "Export footer", TaskLabel::Question));
    let instance = Instance {
        role: RoleName::from("programmer"),
        epic: IssueNumber::from(2),
        number: InstanceNumber::from(1),
    };
    board
        .claim(IssueNumber::from(5), &instance)
        .expect("claims the ready task");
    board
}

const SPAWN_STATE: &str = r#"{
  "launches": [{"task": 5, "instance": "programmer instance 1 on #2", "pid": 4242, "started_at": "2026-10-28T01:00:00Z"}],
  "questions": [10, 77]
}"#;

#[test]
fn the_screen_joins_board_spawn_state_and_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), SPAWN_STATE).expect("spawn.state");
    let guard = guard::assess(
        Some(100.0),
        72.0,
        &[70, 85, 95],
        std::time::SystemTime::UNIX_EPOCH,
    );
    guard::write(dir.path(), &guard).expect("guard.json");

    let status = status(&board(), dir.path(), 2).expect("status");

    let ready: Vec<u64> = status.ready.iter().map(|t| t.number).collect();
    assert_eq!(ready, [3, 4], "the claimed task is no longer ready");
    assert_eq!(status.claimed.len(), 1);
    assert_eq!(status.claimed[0].task, 5);
    assert_eq!(status.claimed[0].instance, "programmer instance 1 on #2");
    assert_eq!(status.running.len(), 1);
    assert_eq!(status.running[0].pid, Some(4242));
    assert_eq!(status.slots, Slots { total: 2, free: 1 });
    assert_eq!(
        status.guard.as_ref().map(|g| g.level),
        Some(guard::Level::Yellow)
    );
    let questions: Vec<(u64, &str)> = status
        .questions
        .iter()
        .map(|q| (q.number, q.title.as_str()))
        .collect();
    assert_eq!(
        questions,
        [(9, "Which columns are exported?")],
        "open questions come from the board: the ruled one and the waiting task are not listed, \
         and the stale numbers in spawn.state are ignored"
    );

    let text = status.render_text();
    assert_eq!(
        text,
        "ready     2 (#3 #4)\n\
         claimed   1 (#5)\n\
         questions 1 (#9)\n\
         running   1\n\
         \x20 programmer instance 1 on #2: #5\n\
         slots     1 free of 2\n\
         guard     yellow 72% ($72 of $100)\n\
         question  #9 Which columns are exported?\n"
    );
}

#[test]
fn json_carries_the_whole_screen() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), SPAWN_STATE).expect("spawn.state");
    let status = status(&board(), dir.path(), 2).expect("status");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["ready"][0]["number"], 3);
    assert_eq!(json["claimed"][0]["task"], 5);
    assert_eq!(
        json["running"][0]["instance"],
        "programmer instance 1 on #2"
    );
    assert_eq!(json["slots"]["free"], 1);
    assert_eq!(json["questions"][0]["title"], "Which columns are exported?");
    assert!(json["guard"].is_null());
}

#[test]
fn without_state_files_nothing_runs_and_the_guard_is_not_computed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = status(&board(), dir.path(), 1).expect("status");
    assert!(status.running.is_empty());
    assert_eq!(status.questions.len(), 1, "questions need no state file");
    assert_eq!(status.slots, Slots { total: 1, free: 1 });
    assert!(status.guard.is_none());
    assert!(status.render_text().contains("guard     not computed"));
}

#[test]
fn more_launches_than_slots_leave_none_free() {
    let dir = tempfile::tempdir().expect("tempdir");
    let launch = |task: u32, number: u32| {
        format!(r#"{{"task": {task}, "instance": "programmer instance {number} on #2"}}"#)
    };
    std::fs::write(
        dir.path().join(SPAWN_STATE_FILE),
        format!(
            r#"{{"launches": [{}, {}, {}]}}"#,
            launch(3, 1),
            launch(4, 2),
            launch(5, 3)
        ),
    )
    .expect("spawn.state");
    assert_eq!(
        status(&board(), dir.path(), 2).expect("status").slots.free,
        0
    );
}

#[test]
fn a_garbled_spawn_state_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), "not json").expect("spawn.state");
    let error = status(&board(), dir.path(), 1).expect_err("garbled");
    assert!(matches!(error, StatusError::SpawnState { .. }), "{error}");
    assert!(error.to_string().contains(SPAWN_STATE_FILE));
}

#[test]
fn a_batch_of_tasks_holds_one_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = r#"{"launches": [
        {"task": 3, "instance": "team instance 1 on #2"},
        {"task": 4, "instance": "team instance 1 on #2"},
        {"task": 5, "instance": "team instance 2 on #7"}
    ]}"#;
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), state).expect("spawn.state");

    let status = status(&board(), dir.path(), 4).expect("status");

    assert_eq!(status.running.len(), 3, "every entry is still listed");
    assert_eq!(status.running_count, 2);
    assert_eq!(
        status.slots,
        Slots { total: 4, free: 2 },
        "two distinct instances hold two slots"
    );
}

#[test]
fn the_running_line_counts_instances_and_each_is_listed_once_with_its_tasks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = r#"{"launches": [
        {"task": 46, "instance": "team instance 3 on #37"},
        {"task": 47, "instance": "team instance 3 on #37"},
        {"task": 5, "instance": "team instance 4 on #7"}
    ]}"#;
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), state).expect("spawn.state");

    let text = status(&board(), dir.path(), 2)
        .expect("status")
        .render_text();

    assert!(
        text.contains(
            "running   2\n  team instance 3 on #37: #46 #47\n  team instance 4 on #7: #5\n\
             slots     0 free of 2\n"
        ),
        "{text}"
    );
}

fn blocked(number: u64, by: &str) -> Ticket {
    Ticket {
        body: format!("Blocked by: {by}"),
        ..ticket(number, &format!("Task {number}"), TaskLabel::Ready)
    }
}

#[test]
fn status_names_a_two_task_cycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_ticket(blocked(12, "#11"));
    board.add_ticket(blocked(11, "#12"));

    let status = status(&board, dir.path(), 2).expect("status");

    assert_eq!(status.cycles, [[11, 12]]);
    assert!(
        status
            .render_text()
            .contains("blocker cycle: #11 → #12 → #11\n")
    );
}

#[test]
fn status_names_a_three_task_cycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_ticket(blocked(22, "#20"));
    board.add_ticket(blocked(20, "#21"));
    board.add_ticket(blocked(21, "#22"));
    board.add_ticket(blocked(11, "#12"));
    board.add_ticket(blocked(12, "#11"));

    let status = status(&board, dir.path(), 2).expect("status");

    assert_eq!(status.cycles, [vec![11, 12], vec![20, 21, 22]]);
    let text = status.render_text();
    assert!(
        text.contains("blocker cycle: #20 → #21 → #22 → #20\n"),
        "{text}"
    );
    assert!(text.contains("blocker cycle: #11 → #12 → #11\n"), "{text}");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["cycles"], serde_json::json!([[11, 12], [20, 21, 22]]));
}

#[test]
fn status_names_dead_blockers_and_keeps_a_plain_open_blocker_quiet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_ticket(blocked(3, "#4 #5 #99"));
    board.add_ticket(ticket(4, "Open blocker", TaskLabel::Blocked));
    board.add_ticket(ticket(5, "Dropped blocker", TaskLabel::Blocked));
    board.close(IssueNumber::from(5));

    let status = status(&board, dir.path(), 2).expect("status");

    assert!(status.cycles.is_empty());
    let text = status.render_text();
    assert!(
        text.contains("dead blocker #5 on #3: closed without done\n"),
        "{text}"
    );
    assert!(
        text.contains("dead blocker #99 on #3: not an issue\n"),
        "{text}"
    );
    assert!(!text.contains("dead blocker #4"), "{text}");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(
        json["dead_blockers"][1],
        serde_json::json!({ "blocker": 99, "on": 3, "reason": "not an issue" })
    );
}
