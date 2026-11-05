use std::time::Duration;

use super::*;

fn ticket(number: u64, phase: Phase) -> TicketRow {
    TicketRow {
        number,
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
    draw(&Snapshot { epics: vec![epic] }, width)
}

#[test]
fn frame_shows_epic_group_and_ticket_rows() {
    let frame = frame_of(epic(), 100);
    let lines: Vec<&str> = frame.lines().collect();
    assert!(lines[0].starts_with("epic #103"), "{frame}");
    assert!(lines[0].contains("17 of 82"), "{frame}");
    assert!(lines[0].contains("$39.83"), "{frame}");
    assert!(lines[0].contains('['), "a bar: {frame}");
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
    one.task = Some(crate::domain::IssueNumber::from(322));
    append(dir.path(), &one).expect("appends");
    append(
        dir.path(),
        &report_at(95, "reading\nwith\ttabs  and\nlines"),
    )
    .expect("appends");
    assert_eq!(
        last_report(dir.path(), crate::domain::IssueNumber::from(322)),
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
    let task = crate::domain::IssueNumber::from(330);
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
        plans_of(dir.path(), crate::domain::IssueNumber::from(331)),
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
