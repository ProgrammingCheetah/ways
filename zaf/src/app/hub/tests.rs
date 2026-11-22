use std::time::Duration;

use super::*;
use crate::domain::EpicStage;

mod line;
mod liveness;
use liveness::{Dead, board, gathered, instance, refresh, run_folder};

fn ticket(number: u64, phase: Phase) -> TicketRow {
    TicketRow {
        number,
        title: "Name each ticket in the hub".to_owned(),
        who: "sonnet programmer".to_owned(),
        phase,
        word: "testing".to_owned(),
        since: Some(Duration::from_secs(40)),
        models: vec![],
    }
}

fn epic() -> EpicRow {
    EpicRow {
        number: 103,
        title: "The hub".to_owned(),
        stage: EpicStage::Challenged,
        done: 17,
        total: 82,
        dollars: 39.83,
        slots_free: 1,
        slots_total: 2,
        ready: 63,
        questions: 0,
        tickets: vec![ticket(143, Phase::Build)],
        groups: vec![GroupRow {
            name: "hub".to_owned(),
            done: 3,
            total: 9,
            tickets: vec![ticket(250, Phase::Check)],
        }],
    }
}

fn frame_of(epic: EpicRow, width: usize) -> String {
    draw(
        &Snapshot {
            epics: vec![epic],
            ..Snapshot::default()
        },
        width,
    )
}

#[test]
fn frame_shows_epic_group_and_ticket_rows() {
    let frame = frame_of(epic(), 100);
    let lines: Vec<&str> = frame.lines().collect();
    assert!(lines[0].starts_with("epic #103"), "{frame}");
    assert!(lines[0].contains("17 of 82"), "{frame}");
    assert!(lines[0].contains("$39.83"), "{frame}");
    assert!(lines[0].contains('['), "a bar: {frame}");
    assert!(
        lines[1].starts_with("  challenged · "),
        "the stage: {frame}"
    );
    assert!(lines[1].contains("1 of 2 slots free"), "{frame}");
    assert!(lines[1].contains("63 ready"), "{frame}");
    assert!(lines[1].contains("no questions"), "{frame}");
    let ticket = lines
        .iter()
        .find(|line| line.contains("#143"))
        .expect("ticket");
    assert!(ticket.contains("sonnet programmer"), "{frame}");
    assert!(ticket.contains("build"), "{frame}");
    assert!(ticket.contains("testing"), "{frame}");
    assert!(ticket.ends_with("40s"), "{frame}");
    let group = lines
        .iter()
        .find(|line| line.starts_with("group: hub"))
        .expect("group");
    assert!(group.contains("3 of 9"), "{frame}");
    assert!(group.contains('['), "{frame}");
    let grouped = lines
        .iter()
        .position(|line| line.contains("#250"))
        .expect("grouped ticket");
    let at = lines
        .iter()
        .position(|line| line.starts_with("group: hub"))
        .expect("group");
    assert!(
        grouped > at,
        "a grouped ticket sits under its group: {frame}"
    );
}

fn filled(line: &str) -> usize {
    let open = line.find('[').expect("bar");
    let close = line.find(']').expect("bar");
    line[open + 1..close].chars().filter(|c| *c == '#').count()
}

fn bar_cells(line: &str) -> usize {
    let open = line.find('[').expect("bar");
    let close = line.find(']').expect("bar");
    line[open + 1..close].chars().count()
}

#[test]
fn ticket_bar_follows_its_phase() {
    let mut steps = Vec::new();
    for phase in Phase::DECLARABLE {
        let mut row = epic();
        row.groups.clear();
        row.tickets = vec![ticket(7, phase)];
        let frame = frame_of(row, 100);
        let line = frame
            .lines()
            .find(|line| line.contains("#7 "))
            .expect("ticket");
        steps.push((filled(line), bar_cells(line)));
    }
    let fills: Vec<usize> = steps.iter().map(|(fill, _)| *fill).collect();
    assert!(
        fills.windows(2).all(|pair| pair[0] < pair[1]),
        "each later phase fills more: {fills:?}"
    );
    let cells = steps[0].1;
    assert!(steps.iter().all(|(_, width)| *width == cells));
    assert_eq!(
        *fills.last().unwrap(),
        cells,
        "the last phase fills the bar"
    );
    let mut unclaimed = epic();
    unclaimed.groups.clear();
    unclaimed.tickets = vec![ticket(7, Phase::Unclaimed)];
    let frame = frame_of(unclaimed, 100);
    let line = frame
        .lines()
        .find(|line| line.contains("#7 "))
        .expect("ticket");
    assert_eq!(filled(line), 0);
}

#[test]
fn no_line_passes_the_width() {
    let mut long = epic();
    long.dollars = 123456.78;
    long.tickets[0].who = "a very long model name and a role".to_owned();
    long.tickets[0].word = "wordwordwordwordword".to_owned();
    for width in [0, 1, 10, 20, 40, 60, 80, 100, 200] {
        let frame = frame_of(long.clone(), width);
        for line in frame.lines() {
            assert!(
                line.chars().count() <= width,
                "width {width}, line {line:?}"
            );
        }
    }
}

#[test]
fn nothing_running_still_shows_slots() {
    let mut idle = epic();
    idle.tickets.clear();
    idle.groups.clear();
    idle.slots_free = 4;
    idle.slots_total = 4;
    let frame = frame_of(idle, 100);
    assert!(frame.contains("4 of 4 slots free"), "{frame}");
    assert!(!frame.contains("group:"), "{frame}");
    assert!(!frame.contains("  #"), "no ticket row: {frame}");
}

#[test]
fn a_frame_with_no_epic_says_so() {
    assert_eq!(draw(&Snapshot::default(), 80), "no open epics\n");
}

fn word_of(tool: &str, command: Option<&str>) -> &'static str {
    let payload = serde_json::json!({
        "tool_name": tool,
        "tool_input": { "command": command },
        "cwd": "/somewhere",
    });
    ToolCall::parse(&payload.to_string()).word()
}

#[test]
fn tool_maps_to_a_word() {
    for tool in ["Read", "Grep", "Glob"] {
        assert_eq!(word_of(tool, None), "reading", "{tool}");
    }
    for tool in ["Edit", "Write"] {
        assert_eq!(word_of(tool, None), "writing", "{tool}");
    }
    for command in [
        "cargo test",
        "cd zaf && cargo test --lib hub",
        "cargo nextest run",
        "pytest -q tests/",
        "npm test",
        "cargo clippy --all-targets -- -D warnings",
        "cd zaf && cargo fmt --check && cargo build && cargo clippy && cargo test",
        "zaf check 322",
        "zaf check #322",
        "zaf verify 322",
        "cd /work && zaf check 322",
    ] {
        assert_eq!(word_of("Bash", Some(command)), "testing", "{command}");
    }
    for command in ["zaf check pr:5:322", "zaf check pr:5", "zaf check pr"] {
        assert_eq!(
            word_of("Bash", Some(command)),
            "waiting on review",
            "{command}"
        );
    }
    for command in [
        "ls -la",
        "jj commit -m \"fix the test\"",
        "rg latest src",
        "zaf ask -t 322 \"fact: what is a test?\"",
        "",
    ] {
        assert_eq!(word_of("Bash", Some(command)), "working", "{command}");
    }
    assert_eq!(word_of("Bash", None), "working");
    for tool in ["WebFetch", "Task", "TodoWrite", "mcp__x__y", ""] {
        assert_eq!(word_of(tool, None), "working", "{tool}");
    }
}

#[test]
fn a_slice_workspace_names_its_task() {
    let workspaces = std::path::Path::new("/p/.zaf/workspaces");
    let pattern = "feature/{version}-{epic}-{task}";
    let task = |cwd: &str| {
        task_in_workspace(std::path::Path::new(cwd), workspaces, pattern).map(|task| task.get())
    };
    assert_eq!(task("/p/.zaf/workspaces/feature/0.2.0-103-322"), Some(322));
    assert_eq!(
        task("/p/.zaf/workspaces/feature/0.2.0-103-322/zaf/src"),
        Some(322)
    );
    assert_eq!(task("/p/.zaf/workspaces/feature/0.2.0"), None);
    assert_eq!(task("/p/.zaf/workspaces/feature"), None);
    assert_eq!(task("/p/.zaf/workspaces"), None);
    assert_eq!(task("/p/zaf/src"), None);
    assert_eq!(task("/elsewhere/feature/0.2.0-103-322"), None);
}

fn report_at(seconds: u64, what: &str) -> Report {
    Report {
        at: std::time::UNIX_EPOCH + Duration::from_secs(seconds),
        task: None,
        worker: "team".to_owned(),
        what: what.to_owned(),
    }
}

#[test]
fn reports_stay_under_the_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let over = 25;
    for seconds in 0..(MAX_LINES + over) as u64 {
        append(dir.path(), &report_at(seconds, "working")).expect("appends");
        let lines = std::fs::read_to_string(reports_file(dir.path()))
            .expect("file")
            .lines()
            .count();
        assert!(lines <= MAX_LINES, "{lines} lines after report {seconds}");
    }
    let text = std::fs::read_to_string(reports_file(dir.path())).expect("file");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), MAX_LINES);
    assert_eq!(MAX_LINES, 1000);
    assert!(
        lines[0].starts_with(&format!("{over} ")),
        "oldest dropped: {}",
        lines[0]
    );
    assert!(
        lines
            .last()
            .expect("a line")
            .starts_with(&format!("{} ", MAX_LINES + over - 1)),
        "newest kept"
    );
}

#[test]
fn a_report_reads_back_as_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut one = report_at(90, "testing");
    one.task = Some(crate::domain::TicketId::from(322));
    append(dir.path(), &one).expect("appends");
    append(
        dir.path(),
        &report_at(95, "reading\nwith\ttabs  and\nlines"),
    )
    .expect("appends");
    assert_eq!(
        last_report(dir.path(), crate::domain::TicketId::from(322)),
        Some(("testing".to_owned(), one.at))
    );
    let text = std::fs::read_to_string(reports_file(dir.path())).expect("file");
    assert_eq!(text.lines().count(), 2, "a report is one line: {text:?}");
    assert!(
        text.contains("95 - team reading with tabs and lines"),
        "{text:?}"
    );
}

fn words(phases: &[&str]) -> Vec<String> {
    phases.iter().map(|phase| (*phase).to_owned()).collect()
}

fn model(role: &str, phases: &[&str], current: Option<&str>, iteration: u32) -> ModelRow {
    ModelRow {
        role: role.to_owned(),
        phases: words(phases),
        current: current.map(str::to_owned),
        iteration,
    }
}

#[test]
fn a_repeated_phase_counts_an_iteration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let task = crate::domain::TicketId::from(330);
    declare(
        dir.path(),
        task,
        "programmer",
        &words(&["read", "write", "test"]),
    )
    .expect("declares");
    let iteration = |phase: &str| {
        let plan = enter(dir.path(), task, "programmer", phase).expect("enters");
        (plan.current().map(str::to_owned), plan.iteration())
    };
    assert_eq!(iteration("read"), (Some("read".to_owned()), 1));
    assert_eq!(iteration("write"), (Some("write".to_owned()), 1));
    // The same phase again, with nothing between, is not a second pass.
    assert_eq!(iteration("write"), (Some("write".to_owned()), 1));
    assert_eq!(iteration("test"), (Some("test".to_owned()), 1));
    assert_eq!(iteration("write"), (Some("write".to_owned()), 2));
    assert_eq!(iteration("test"), (Some("test".to_owned()), 2));
    assert_eq!(iteration("write"), (Some("write".to_owned()), 3));
    // The hub reads it back from the files, and draws the second pass as `write ×2`.
    let plans = plans_of(dir.path(), task);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].0.iteration(), 3);
    assert_eq!(
        plans_of(dir.path(), crate::domain::TicketId::from(331)),
        vec![]
    );
    let mut row = epic();
    row.groups.clear();
    row.tickets[0].models = vec![model(
        "programmer",
        &["read", "write", "test"],
        Some("write"),
        2,
    )];
    let frame = frame_of(row, 100);
    assert!(frame.contains("write ×2"), "{frame}");
}

#[test]
fn frame_draws_a_row_per_reporting_model() {
    let mut row = epic();
    row.groups.clear();
    row.tickets[0].models = vec![
        model(
            "programmer",
            &["read", "write", "test", "fix"],
            Some("test"),
            1,
        ),
        model("keeper", &["search", "answer"], None, 0),
    ];
    let frame = frame_of(row, 100);
    let lines: Vec<&str> = frame.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("#143"))
        .expect("ticket");
    let programmer = lines[at + 1];
    let keeper = lines[at + 2];
    assert!(programmer.trim_start().starts_with("programmer"), "{frame}");
    assert!(programmer.contains("test"), "{frame}");
    assert!(
        !programmer.contains('×'),
        "first pass shows no count: {frame}"
    );
    assert_eq!(filled(programmer), bar_cells(programmer) * 3 / 4, "{frame}");
    assert!(keeper.trim_start().starts_with("keeper"), "{frame}");
    assert_eq!(filled(keeper), 0, "not started: {frame}");
    // Nothing else sits between the ticket and its models.
    assert_eq!(lines.len(), at + 3, "{frame}");
}

#[test]
fn a_ticket_gains_one_line_per_model() {
    let mut row = epic();
    row.groups.clear();
    let plain = frame_of(row.clone(), 100);
    row.tickets[0].models = vec![model("programmer", &["read"], Some("read"), 1)];
    let with = frame_of(row, 100);
    assert_eq!(with.lines().count(), plain.lines().count() + 1);
}

#[test]
fn no_model_row_passes_the_width() {
    let mut long = epic();
    long.tickets[0].models = vec![model(
        "a-role-with-a-very-long-name",
        &["a-long-phase", "another-long-phase", "third"],
        Some("another-long-phase"),
        12,
    )];
    long.groups[0].tickets[0].models = long.tickets[0].models.clone();
    for width in [0, 1, 10, 20, 40, 60, 80, 100, 200] {
        let frame = frame_of(long.clone(), width);
        for line in frame.lines() {
            assert!(
                line.chars().count() <= width,
                "width {width}, line {line:?}"
            );
        }
    }
}

#[test]
fn dead_instance_frees_its_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_folder(dir.path(), &instance(7), 4242);
    let board = board();
    let running = gathered(dir.path(), &board, &Dead::default());
    let epic = &running.epics[0];
    assert_eq!(epic.slots_free, 1, "one instance holds one slot");
    assert_eq!(epic.tickets.len(), 1, "its task is drawn");

    // The same folder, its process gone: no slot, no row, in the state and in the frame.
    let gone = gathered(dir.path(), &board, &Dead::new([4242].into()));
    let epic = &gone.epics[0];
    assert_eq!(epic.slots_free, 2, "{epic:?}");
    assert!(epic.tickets.is_empty(), "{epic:?}");
    let frame = draw(&gone, 100);
    assert!(frame.contains("2 of 2 slots free"), "{frame}");
    assert!(!frame.contains("#143"), "{frame}");
}

#[test]
fn instance_ending_between_redraws() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_folder(dir.path(), &instance(7), 4242);
    let board = board();
    let dead = Dead::default();
    let mut snapshot = gathered(dir.path(), &board, &dead);
    let before = draw(&snapshot, 100);
    assert!(before.contains("1 of 2 slots free"), "{before}");
    assert!(before.contains("#143"), "{before}");

    // The process ends; the board is not read again, only the process table is.
    dead.borrow_mut().insert(4242);
    refresh(&mut snapshot, dir.path(), &dead).expect("reads");
    let after = draw(&snapshot, 100);
    assert!(after.contains("2 of 2 slots free"), "{after}");
    assert!(!after.contains("#143"), "{after}");
}

#[test]
fn a_quiet_redraw_reads_the_process_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_folder(dir.path(), &instance(7), 4242);
    let board = board();
    let dead = Dead::default();
    let reads = std::cell::Cell::new(0_u32);
    let second = |seconds: u64| std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
    let (mut watch, first) = Watch::start(
        |_| {
            reads.set(reads.get() + 1);
            Ok(gathered(dir.path(), &board, &dead))
        },
        |snapshot: &mut Snapshot| refresh(snapshot, dir.path(), &dead),
        second(0),
        Mark::default(),
        100,
    )
    .expect("starts");
    assert!(first.contains("1 of 2 slots free"), "{first}");
    dead.borrow_mut().insert(4242);
    let next = watch
        .step(second(1), Mark::default())
        .expect("steps")
        .expect("a frame");
    assert!(next.contains("2 of 2 slots free"), "{next}");
    assert!(!next.contains("#143"), "{next}");
    assert_eq!(reads.get(), 1, "the board was not read again");
}

#[test]
fn running_row_names_its_ticket_by_id_and_title() {
    let frame = frame_of(epic(), 100);
    let line = frame
        .lines()
        .find(|line| line.contains("#143"))
        .expect("ticket");
    assert!(
        line.trim_start()
            .starts_with("#143  Name each ticket in the hub"),
        "the title follows the id: {frame}"
    );
    assert!(line.contains("sonnet programmer"), "{frame}");
    assert!(line.ends_with("40s"), "{frame}");
    let epic_line = frame.lines().next().expect("epic line");
    assert!(epic_line.starts_with("epic #103  The hub"), "{frame}");
}

#[test]
fn long_title_is_cut_to_the_width() {
    let title = "A ticket title that runs on and on, far past what any narrow terminal could show";
    let mut long = epic();
    long.groups.clear();
    long.tickets[0].title = title.to_owned();
    let wide = frame_of(long.clone(), 300);
    assert!(wide.contains(title), "whole at a wide width: {wide}");
    assert!(!wide.contains('…'), "{wide}");
    let lines = wide.lines().count();
    for width in [70, 80, 100] {
        let frame = frame_of(long.clone(), width);
        assert_eq!(frame.lines().count(), lines, "cut, never wrapped: {frame}");
        let line = frame
            .lines()
            .find(|line| line.contains("#143"))
            .expect("ticket");
        assert!(line.contains("#143  A ticket"), "{frame}");
        assert!(
            line.contains('…'),
            "cut with an ellipsis at {width}: {frame}"
        );
        assert!(line.ends_with("40s"), "the tail is kept: {frame}");
        assert!(line.chars().count() <= width, "{frame}");
        assert!(line.contains('['), "the bar is kept: {frame}");
    }
}

/// The state gathered from `board` and `state_dir`'s run folders, every process alive.
fn state_of(state_dir: &std::path::Path, board: &crate::fakes::FakeBoard) -> state::ProjectState {
    let (forge, vcs) = (
        crate::fakes::FakeForge::default(),
        crate::fakes::FakeVcs::default(),
    );
    let records = crate::app::actions::SliceRecords::new(state_dir);
    let alive = |_: &crate::ports::Holder| true;
    state::gather_state(&state::StateInputs {
        lists: None,
        project: "ways",
        root: state_dir,
        state_dir,
        board: Ok(board),
        forge: Some(&forge),
        vcs: Some(&vcs),
        records: &records,
        ledger: &[],
        prices: &crate::app::report::PriceTable::default(),
        pull_base: None,
        alive: &alive,
        now: std::time::SystemTime::UNIX_EPOCH,
        places: None,
        gathered: None,
        releases: None,
    })
}

#[test]
fn epic_instance_row_names_its_epic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board();
    let planning = crate::domain::Instance {
        role: "planning".into(),
        epic: crate::domain::TicketId::from(103),
        number: 1.into(),
    };
    run_folder(dir.path(), &planning, 4242);

    let state = state_of(dir.path(), &board);

    let row = state
        .instances
        .iter()
        .find(|row| row.role == "planning")
        .expect("the planning instance");
    assert_eq!(row.task, None);
    assert_eq!(row.number, Some(103));
    assert_eq!(row.title.as_deref(), Some("Epic"));
}

#[test]
fn gathered_state_carries_whole_titles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board();
    let long = "A task title far longer than any frame would keep, whole in the state";
    board.add_ticket(crate::domain::Ticket {
        id: crate::domain::TicketId::from(144),
        epic: Some(crate::domain::TicketId::from(103)),
        title: long.into(),
        body: "".into(),
        state: Some(crate::domain::TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    run_folder(dir.path(), &instance(7), 4242);

    let state = state_of(dir.path(), &board);

    assert_eq!(state.epics[0].number, 103);
    assert_eq!(state.epics[0].title, "Epic");
    let queued = |number: u64| {
        state
            .queue
            .iter()
            .find(|task| task.number == number)
            .unwrap_or_else(|| panic!("#{number} in the queue"))
    };
    assert_eq!(queued(144).title, long);
    assert_eq!(queued(143).title, "A task");
    let running = &state.instances[0];
    assert_eq!(running.number, Some(143));
    assert_eq!(running.title.as_deref(), Some("A task"));

    let snapshot = gathered(dir.path(), &board, &Dead::default());
    assert_eq!(snapshot.epics[0].title, "Epic");
    assert_eq!(snapshot.epics[0].tickets[0].title, "A task");
}
