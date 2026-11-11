use super::*;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{
    Budget, Instance, InstanceNumber, RoleName, SizeTable, TaskSize, TicketId, TicketState,
};
use crate::fakes::FakeBoard;

/// A task body that follows the template, its security trigger line reading `trigger`.
fn task_body(trigger: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: {trigger}\n\
         Model: sonnet\nSize: S\nBlocked by: —\n"
    )
}

/// A ticket; a ready one gets a body that follows the template.
fn ticket(number: u64, title: &str, label: TicketState) -> Ticket {
    let body = match label {
        TicketState::Ready => task_body("no"),
        _ => String::new(),
    };
    Ticket {
        id: TicketId::from(number),
        epic: None,
        title: title.into(),
        body: body.into(),
        state: Some(label),
        url: "".into(),
        kind: None,
    }
}

fn instance(number: u32, epic: u64) -> Instance {
    Instance {
        role: RoleName::from("team"),
        epic: TicketId::from(epic),
        number: InstanceNumber::from(number),
    }
}

/// A run folder for `instance` carrying `tasks`, held by pid 4242 started at `1000`.
fn launched(state_dir: &Path, instance: Instance, tasks: &[u64]) {
    let planned = PlannedLaunch {
        instance,
        tasks: tasks.iter().copied().map(TicketId::from).collect(),
    };
    let holder = Holder {
        pid: 4242,
        start: "1000".to_owned(),
    };
    runs::reserve(state_dir, &planned, &holder, "2026-10-28T01:00:00Z").expect("reserves");
}

/// The process table with every recorded process alive.
fn all_alive(_: &Holder) -> bool {
    true
}

/// The process table with no recorded process alive.
fn none_alive(_: &Holder) -> bool {
    false
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(3, "Export rows", TicketState::Ready));
    board.add_ticket(ticket(4, "Export header", TicketState::Ready));
    board.add_ticket(ticket(5, "Import rows", TicketState::Ready));
    board.add_question(ticket(
        9,
        "Which columns are exported?",
        TicketState::Question,
    ));
    board.add_question(ticket(10, "Quoting rule?", TicketState::Question));
    board
        .record_ruling(TicketId::from(10), "Always quote.")
        .expect("a ruled question closes");
    board.add_ticket(ticket(6, "Export footer", TicketState::Question));
    board
        .claim(TicketId::from(5), &instance(1, 2))
        .expect("claims the ready task");
    board
}

#[test]
fn the_screen_joins_board_run_folders_and_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);
    std::fs::write(
        dir.path().join(SPAWN_STATE_FILE),
        r#"{"next_instance": 2, "questions": [10, 77]}"#,
    )
    .expect("spawn.state");
    let guard = guard::assess(
        Some(100.0),
        72.0,
        &[70, 85, 95],
        std::time::SystemTime::UNIX_EPOCH,
    );
    guard::write(dir.path(), &guard).expect("guard.json");

    let status = status(&board(), dir.path(), 2, &all_alive).expect("status");

    let ready: Vec<u64> = status.ready.iter().map(|t| t.number.get()).collect();
    assert_eq!(ready, [3, 4], "the claimed task is no longer ready");
    assert_eq!(status.claimed.len(), 1);
    assert_eq!(status.claimed[0].task, 5);
    assert_eq!(status.claimed[0].instance, "team instance 1 on #2");
    assert!(status.orphaned.is_empty());
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
        .map(|q| (q.number.get(), q.title.as_str()))
        .collect();
    assert_eq!(
        questions,
        [(9, "Which columns are exported?")],
        "open questions come from the board: the ruled one and the waiting task are not listed, \
         and the stale numbers an older spawn.state kept are ignored"
    );

    let text = status.render_text();
    assert_eq!(
        text,
        "ready     2 (#3 #4)\n\
         claimed   1 (#5)\n\
         questions 1 (#9)\n\
         running   1\n\
         \x20 team instance 1 on #2: #5\n\
         slots     1 free of 2\n\
         machine: 0 instances in 0 projects, 0.0 units in use\n\
         budget: no machine cap\n\
         guard     yellow 72% ($72 of $100)\n\
         question  #9 Which columns are exported?\n"
    );
}

fn machine_instance(root: &str, slug: &str, size: Option<TaskSize>) -> MachineInstance {
    machine_instance_holding(root, slug, size, 0)
}

fn machine_instance_holding(
    root: &str,
    slug: &str,
    size: Option<TaskSize>,
    memory: u64,
) -> MachineInstance {
    MachineInstance {
        run: Path::new(root).join(".zaf/runs").join(slug),
        root: PathBuf::from(root),
        instance: Some(slug.replace('-', " ").replace(" on ", " on #")),
        size,
        pids: vec![100, 101],
        memory: Mebibytes::new(memory),
    }
}

#[test]
fn status_shows_the_units_in_use() {
    let dir = tempfile::tempdir().expect("tempdir");
    let machine = [
        machine_instance_holding(
            "/work/bookshelf",
            "planning-instance-1-on-2",
            Some(TaskSize::XS),
            512,
        ),
        machine_instance_holding("/work/ledger", "team-instance-2-on-3", None, 1024),
    ];

    let status = status(&board(), dir.path(), 2, &all_alive)
        .expect("status")
        .with_machine(&machine);

    let text = status.render_text();
    assert!(
        text.contains("machine: 2 instances in 2 projects, 1.5 units in use\n"),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(
        json["machine"]["memory_mib"], 1536,
        "the total, in mebibytes"
    );
    assert_eq!(json["machine"]["instances"][0]["memory_mib"], 512);
    assert_eq!(json["machine"]["instances"][1]["memory_mib"], 1024);
}

#[test]
fn status_shows_the_budget_in_use() {
    let dir = tempfile::tempdir().expect("tempdir");
    let machine = [
        machine_instance_holding("/work/bookshelf", "team-instance-1-on-2", None, 30 * 1024),
        machine_instance_holding("/work/ledger", "team-instance-2-on-3", None, 4 * 1024 + 512),
    ];
    let budget = Budget {
        units: Some(38),
        sizes: SizeTable::default().with(TaskSize::M, 6),
    };

    // The budget and the machine may come in either order: the line uses the machine's total.
    let screens = [
        status(&board(), dir.path(), 2, &all_alive)
            .expect("status")
            .with_machine(&machine)
            .with_budget(budget),
        status(&board(), dir.path(), 2, &all_alive)
            .expect("status")
            .with_budget(budget)
            .with_machine(&machine),
    ];
    for status in screens {
        let text = status.render_text();
        assert!(
            text.contains("units in use\nbudget 34.5 of 38 units\nguard"),
            "{text}"
        );
        let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
        assert_eq!(
            json["budget"],
            serde_json::json!({
                "in_use_units": 34.5,
                "units": 38,
                "sizes": { "XS": 1, "S": 2, "M": 6, "L": 8, "XL": 16 },
            })
        );
    }

    // With no budget the line says there is no cap, and the JSON carries a null budget.
    let status = status(&board(), dir.path(), 2, &all_alive)
        .expect("status")
        .with_machine(&machine)
        .with_budget(Budget::default());
    assert!(status.render_text().contains("\nbudget: no machine cap\n"));
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert!(json["budget"]["units"].is_null());
    assert_eq!(json["budget"]["in_use_units"], 34.5);
    assert_eq!(json["budget"]["sizes"]["XL"], 16);
}

#[test]
fn status_lists_the_machines_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let machine = [
        machine_instance(
            "/work/bookshelf",
            "planning-instance-1-on-2",
            Some(TaskSize::XS),
        ),
        machine_instance("/work/bookshelf", "team-instance-2-on-3", None),
        machine_instance(
            "/work/ledger",
            "challenger-instance-5-on-8",
            Some(TaskSize::XS),
        ),
    ];

    let status = status(&board(), dir.path(), 2, &all_alive)
        .expect("status")
        .with_machine(&machine);

    let text = status.render_text();
    assert!(
        text.contains(
            "slots     2 free of 2\nmachine: 3 instances in 2 projects, 0.0 units in use\n"
        ),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(
        json["machine"],
        serde_json::json!({
            "projects": 2,
            "memory_mib": 0,
            "instances": [
                {
                    "root": "/work/bookshelf",
                    "instance": "planning instance 1 on #2",
                    "size": "XS",
                    "memory_mib": 0,
                },
                {
                    "root": "/work/bookshelf",
                    "instance": "team instance 2 on #3",
                    "size": null,
                    "memory_mib": 0,
                },
                {
                    "root": "/work/ledger",
                    "instance": "challenger instance 5 on #8",
                    "size": "XS",
                    "memory_mib": 0,
                },
            ],
        })
    );
}

#[test]
fn json_carries_the_whole_screen() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);
    let status = status(&board(), dir.path(), 2, &all_alive).expect("status");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["ready"][0]["number"], 3);
    assert_eq!(json["claimed"][0]["task"], 5);
    assert_eq!(json["orphaned"], serde_json::json!([]));
    assert_eq!(json["running"][0]["instance"], "team instance 1 on #2");
    assert_eq!(json["slots"]["free"], 1);
    assert_eq!(json["questions"][0]["title"], "Which columns are exported?");
    assert!(json["guard"].is_null());
}

#[test]
fn status_names_a_stale_profile() {
    use crate::app::profile;

    const HEAD: &str = "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";
    let dir = tempfile::tempdir().expect("tempdir");
    let before: crate::manifest::Manifest =
        format!("{HEAD}[forbidden]\ncommands = [\"git push\"]\n")
            .parse()
            .expect("manifest");
    let after: crate::manifest::Manifest =
        format!("{HEAD}[forbidden]\ncommands = [\"gh pr merge\"]\n")
            .parse()
            .expect("manifest");
    profile::write_stamps(dir.path(), &before);

    let current = status(&board(), dir.path(), 1, &all_alive)
        .expect("status")
        .with_stale(profile::stale(dir.path(), &before));
    assert!(!current.render_text().contains("older than"));

    let screen = status(&board(), dir.path(), 1, &all_alive)
        .expect("status")
        .with_stale(profile::stale(dir.path(), &after));
    let text = screen.render_text();
    assert!(
        text.contains("profile build is older than the manifest (forbidden changed)"),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    assert_eq!(json["stale"][1]["profile"], "build");
    assert_eq!(json["stale"][1]["reasons"][0], "forbidden changed");
}

#[test]
fn a_dead_instance_frees_its_slot_at_once_and_its_claims_read_orphaned() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);

    let status = status(&board(), dir.path(), 2, &none_alive).expect("status");

    assert_eq!(status.running_count, 0);
    assert_eq!(
        status.slots,
        Slots { total: 2, free: 2 },
        "no tick in between"
    );
    assert!(
        status.claimed.is_empty(),
        "a claim counts only while its instance runs"
    );
    assert_eq!(status.orphaned.len(), 1);
    assert_eq!(status.orphaned[0].task, 5);
    let text = status.render_text();
    assert!(text.contains("claimed   0\norphaned  1 (#5)\n"), "{text}");
    assert!(
        text.contains("running   0\nslots     2 free of 2\n"),
        "{text}"
    );
}

#[test]
fn a_recorded_pid_now_held_by_another_process_counts_as_dead() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);
    // pid 4242 runs again, but it is a process that started later.
    let table = |holder: &Holder| holder.pid == 4242 && holder.start == "2000";

    let status = status(&board(), dir.path(), 2, &table).expect("status");

    assert_eq!(status.running_count, 0);
    assert_eq!(status.orphaned.len(), 1);
}

#[test]
fn a_claim_by_an_instance_this_machine_did_not_launch_stays_claimed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = status(&board(), dir.path(), 2, &none_alive).expect("status");
    assert_eq!(
        status.claimed.len(),
        1,
        "no run folder here: not this machine's to judge"
    );
    assert!(status.orphaned.is_empty());
}

#[test]
fn without_state_files_nothing_runs_and_the_guard_is_not_computed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = status(&board(), dir.path(), 1, &all_alive).expect("status");
    assert!(status.running.is_empty());
    assert_eq!(status.questions.len(), 1, "questions need no state file");
    assert_eq!(status.slots, Slots { total: 1, free: 1 });
    assert!(status.guard.is_none());
    assert!(status.render_text().contains("guard     not computed"));
}

#[test]
fn more_launches_than_slots_leave_none_free() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[3]);
    launched(dir.path(), instance(2, 2), &[4]);
    launched(dir.path(), instance(3, 2), &[5]);
    assert_eq!(
        status(&board(), dir.path(), 2, &all_alive)
            .expect("status")
            .slots
            .free,
        0
    );
}

#[test]
fn a_garbled_spawn_state_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), "not json").expect("spawn.state");
    let error = status(&board(), dir.path(), 1, &all_alive).expect_err("garbled");
    assert!(matches!(error, StatusError::SpawnState { .. }), "{error}");
    assert!(error.to_string().contains(SPAWN_STATE_FILE));
}

#[test]
fn a_batch_of_tasks_holds_one_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[3, 4]);
    launched(dir.path(), instance(2, 7), &[5]);

    let status = status(&board(), dir.path(), 4, &all_alive).expect("status");

    assert_eq!(status.running.len(), 3, "every task is listed");
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
    launched(dir.path(), instance(3, 37), &[46, 47]);
    launched(dir.path(), instance(4, 7), &[5]);

    let text = status(&board(), dir.path(), 2, &all_alive)
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

#[test]
fn status_lists_ready_tasks_that_do_not_parse() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_ticket(ticket(3, "Export rows", TicketState::Ready));
    board.add_ticket(Ticket {
        body: task_body("no, it removes x").into(),
        ..ticket(4, "Export header", TicketState::Ready)
    });

    let status = status(&board, dir.path(), 2, &all_alive).expect("status");

    let ready: Vec<u64> = status.ready.iter().map(|t| t.number.get()).collect();
    assert_eq!(ready, [3], "a task that does not parse is not runnable");
    assert_eq!(status.unparsed.len(), 1);
    assert_eq!(status.unparsed[0].number, 4);
    assert_eq!(
        status.unparsed[0].problems,
        ["Security trigger: no, it removes x (accepts: no | yes <which>)"]
    );
    let text = status.render_text();
    assert!(
        text.contains(
            "Ready, does not parse:\n  #4 Export header\n    \
             Security trigger: no, it removes x (accepts: no | yes <which>)\n"
        ),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["unparsed"][0]["number"], 4);
}

fn blocked(number: u64, by: &str) -> Ticket {
    Ticket {
        body: format!("Blocked by: {by}").into(),
        ..ticket(number, &format!("Task {number}"), TicketState::Ready)
    }
}

#[test]
fn status_names_a_two_task_cycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_ticket(blocked(12, "#11"));
    board.add_ticket(blocked(11, "#12"));

    let status = status(&board, dir.path(), 2, &all_alive).expect("status");

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

    let status = status(&board, dir.path(), 2, &all_alive).expect("status");

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
    board.add_ticket(blocked(3, "#4 #5 #7 #99"));
    board.add_ticket(ticket(4, "Open blocker", TicketState::Blocked));
    board.add_ticket(ticket(5, "Dropped blocker", TicketState::Blocked));
    board.close_not_planned(TicketId::from(5));
    board.add_ticket(ticket(7, "Finished blocker", TicketState::Blocked));
    board.close(TicketId::from(7));

    let status = status(&board, dir.path(), 2, &all_alive).expect("status");

    assert!(status.cycles.is_empty());
    let text = status.render_text();
    assert!(
        text.contains("dead blocker #5 on #3: closed as not planned\n"),
        "{text}"
    );
    assert!(
        text.contains("dead blocker #99 on #3: not an issue\n"),
        "{text}"
    );
    assert!(!text.contains("dead blocker #4"), "{text}");
    assert!(
        !text.contains("dead blocker #7"),
        "closed as completed without `done` is done: {text}"
    );
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(
        json["dead_blockers"][1],
        serde_json::json!({ "blocker": 99, "on": 3, "reason": "not an issue" })
    );
}

#[test]
fn each_open_epic_shows_with_its_stage() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    board.add_epic(TicketId::from(90), "tickets over backends");
    let status = status(&board, dir.path(), 2, &all_alive).expect("status");
    let epic = status
        .epics
        .iter()
        .find(|epic| epic.number == TicketId::from(90))
        .expect("the epic is listed");
    assert_eq!(epic.stage, crate::domain::EpicStage::Unsigned);
    let text = status.render_text();
    assert!(
        text.contains("epic      #90 unsigned 0 of 0  tickets over backends"),
        "{text}"
    );
    assert!(status.to_json().contains("\"stage\": \"unsigned\""));
}
/// Writes `.zaf/slots/<pid>` as a slotted gate run would, its process started at `start`.
fn slot_record(state_dir: &Path, pid: u32, holder: &str, level: &str, start: &str) {
    let folder = state_dir.join(SLOTS_DIR);
    std::fs::create_dir_all(&folder).expect("slots dir");
    let record = SlotRecord {
        holder: holder.to_owned(),
        level: level.to_owned(),
        since: "2026-11-09T10:00:00Z".to_owned(),
        start: start.to_owned(),
    };
    std::fs::write(
        folder.join(pid.to_string()),
        serde_json::to_string(&record).expect("json"),
    )
    .expect("record");
}

#[test]
fn status_shows_who_holds_a_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    slot_record(dir.path(), 4242, "team instance 1 on #2", "full", "1000");
    slot_record(dir.path(), 4343, "the Engineer", "tests", "2000");
    // Pid 4343 is gone, and pid 4242 was reused by a process that started later.
    let only_4242_at_1000 = |holder: &Holder| holder.pid == 4242 && holder.start == "1000";

    let screen = status(&board(), dir.path(), 2, &only_4242_at_1000).expect("status");

    assert_eq!(
        screen.held_slots,
        [SlotLine {
            holder: "team instance 1 on #2".to_owned(),
            level: "full".to_owned(),
            since: "2026-11-09T10:00:00Z".to_owned(),
        }]
    );
    let text = screen.render_text();
    assert!(
        text.contains("slot: team instance 1 on #2 full since 2026-11-09T10:00:00Z\n"),
        "{text}"
    );
    assert!(!text.contains("the Engineer"), "{text}");
    assert!(
        !dir.path().join(SLOTS_DIR).join("4343").exists(),
        "a record left by a dead process is dropped"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    assert_eq!(json["held_slots"][0]["level"], "full");
}

#[test]
fn a_screen_without_slot_records_has_no_slot_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = status(&board(), dir.path(), 2, &all_alive).expect("status");
    assert!(screen.held_slots.is_empty());
    assert!(!screen.render_text().contains("slot:"));
}
