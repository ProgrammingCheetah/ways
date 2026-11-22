use super::status_listing as status;
use super::*;
use std::time::UNIX_EPOCH;

use crate::app::release::due_releases;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{
    Budget, Instance, InstanceNumber, OpenTicket, RoleName, SizeTable, TaskSize, TicketId,
    TicketState,
};
use crate::domain::{DocumentSignature, Epic, with_release};
use crate::fakes::FakeBoard;
use crate::manifest::Manifest;

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
fn launched(state_dir: &Path, instance: Instance, _tasks: &[u64]) {
    let planned = PlannedLaunch { instance };
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
        "questions 1 (#9)\n\
         question  #9 Which columns are exported?\n\
         running   1\n\
         \x20 team instance 1 on #2\n\
         ready     2 (#3 #4)\n\
         claimed   1 (#5)\n\
         slots     1 free of 2\n\
         machine: 0 instances in 0 projects, 0.0 units in use\n\
         budget: no machine cap\n\
         guard     yellow 72% ($72 of $100)\n"
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
fn status_shows_the_reserve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Budget {
        units: Some(38),
        reserve: 10,
        ..Budget::default()
    };
    let status = status(&board(), dir.path(), 2, &all_alive)
        .expect("status")
        .with_budget(budget);
    let text = status.render_text();
    assert!(
        text.contains("\nbudget 0.0 of 38 units, reserve 10\n"),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["budget"]["reserve"], 10);
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
        ..Budget::default()
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
            text.contains("units in use\nbudget 34.5 of 38 units, reserve 6\nguard"),
            "{text}"
        );
        let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
        assert_eq!(
            json["budget"],
            serde_json::json!({
                "in_use_units": 34.5,
                "units": 38,
                "reserve": 6,
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
fn status_prints_one_line_for_stale_profiles() {
    use crate::app::profile;

    const HEAD: &str = "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n\
        [runners.claude]\ncommand = \"true\"\n\n\
        [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
        [roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\nprofile = \"lean\"\nturns = 5\nbatch = 2\n";
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

    let screen = status(&board(), dir.path(), 1, &all_alive)
        .expect("status")
        .with_stale(profile::stale(dir.path(), &after));
    let text = screen.render_text();
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("older than")).collect();
    assert_eq!(
        lines,
        ["profiles lean and build are older than the manifest (forbidden changed)"],
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    assert_eq!(json["stale"].as_array().map(Vec::len), Some(2));
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
        text.contains("profiles lean and build are older than the manifest (forbidden changed)"),
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
    assert!(text.contains("orphaned  1 (#5)\n"), "{text}");
    assert!(text.contains("claimed   0\n"), "{text}");
    assert!(text.contains("running   0\n"), "{text}");
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

    assert_eq!(status.running.len(), 2, "each instance is listed once");
    assert_eq!(status.running_count, 2);
    assert_eq!(
        status.slots,
        Slots { total: 4, free: 2 },
        "two distinct instances hold two slots"
    );
}

#[test]
fn the_running_line_counts_instances_and_each_is_listed_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(3, 37), &[46, 47]);
    launched(dir.path(), instance(4, 7), &[5]);

    let text = status(&board(), dir.path(), 2, &all_alive)
        .expect("status")
        .render_text();

    assert!(
        text.contains("running   2\n  team instance 3 on #37\n  team instance 4 on #7\nready"),
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
fn status_names_an_epic_whose_every_task_was_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(90), "Export");
    board.add_challenged_epic(TicketId::from(91), "Import");
    board.add_challenged_epic(TicketId::from(92), "Empty");
    for (number, epic) in [(3, 90), (4, 90), (5, 91)] {
        let mut task = ticket(number, "Task", TicketState::Ready);
        task.epic = Some(TicketId::from(epic));
        board.add_ticket(task);
    }
    board.close_not_planned(TicketId::from(3));
    board.close_not_planned(TicketId::from(4));
    board.close_not_planned(TicketId::from(5));
    // Epic 91 has one done task beside its dropped one.
    let mut done = ticket(6, "Done", TicketState::Done);
    done.epic = Some(TicketId::from(91));
    board.add_ticket(done);

    let status = status(&board, dir.path(), 2, &all_alive).expect("status");

    let text = status.render_text();
    assert!(
        text.contains("problem   epic #90 Export: every task dropped\n"),
        "{text}"
    );
    assert_eq!(text.matches("every task dropped").count(), 1, "{text}");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["problems"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["problems"][0]["kind"], "every_task_dropped");
    assert!(json["dead_blockers"].is_array(), "existing fields stay");
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

/// A board with one question whose title is cut: its body holds the whole text.
fn long_question_board() -> (FakeBoard, String) {
    let board = FakeBoard::default();
    let text = format!(
        "{} Proposed answer: keep the header.",
        "Should the export keep it? ".repeat(10)
    );
    let question = NewQuestion {
        on: TicketId::from(4),
        text: text.clone(),
        door: true,
    };
    let mut cut = ticket(20, &question.title(), TicketState::Question);
    cut.body = question.body().into();
    board.add_question(cut);
    (board, text.trim().to_owned())
}

#[test]
fn a_cut_question_title_ends_with_an_ellipsis_and_more_prints_it_whole() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (board, text) = long_question_board();
    let status = status(&board, dir.path(), 2, &none_alive).expect("status");

    let terse = status.render_text();
    let line = terse
        .lines()
        .find(|l| l.starts_with("question  #20"))
        .expect("line");
    assert!(line.ends_with(" …"), "{line}");
    assert!(!terse.contains("Proposed answer"), "{terse}");

    let more = status.render(true);
    assert!(more.contains("question  #20\n"), "{more}");
    assert!(more.contains("Proposed answer: keep the header."), "{more}");
    assert!(more.contains("  raised on #4\n"), "{more}");
    assert!(
        more.contains("  rule it: zaf board ruling 20 '<answer>'\n"),
        "{more}"
    );
    assert_eq!(status.question_details[0].text, text);
    assert_eq!(status.question_details[0].raised_on, Some(4));
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["question_details"][0]["text"], text);
    assert_eq!(json["questions"][0]["number"], 20);
}

#[test]
fn a_whole_question_title_gets_no_ellipsis() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = status(&board(), dir.path(), 2, &none_alive).expect("status");

    let terse = status.render_text();

    assert!(
        terse.contains("question  #9 Which columns are exported?\n"),
        "{terse}"
    );
}

/// A phase run folder of `task`, its holder recorded.
fn phase_folder(state_dir: &Path, task: u64, action: &str, instance: &str) {
    let dir = state_dir.join(format!("phases/{action}-1-task-{task}"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("action"), action).expect("action");
    std::fs::write(dir.join("instance"), instance).expect("instance");
    std::fs::write(dir.join("pid"), "4242 1000").expect("pid");
    std::fs::write(dir.join("task"), task.to_string()).expect("task");
}

#[test]
fn a_running_phase_is_listed_and_holds_a_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    phase_folder(dir.path(), 3, "implement", "implement instance 1 on #1");
    let board = FakeBoard::default();

    let status = status(&board, dir.path(), 4, &all_alive).expect("status");

    assert_eq!(status.running_count, 1);
    assert_eq!(status.slots.free, 3);
    assert_eq!(status.phases.len(), 1);
    assert_eq!(status.phases[0].task, 3);
    assert_eq!(status.phases[0].epic, Some(1));
    let text = status.render_text();
    assert!(
        text.contains("running   1\n  implement #3 (epic #1)\n"),
        "{text}"
    );
    assert!(text.contains("slots     3 free of 4"), "{text}");
    let json: serde_json::Value = serde_json::from_str(&status.to_json()).expect("json");
    assert_eq!(json["phases"][0]["action"], "implement");
    assert_eq!(
        json["running_count"], 1,
        "existing fields keep their meaning"
    );
}

#[test]
fn an_ended_phase_is_not_listed() {
    let dir = tempfile::tempdir().expect("tempdir");
    phase_folder(dir.path(), 3, "implement", "implement instance 1 on #1");
    let board = FakeBoard::default();

    let status = status(&board, dir.path(), 4, &none_alive).expect("status");

    assert_eq!(status.running_count, 0);
    assert!(status.phases.is_empty());
    assert_eq!(status.slots.free, 4);
}

#[test]
fn status_shows_the_lane_and_what_each_ticket_waits_on() {
    use crate::app::actions::{ActionKind, ActionName, ListedPhase, PhaseEnd};
    use crate::app::schedule::tick::support::{facts, gathered};
    use crate::domain::schedule::{Dependency, TicketFacts};
    use crate::domain::{Standing, TaskSize};
    use std::time::{Duration, SystemTime};

    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    let labels = [
        (3, TicketState::Ready),
        (5, TicketState::Ready),
        (7, TicketState::Blocked),
        (8, TicketState::Blocked),
        (11, TicketState::Ready),
    ];
    for (number, label) in labels {
        board.add_ticket(ticket(number, &format!("Task {number}"), label));
    }
    let list = vec![ListedPhase {
        action: ActionName::from("implement"),
        kind: ActionKind::Model,
        ends: PhaseEnd::PrOpen,
        on_fail: None,
    }];
    let since = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
    let of = |task: u64| facts(task, &list, TaskSize::S, since, None, Vec::new());
    // #3 is in the lane, and so is #8, whose `blocked` nothing holds; #5, labelled `ready`, waits
    // on a question; #7 on #3. #11 has no phase list, so its label speaks for it.
    let asks = TicketFacts {
        open_questions: [TicketId::from(45)].into(),
        ..of(5)
    };
    let waits = TicketFacts {
        dependencies: vec![Dependency {
            ticket: TicketId::from(3),
            standing: Standing::Open,
        }],
        ..of(7)
    };
    let mut placed = gathered(vec![
        (of(3), list.clone()),
        (asks, list.clone()),
        (waits, list.clone()),
        (of(8), list.clone()),
    ]);
    placed.open = OpenTicket::tickets(board.open_tickets().expect("the open tickets read"));
    let now = since + Duration::from_secs(60);

    let screen = status(&board, &state_dir, 2, &none_alive)
        .expect("reads")
        .with_placement(&placed, now);

    let text = screen.render(false);
    assert!(text.contains("ready     3 (#3 #8 #11)\n"), "{text}");
    assert!(text.contains("waiting   2 (#5 #7)\n"), "{text}");
    assert!(
        !text.contains("waiting(") && !text.contains("  #3 implement"),
        "{text}"
    );
    let more = screen.render(true);
    assert!(
        more.contains("ready     3 (#3 #8 #11)\n  #3 implement\n  #8 implement\n"),
        "{more}"
    );
    assert!(
        more.contains("waiting   2 (#5 #7)\n  #5 waiting(question #45)\n  #7 waiting(ticket #3)\n"),
        "{more}"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    let ready: Vec<u64> = json["ready"]
        .as_array()
        .expect("ready")
        .iter()
        .map(|task| task["number"].as_u64().expect("a number"))
        .collect();
    assert_eq!(ready, [3, 8, 11]);
    assert_eq!(
        json["lane"],
        serde_json::json!([
            {"task": 3, "next": "implement"},
            {"task": 8, "next": "implement"},
        ])
    );
    assert_eq!(
        json["waiting"],
        serde_json::json!([
            {"task": 5, "on": "waiting(question #45)"},
            {"task": 7, "on": "waiting(ticket #3)"},
        ])
    );
    // Without the placement, as with no `[phases]`: the labels' word, and no waiting line.
    let labelled = status(&board, &state_dir, 2, &none_alive).expect("reads");
    assert!(
        !labelled.render(true).contains("waiting"),
        "{}",
        labelled.render(true)
    );
}

#[test]
fn status_prints_the_waits_of_a_merge_as_it_prints_the_others() {
    use crate::app::actions::{ActionKind, ActionName, ListedPhase, PhaseEnd};
    use crate::app::schedule::tick::support::{facts, gathered};
    use crate::domain::schedule::{BaseFacts, CheckFact, CheckName, CheckResult, PullFacts};
    use crate::domain::{CommitId, TaskSize};
    use std::time::{Duration, SystemTime};

    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    for number in [3, 5] {
        board.add_ticket(ticket(
            number,
            &format!("Task {number}"),
            TicketState::Ready,
        ));
    }
    let listed = |action: &str, kind: ActionKind, ends: PhaseEnd| ListedPhase {
        action: ActionName::from(action),
        kind,
        ends,
        on_fail: None,
    };
    let list = vec![
        listed("implement", ActionKind::Model, PhaseEnd::PrOpen),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged),
    ];
    let since = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
    // Each has its pull request open, so its merge is next.
    let merging = |task: u64| {
        let pull = PullFacts {
            opened_at: since,
            merged_at: None,
            head: Some(CommitId::from("h1")),
            changed_files: crate::domain::ChangedFiles::of(&["src/lib.rs"]),
            checks: Vec::new(),
            reviews: Vec::new(),
            mergeable: None,
            checks_expected: true,
        };
        facts(task, &list, TaskSize::S, since, Some(pull), Vec::new())
    };
    // #3 waits for #12, fresh from a fix, to merge; #5 on the integration branch's checks.
    let mut after_a_fix = merging(3);
    after_a_fix.merging.first = Some(TicketId::from(12));
    let mut over_a_busy_base = merging(5);
    over_a_busy_base.merging.base = BaseFacts {
        head: Some(CommitId::from("a1b2c3d4e5f60718")),
        checks: vec![CheckFact {
            name: CheckName::from("ci"),
            result: CheckResult::Pending,
        }],
        reached_at: None,
    };
    let mut placed = gathered(vec![
        (after_a_fix, list.clone()),
        (over_a_busy_base, list.clone()),
    ]);
    placed.open = OpenTicket::tickets(board.open_tickets().expect("the open tickets read"));

    let screen = status(&board, &state_dir, 2, &none_alive)
        .expect("reads")
        .with_placement(&placed, since + Duration::from_secs(60));

    let more = screen.render(true);
    assert!(
        more.contains(
            "waiting   2 (#3 #5)\n  #3 waiting(merge of #12)\n  #5 waiting(base a1b2c3d)\n"
        ),
        "{more}"
    );
    let json: serde_json::Value = serde_json::from_str(&screen.to_json()).expect("json");
    assert_eq!(
        json["waiting"],
        serde_json::json!([
            {"task": 3, "on": "waiting(merge of #12)"},
            {"task": 5, "on": "waiting(base a1b2c3d)"},
        ])
    );
}

const RELEASE_MANIFEST: &str =
    "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n";

/// A project root holding the release folder of `0.6.0`, its `objectives.md` signed or not, and
/// `design.md` signed when `design` is set.
fn release_project(signed: bool, design: bool) -> (tempfile::TempDir, Manifest) {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    let objectives = if signed {
        DocumentSignature::sign("The objectives.\n", UNIX_EPOCH)
    } else {
        "The objectives.\n".to_owned()
    };
    std::fs::write(folder.join("objectives.md"), objectives).expect("objectives");
    if design {
        let text = DocumentSignature::sign("The design.\n", UNIX_EPOCH);
        std::fs::write(folder.join("design.md"), text).expect("design");
    }
    (dir, RELEASE_MANIFEST.parse().expect("manifest"))
}

/// The screen with the releases due in the project, none of them released.
fn releases_screen(dir: &Path, manifest: &Manifest, epics: &[Epic]) -> Status {
    let state = tempfile::tempdir().expect("state dir");
    let due = due_releases(dir, manifest, epics, &|_| false);
    status(&FakeBoard::default(), state.path(), 1, &all_alive)
        .expect("status")
        .with_releases(&due)
}

fn epic_of(version: &str) -> Epic {
    let body = with_release(version, "Goal.");
    Epic {
        id: 7.into(),
        title: "release".into(),
        signature: None,
        body: body.as_str().into(),
        tasks: Vec::new(),
        planned_at: None,
        challenged: false,
    }
}

#[test]
fn status_names_a_release_with_objectives_due() {
    let (dir, manifest) = release_project(false, false);
    let text = releases_screen(dir.path(), &manifest, &[]).render_text();
    assert!(
        text.contains("release 0.6.0: objectives due, run zaf begin --version 0.6.0\n"),
        "{text}"
    );
}

#[test]
fn status_names_a_release_with_design_due() {
    let (dir, manifest) = release_project(true, false);
    let text = releases_screen(dir.path(), &manifest, &[]).render_text();
    assert!(
        text.contains("release 0.6.0: design due, run zaf begin --version 0.6.0\n"),
        "{text}"
    );
}

#[test]
fn status_omits_a_release_with_nothing_due() {
    let (dir, manifest) = release_project(true, true);
    let screen = releases_screen(dir.path(), &manifest, &[epic_of("0.6.0")]);
    assert!(screen.releases.is_empty());
    assert!(!screen.render_text().contains("release 0.6.0"));
}

#[test]
fn status_omits_a_released_version_and_a_folder_with_a_signoff() {
    let (dir, manifest) = release_project(false, false);
    let old = dir.path().join(".teams/releases/0.5.0");
    std::fs::create_dir_all(&old).expect("folder");
    std::fs::write(old.join("signoff.md"), "| File | hash | date |\n").expect("signoff");
    let due = due_releases(dir.path(), &manifest, &[], &|version| version == "0.6.0");
    assert!(due.is_empty(), "{due:?}");
}

#[test]
fn a_version_is_released_when_the_vcs_holds_its_tag() {
    let (dir, mut manifest) = release_project(false, false);
    let vcs = crate::fakes::FakeVcs::default();
    vcs.tag("zaf-v0.6.0");
    let released = |manifest: &Manifest, vcs: Option<&dyn crate::ports::Vcs>| {
        crate::app::release::is_released(dir.path(), manifest, vcs, "0.6.0")
    };
    assert!(!released(&manifest, Some(&vcs)), "no key, nothing released");
    manifest.release.tag = Some("zaf-v{version}".to_owned());
    assert!(released(&manifest, Some(&vcs)));
    assert!(!released(&manifest, None), "no vcs, nothing released");
    manifest.release.tag = Some("other-v{version}".to_owned());
    assert!(!released(&manifest, Some(&vcs)));
    manifest.release.tag = Some("zaf-v{version}".to_owned());
    let due = due_releases(dir.path(), &manifest, &[], &|v| {
        crate::app::release::is_released(dir.path(), &manifest, Some(&vcs), v)
    });
    assert!(due.is_empty(), "{due:?}");
}

#[test]
fn status_json_lists_due_releases() {
    let (dir, manifest) = release_project(false, false);
    let json = releases_screen(dir.path(), &manifest, &[]).to_json();
    let json: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(
        json["releases"],
        serde_json::json!([{
            "version": "0.6.0",
            "phase": "objectives",
            "command": "zaf begin --version 0.6.0",
        }])
    );
}

/// A board that logs each call it is asked, by name.
struct CallLog {
    board: FakeBoard,
    calls: std::sync::Mutex<Vec<&'static str>>,
}

impl CallLog {
    fn note(&self, call: &'static str) {
        self.calls.lock().expect("log").push(call);
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("log").clone()
    }
}

impl Board for CallLog {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.note("ready");
        self.board.ready()
    }
    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        self.note("task");
        self.board.task(number)
    }
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        self.note("epics");
        self.board.epics()
    }
    fn claim(
        &self,
        task: TicketId,
        instance: &Instance,
    ) -> Result<crate::ports::ClaimOutcome, PortError> {
        self.note("claim");
        self.board.claim(task, instance)
    }
    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.note("heartbeat");
        self.board.heartbeat(task, instance)
    }
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        self.note("claims");
        self.board.claims()
    }
    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.note("release");
        self.board.release(task, instance)
    }
    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        self.note("set_state");
        self.board.set_state(task, label)
    }
    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.note("comment");
        self.board.comment(issue, body)
    }
    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.note("report");
        self.board.report(task, instance, body)
    }
    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.note("questions");
        self.board.questions()
    }
    fn open_doors(&self) -> Result<Vec<TicketId>, PortError> {
        self.note("open_doors");
        self.board.open_doors()
    }
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        self.note("open_question");
        self.board.open_question(question)
    }
    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        self.note("record_ruling");
        self.board.record_ruling(question, ruling)
    }
}

#[test]
fn needs_you_reads_the_board_once() {
    use crate::app::hub::next::NextKind;

    let state = tempfile::tempdir().expect("state dir");
    let log = CallLog {
        board: FakeBoard::default(),
        calls: std::sync::Mutex::default(),
    };
    log.board.add_epic(TicketId::from(7), "Unsigned epic");
    log.board.add_ticket(Ticket {
        epic: Some(TicketId::from(7)),
        ..ticket(8, "Export rows", TicketState::Ready)
    });
    log.board
        .open_question(&NewQuestion {
            on: TicketId::from(7),
            text: "Which name?".to_owned(),
            door: true,
        })
        .expect("opens");
    let (dir, manifest) = release_project(false, false);
    let due = due_releases(dir.path(), &manifest, &[], &|_| false);
    let screen = status(&log, state.path(), 1, &all_alive)
        .expect("status")
        .with_project("example", dir.path())
        .with_releases(&due);
    let read = log.calls();

    let items = screen.needs_you();
    let again = screen.needs_you();

    assert_eq!(log.calls(), read, "needs_you makes no call of its own");
    assert_eq!(items, again);
    let kinds: Vec<NextKind> = items.iter().map(|item| item.kind).collect();
    for kind in [NextKind::Sign, NextKind::Door, NextKind::Phase] {
        assert!(kinds.contains(&kind), "{kinds:?} lacks {kind:?}");
    }
    for once in ["epics", "questions", "open_doors", "claims"] {
        let times = read.iter().filter(|call| **call == once).count();
        assert_eq!(times, 1, "{once} is read once: {read:?}");
    }
}

#[test]
fn the_lines_joined_are_the_rendered_text() {
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
    let mut status = status(&board(), dir.path(), 2, &all_alive).expect("status");
    status.lane.push(LaneLine {
        task: 3,
        next: "implement".to_owned(),
    });
    status.waiting = Some(vec![WaitingLine {
        task: 4,
        on: "task 3".to_owned(),
    }]);
    status.releases.push(ReleaseLine {
        version: "0.5.3".to_owned(),
        phase: "design".to_owned(),
        command: "zaf begin --version 0.5.3".to_owned(),
    });
    status.phases.push(PhaseLine {
        task: 5,
        action: "implement".to_owned(),
        epic: Some(2),
        started_at: "now".to_owned(),
    });

    for more in [false, true] {
        let joined: String = status
            .lines(more)
            .iter()
            .map(|line| {
                let text: String = line.iter().map(|(_, text)| text.as_str()).collect();
                format!("{text}\n")
            })
            .collect();
        assert_eq!(joined, status.render(more), "more = {more}");
    }
    let lines = status.lines(true);
    let meant = |meaning: Meaning, word: &str| {
        lines
            .iter()
            .flatten()
            .any(|(m, text)| *m == meaning && text.as_str().contains(word))
    };
    assert!(meant(Meaning::Verb, "zaf begin --version 0.5.3"));
    assert!(meant(Meaning::Waiting, "question  "));
    assert!(meant(Meaning::Noun, "#9"));
    assert!(meant(Meaning::Dim, "0"), "a zero count is dim");
}

/// The index of the first plain line that starts with `prefix`.
fn at(text: &str, prefix: &str) -> usize {
    text.lines()
        .position(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starts with {prefix:?}:\n{text}"))
}

#[test]
fn what_waits_on_you_comes_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut screen = status(&board(), dir.path(), 2, &all_alive).expect("status");
    screen.epics.push(EpicLine {
        number: TicketId::from(7),
        title: "Planned epic".into(),
        stage: EpicStage::Planned,
        signature_void: false,
        done: 0,
        total: 2,
    });

    let text = screen.render_text();

    let question = at(&text, "question  #9 Which columns are exported?");
    assert!(question < at(&text, "ready"), "{text}");
    assert!(question < at(&text, "epic      #7"), "{text}");
    assert!(question < at(&text, "running"), "{text}");
    let more = screen.render(true);
    assert!(
        at(&more, "  rule it: zaf board ruling 9") < at(&more, "ready"),
        "{more}"
    );
}

#[test]
fn its_first_block_is_the_inbox() {
    use crate::app::hub::next::NextKind;

    let state = tempfile::tempdir().expect("state dir");
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(7), "Unsigned epic");
    board.add_ticket(Ticket {
        epic: Some(TicketId::from(7)),
        ..ticket(8, "Export rows", TicketState::Ready)
    });
    board
        .open_question(&NewQuestion {
            on: TicketId::from(7),
            text: "Which name?".to_owned(),
            door: true,
        })
        .expect("opens");
    board.add_question(ticket(20, "Quoting rule?", TicketState::Question));
    let (dir, manifest) = release_project(false, false);
    let due = due_releases(dir.path(), &manifest, &[], &|_| false);
    let screen = status(&board, state.path(), 1, &all_alive)
        .expect("status")
        .with_project("example", dir.path())
        .with_releases(&due);

    let wanted: Vec<String> = screen
        .needs_you()
        .iter()
        .filter_map(|item| match item.kind {
            NextKind::Sign => Some(format!("epic      #{}", item.number.expect("an epic"))),
            NextKind::Question | NextKind::Door | NextKind::Failed => {
                Some(format!("question  #{}", item.number.expect("a question")))
            }
            NextKind::Phase => Some("release 0.6.0: ".to_owned()),
            // Status draws no line for these: adding them is out of scope (#468).
            NextKind::Unticked | NextKind::Broken => None,
        })
        .collect();

    let text = screen.render_text();
    let first_block: Vec<&str> = text
        .lines()
        .take_while(|line| !line.starts_with("running"))
        .filter(|line| {
            ["epic ", "question  ", "release "]
                .iter()
                .any(|lead| line.starts_with(lead))
        })
        .collect();
    assert_eq!(wanted.len(), 4, "{wanted:?}");
    assert_eq!(first_block.len(), wanted.len(), "{text}");
    for (line, prefix) in first_block.iter().zip(&wanted) {
        assert!(line.starts_with(prefix.as_str()), "{line:?} for {prefix:?}");
    }
}

#[test]
fn problems_come_before_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);
    let board = FakeBoard::default();
    board.add_ticket(ticket(3, "Export rows", TicketState::Ready));
    board.add_ticket(ticket(5, "Import rows", TicketState::Ready));
    board
        .claim(TicketId::from(5), &instance(1, 2))
        .expect("claims the ready task");

    let text = status(&board, dir.path(), 2, &none_alive)
        .expect("status")
        .render_text();

    let orphaned = at(&text, "orphaned  1 (#5)");
    assert!(orphaned < at(&text, "running"), "{text}");
    assert!(orphaned < at(&text, "ready"), "{text}");
}

#[test]
fn reordering_keeps_every_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    launched(dir.path(), instance(1, 2), &[5]);
    let mut screen = status(&board(), dir.path(), 2, &none_alive).expect("status");
    screen.releases.push(ReleaseLine {
        version: "0.5.3".to_owned(),
        phase: "design".to_owned(),
        command: "zaf begin --version 0.5.3".to_owned(),
    });
    screen.unparsed.push(UnparsedLine {
        number: 4,
        title: "Export header".to_owned(),
        problems: vec!["Model: missing".to_owned()],
    });
    screen.cycles.push(vec![11, 12]);
    screen.dead_blockers.push(DeadLine {
        blocker: 8,
        on: 9,
        reason: "not an issue".to_owned(),
    });
    let red = guard::assess(Some(100.0), 99.0, &[70, 85, 95], UNIX_EPOCH);
    let guard_line = format!("guard     {}", red.line());
    screen.guard = Some(red);

    let mut wanted = vec![
        "questions 1 (#9)".to_owned(),
        "question  #9 Which columns are exported?".to_owned(),
        "release 0.5.3: design due, run zaf begin --version 0.5.3".to_owned(),
        "orphaned  1 (#5)".to_owned(),
        "Ready, does not parse:".to_owned(),
        "  #4 Export header".to_owned(),
        "    Model: missing".to_owned(),
        "blocker cycle: #11 → #12 → #11".to_owned(),
        "dead blocker #8 on #9: not an issue".to_owned(),
        "running   0".to_owned(),
        "ready     2 (#3 #4)".to_owned(),
        "claimed   0".to_owned(),
        "slots     2 free of 2".to_owned(),
        "machine: 0 instances in 0 projects, 0.0 units in use".to_owned(),
        "budget: no machine cap".to_owned(),
        guard_line,
    ];
    let text = screen.render_text();
    let mut got: Vec<String> = text.lines().map(str::to_owned).collect();

    wanted.sort();
    got.sort();
    assert_eq!(got, wanted, "{text}");
}
