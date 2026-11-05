use std::fs;

use serde_json::json;

use super::super::ledger::{LEDGER_FILE, Ledger, Source};
use super::*;
use crate::domain::TaskLabel;
use crate::fakes::FakeBoard;
use crate::ports::Ticket;

const OPUS: &str = "claude-opus-5-5";

fn number(n: u64) -> IssueNumber {
    IssueNumber::from(n)
}

fn ticket(n: u64, epic: Option<u64>, label: Option<TaskLabel>) -> Ticket {
    Ticket {
        number: number(n),
        epic: epic.map(number),
        title: format!("task {n}"),
        body: String::new(),
        label,
    }
}

/// A run of one turn-million: 1M of each kind, so a price shows plainly in the dollars.
fn run(run_id: &str, role: Option<&str>, epic: Option<u64>, tasks: &[u64]) -> LedgerRow {
    LedgerRow {
        run_id: run_id.to_owned(),
        parent_run: None,
        project: "ways".to_owned(),
        role: role.map(str::to_owned),
        epic,
        tasks: tasks.to_vec(),
        model: OPUS.to_owned(),
        first_time: None,
        last_time: None,
        turns: 10,
        input: 1_000_000,
        cache_write: 1_000_000,
        cache_read: 1_000_000,
        output: 1_000_000,
        context_first: 20_000,
        context_last: 80_000,
        first_prompt: String::new(),
        path: format!("/gone/{run_id}.jsonl"),
        size: 1,
        modified_ns: 1,
    }
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(ticket(96, Some(103), Some(TaskLabel::Done)));
    board.add_ticket(ticket(97, Some(103), Some(TaskLabel::Claimed)));
    board.add_ticket(ticket(40, Some(37), Some(TaskLabel::Done)));
    board.add_ticket(ticket(41, Some(37), Some(TaskLabel::Done)));
    board
}

fn row_for(rows: &[EpicRow], epic: u64) -> &EpicRow {
    rows.iter()
        .find(|row| row.epic == epic)
        .unwrap_or_else(|| panic!("no row for #{epic}"))
}

#[test]
fn keeper_run_belongs_to_the_task_epic() {
    let rows = [
        run("team", Some("team"), Some(103), &[]),
        // A keeper asked from outside any instance holds no epic, only the task it was asked for.
        run("keeper", Some("keeper"), None, &[96]),
        // The board's epic for the task wins over an epic the row holds.
        run("keeper-2", Some("keeper"), Some(37), &[96]),
    ];
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    assert_eq!(rows.len(), 1, "all three belong to #103: {rows:?}");
    let epic = row_for(&rows, 103);
    assert_eq!(epic.turns, 30);
    let roles: Vec<&str> = epic.roles.iter().map(|role| role.role.as_str()).collect();
    assert_eq!(
        roles,
        ["keeper", "team"],
        "keeper has two of the three runs"
    );
}

#[test]
fn a_run_with_no_epic_on_the_board_or_the_row_is_outside_any_epic() {
    let rows = [
        run("loose", Some("keeper"), None, &[]),
        run("unknown", None, None, &[999]),
        run("planning", Some("planning"), Some(7), &[999]),
    ];
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    assert_eq!(
        rows.len(),
        1,
        "a task the board lacks falls back to the row"
    );
    assert_eq!(rows[0].epic, 7);
}

#[test]
fn each_row_holds_what_the_ticket_lists() {
    let mut sub = run("s/a", Some("programmer"), Some(103), &[96]);
    sub.parent_run = Some("s".to_owned());
    sub.turns = 30;
    sub.context_first = 26_000;
    sub.context_last = 90_000;
    let rows = [
        run("s", Some("team"), Some(103), &[96, 97]),
        sub,
        run("old", Some("team"), Some(37), &[40, 41]),
    ];
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    assert_eq!(rows.len(), 2);
    let epic = row_for(&rows, 103);
    assert_eq!(epic.tasks_done, 1, "#97 is claimed, not done");
    // opus: 4 + 5 + 0.2 + 20 dollars per million of each kind; two runs of 1M each.
    let one = 4.0 + 5.0 + 0.2 + 20.0;
    assert!((epic.est_usd - 2.0 * one).abs() < 1e-9, "{}", epic.est_usd);
    assert!((epic.usd_per_task.expect("per task") - 2.0 * one).abs() < 1e-9);
    assert_eq!(
        epic.tokens,
        Tokens {
            input: 2_000_000,
            cache_write: 2_000_000,
            cache_read: 2_000_000,
            output: 2_000_000
        }
    );
    assert_eq!(epic.turns_per_task, Some(40.0));
    assert_eq!(
        (epic.context_first, epic.context_last),
        (Some(26_000.0), Some(90_000.0)),
        "the subagent's, not the session's"
    );
    let shares: f64 = epic.roles.iter().map(|role| role.share_percent).sum();
    assert!((shares - 100.0).abs() < 1e-9);
    assert_eq!(epic.roles[0].share_percent, 50.0);
    let old = row_for(&rows, 37);
    assert_eq!(old.tasks_done, 2);
    assert_eq!(old.context_first, None, "no subagent ran in it");
}

#[test]
fn an_epic_with_no_task_done_has_no_per_task_figure() {
    let rows = [run("t", Some("planning"), Some(103), &[97])];
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    assert_eq!(rows[0].tasks_done, 0);
    assert_eq!(rows[0].usd_per_task, None);
    assert_eq!(rows[0].turns_per_task, None);
    assert!(render_text(&rows).contains("  -"), "a dash, not a zero");
}

/// A task ticket whose body reads, so the board gives it a size.
fn sized(n: u64, label: Option<TaskLabel>, size: &str) -> Ticket {
    let body = format!(
        "Goal: g\nOwned paths: p\nInterfaces: i\nDesign pages: d\nArchitecture: none\n\
         Acceptance:\n  - a\nTests: t\nSecurity trigger: no\nModel: sonnet\nSize: {size}\n\
         Blocked by: —\n"
    );
    Ticket {
        body,
        ..ticket(n, Some(103), label)
    }
}

/// What one run of `run` costs: 1M of each kind on opus.
const ONE_RUN: f64 = 4.0 + 5.0 + 0.2 + 20.0;

#[test]
fn median_splits_across_named_tasks() {
    let board = board();
    board.add_ticket(sized(98, Some(TaskLabel::Done), "S"));
    board.add_ticket(sized(99, Some(TaskLabel::Done), "M"));
    board.add_ticket(sized(100, Some(TaskLabel::Claimed), "S"));
    board.add_ticket(sized(101, Some(TaskLabel::Done), "S"));
    let rows = [
        // Two named tasks: half each, and the claimed one's half is in no median.
        run("a", Some("programmer"), Some(103), &[101, 100]),
        run("b", Some("programmer"), Some(103), &[98]),
        run("d", Some("programmer"), Some(103), &[99]),
        // Names nothing: split evenly across the epic's three done tasks.
        run("c", Some("planning"), Some(103), &[]),
    ];
    let rows = epic_rows(&rows, &board, &PriceTable::default()).expect("rows");
    let epic = row_for(&rows, 103);
    assert_eq!(epic.tasks_done, 3);
    let third = ONE_RUN / 3.0;
    let task_101 = ONE_RUN / 2.0 + third;
    let task_98 = ONE_RUN + third;
    let task_99 = ONE_RUN + third;
    let small = epic.median_usd.s.expect("two S tasks are done");
    assert!(
        (small - f64::midpoint(task_101, task_98)).abs() < 1e-9,
        "{small}"
    );
    let medium = epic.median_usd.m.expect("one M task is done");
    assert!((medium - task_99).abs() < 1e-9, "{medium}");
    let text = render_text(&rows);
    assert!(text.contains("med S $"), "{text}");
    assert!(text.contains(&format!("{small:.2}")), "{text}");
    let value: serde_json::Value = serde_json::from_str(&to_json(&rows)).expect("json");
    assert!(value["epics"][0]["median_usd"]["m"].is_number());
}

#[test]
fn unsized_task_is_in_no_median() {
    let board = board();
    board.add_ticket(sized(98, Some(TaskLabel::Done), "S"));
    // #96 is done but its body does not read, so it has no size.
    let rows = [
        run("a", Some("programmer"), Some(103), &[96]),
        run("b", Some("programmer"), Some(103), &[98]),
        run("c", Some("planning"), Some(103), &[]),
    ];
    let rows = epic_rows(&rows, &board, &PriceTable::default()).expect("rows");
    let epic = row_for(&rows, 103);
    assert_eq!(epic.tasks_done, 2);
    assert!(
        (epic.est_usd - 3.0 * ONE_RUN).abs() < 1e-9,
        "the unsized task's dollars are in the total"
    );
    // #98 alone: its own run plus half of the run that names nothing.
    let small = epic.median_usd.s.expect("one S task");
    assert!((small - 1.5 * ONE_RUN).abs() < 1e-9, "{small}");
    assert_eq!(epic.median_usd.m, None, "no M task is done");
    assert!(render_text(&rows).contains("  -"), "a dash for no median");
}

/// `count` done S tasks in `epic`, numbered from `first`, each with one run of its own that
/// costs `ONE_RUN` less `saved` (the output tokens dropped, at $20 per million).
fn small_epic(board: &FakeBoard, epic: u64, first: u64, count: u64, saved: bool) -> Vec<LedgerRow> {
    (first..first + count)
        .map(|task| {
            board.add_ticket(Ticket {
                epic: Some(number(epic)),
                ..sized(task, Some(TaskLabel::Done), "S")
            });
            let mut one = run(&format!("r{task}"), Some("programmer"), Some(epic), &[task]);
            if saved {
                one.output = 0;
            }
            one
        })
        .collect()
}

#[test]
fn five_tasks_give_a_verdict() {
    let board = board();
    let mut rows = small_epic(&board, 37, 200, 5, false);
    rows.extend(small_epic(&board, 103, 210, 5, true));
    rows.extend(small_epic(&board, 150, 220, 5, true));
    let rows = epic_rows(&rows, &board, &PriceTable::default()).expect("rows");
    let first = row_for(&rows, 37);
    assert_eq!(
        first.versus_last,
        SizeVerdicts::default(),
        "no earlier epic"
    );
    let cheaper = row_for(&rows, 103);
    assert_eq!(cheaper.done_by_size.s, 5);
    assert_eq!(cheaper.versus_last.s, Some(Verdict::Lower));
    // The same median as the epic before it is not lower.
    assert_eq!(row_for(&rows, 150).versus_last.s, Some(Verdict::NotLower));
    // No M task on either side: no verdict for M.
    assert_eq!(cheaper.versus_last.m, Some(Verdict::TooFew));
    let text = render_text(&rows);
    assert!(text.contains("lower"), "{text}");
    assert!(text.contains("not lower"), "{text}");
    let value: serde_json::Value = serde_json::from_str(&to_json(&rows)).expect("json");
    let versus = &value["epics"][1]["versus_last"];
    assert_eq!(versus["s"], "lower");
    assert_eq!(versus["m"], "too_few");
}

#[test]
fn four_tasks_say_too_few() {
    // Four on the later side.
    let board = board();
    let mut rows = small_epic(&board, 37, 200, 5, false);
    rows.extend(small_epic(&board, 103, 210, 4, true));
    let rows = epic_rows(&rows, &board, &PriceTable::default()).expect("rows");
    let later = row_for(&rows, 103);
    assert_eq!(later.done_by_size.s, 4);
    assert!(later.median_usd.s.is_some(), "the median is still shown");
    assert_eq!(later.versus_last.s, Some(Verdict::TooFew));
    assert!(render_text(&rows).contains("too few"));

    // Four on the earlier side.
    let board = self::board();
    let mut rows = small_epic(&board, 37, 200, 4, false);
    rows.extend(small_epic(&board, 103, 210, 5, true));
    let rows = epic_rows(&rows, &board, &PriceTable::default()).expect("rows");
    assert_eq!(row_for(&rows, 103).versus_last.s, Some(Verdict::TooFew));
}

fn transcript(prompt: &str) -> String {
    let user = json!({
        "type": "user",
        "timestamp": "2026-11-04T10:00:00.000Z",
        "message": {"role": "user", "content": prompt},
    });
    let turn = json!({
        "type": "assistant",
        "timestamp": "2026-11-04T10:00:05.000Z",
        "message": {
            "id": "m1",
            "model": OPUS,
            "usage": {
                "input_tokens": 1_000_000,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
                "output_tokens": 0,
            },
        },
    });
    format!("{user}\n{turn}\n")
}

#[test]
fn expired_epic_prints_from_the_ledger() {
    let temp = tempfile::tempdir().expect("temp dir");
    let projects = temp.path().join("projects");
    fs::create_dir_all(&projects).expect("projects");
    let file = projects.join("-demo").join("session.jsonl");
    fs::create_dir_all(file.parent().expect("folder")).expect("folder");
    fs::write(
        &file,
        transcript("You are team instance 15 on #103, for epic #103. Tasks: #96."),
    )
    .expect("transcript");
    let mut ledger = Ledger::open(&temp.path().join("home").join(LEDGER_FILE)).expect("open");
    ledger
        .ingest(&[Source {
            root: projects,
            project: Some("ways".to_owned()),
        }])
        .expect("ingest");
    fs::remove_file(&file).expect("the transcript expires");

    let rows = ledger.rows().expect("rows");
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    let epic = row_for(&rows, 103);
    assert_eq!(epic.tasks_done, 1);
    assert_eq!(epic.tokens.input, 1_000_000);
    assert!((epic.est_usd - 4.0).abs() < 1e-9, "1M input at $4");
    let text = render_text(&rows);
    assert!(text.contains("#103"), "{text}");
    assert!(text.contains("estimates"), "the output says so: {text}");
}

#[test]
fn price_change_moves_every_row_alike() {
    let rows = [
        run("a", Some("team"), Some(103), &[96]),
        run("b", Some("programmer"), Some(37), &[40]),
    ];
    let before = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    let doubled =
        PriceTable::from_local_toml("[report.prices]\nclaude-opus-5-5 = [8.0, 10.0, 0.4, 40.0]\n")
            .expect("table");
    let after = epic_rows(&rows, &board(), &doubled).expect("rows");
    assert_eq!(before.len(), after.len());
    for (old, new) in before.iter().zip(&after) {
        assert!(
            (new.est_usd - 2.0 * old.est_usd).abs() < 1e-9,
            "#{}: {} -> {}",
            old.epic,
            old.est_usd,
            new.est_usd
        );
        assert_eq!(new.tokens, old.tokens, "tokens do not move with a price");
        assert_eq!(new.roles[0].share_percent, old.roles[0].share_percent);
    }
}

#[test]
fn json_rows_carry_the_note_and_every_figure() {
    let rows = [run("s", Some("team"), Some(103), &[96])];
    let rows = epic_rows(&rows, &board(), &PriceTable::default()).expect("rows");
    let value: serde_json::Value = serde_json::from_str(&to_json(&rows)).expect("json");
    assert!(value["note"].as_str().expect("note").contains("estimates"));
    let epic = &value["epics"][0];
    for key in [
        "epic",
        "tasks_done",
        "est_usd",
        "usd_per_task",
        "tokens",
        "turns_per_task",
        "context_first",
        "context_last",
        "roles",
    ] {
        assert!(epic.get(key).is_some(), "missing {key}: {epic}");
    }
    assert_eq!(epic["tokens"]["cache_read"], 1_000_000);
    assert_eq!(epic["roles"][0]["role"], "team");
}

#[test]
fn a_board_failure_is_an_error_but_a_missing_task_is_not() {
    struct Down;
    impl Board for Down {
        fn task(&self, _: IssueNumber) -> Result<Ticket, PortError> {
            Err(PortError::Backend("gh is down".to_owned()))
        }
        fn ready(&self) -> Result<Vec<Ticket>, PortError> {
            unreachable!()
        }
        fn claim(
            &self,
            _: IssueNumber,
            _: &crate::domain::Instance,
        ) -> Result<crate::ports::ClaimOutcome, PortError> {
            unreachable!()
        }
        fn heartbeat(&self, _: IssueNumber, _: &crate::domain::Instance) -> Result<(), PortError> {
            unreachable!()
        }
        fn claims(&self) -> Result<Vec<crate::ports::ClaimRecord>, PortError> {
            unreachable!()
        }
        fn release(&self, _: IssueNumber, _: &crate::domain::Instance) -> Result<(), PortError> {
            unreachable!()
        }
        fn label(&self, _: IssueNumber, _: TaskLabel) -> Result<(), PortError> {
            unreachable!()
        }
        fn comment(&self, _: IssueNumber, _: &str) -> Result<(), PortError> {
            unreachable!()
        }
        fn report(
            &self,
            _: IssueNumber,
            _: &crate::domain::Instance,
            _: &str,
        ) -> Result<(), PortError> {
            unreachable!()
        }
        fn questions(&self) -> Result<Vec<Ticket>, PortError> {
            unreachable!()
        }
        fn open_question(&self, _: &crate::ports::NewQuestion) -> Result<IssueNumber, PortError> {
            unreachable!()
        }
        fn record_ruling(&self, _: IssueNumber, _: &str) -> Result<Option<IssueNumber>, PortError> {
            unreachable!()
        }
    }
    let rows = [run("s", Some("team"), Some(103), &[96])];
    let error = epic_rows(&rows, &Down, &PriceTable::default()).expect_err("fails");
    assert_eq!(error, PortError::Backend("gh is down".to_owned()));
}

#[test]
fn outside_any_epic_prints_one_line() {
    let rows = [
        run("in", Some("team"), Some(103), &[96]),
        run("loose", Some("keeper"), None, &[]),
        run("unknown", None, None, &[999]),
    ];
    let view = epic_view(&rows, &board(), &PriceTable::default()).expect("view");
    assert_eq!(view.rows.len(), 1);
    let outside = view.outside.as_ref().expect("two runs ran outside");
    // opus: 4 + 5 + 0.2 + 20 dollars per million of each kind; two runs of 1M each.
    let one = 4.0 + 5.0 + 0.2 + 20.0;
    assert_eq!((outside.runs, outside.turns), (2, 20));
    assert!(
        (outside.est_usd - 2.0 * one).abs() < 1e-9,
        "{}",
        outside.est_usd
    );
    assert_eq!(outside.tokens.input, 2_000_000);

    let text = view.render_text();
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("outside any epic"))
        .collect();
    assert_eq!(lines.len(), 1, "one line: {text}");
    assert!(lines[0].contains("$58.40"), "{}", lines[0]);
    assert_eq!(
        epic_rows(&rows, &board(), &PriceTable::default()).expect("rows"),
        view.rows,
        "the row per epic is unchanged"
    );

    let value: serde_json::Value = serde_json::from_str(&view.to_json()).expect("json");
    assert_eq!(value["outside_any_epic"]["runs"], 2);

    let inside = [run("in", Some("team"), Some(103), &[96])];
    let view = epic_view(&inside, &board(), &PriceTable::default()).expect("view");
    assert_eq!(view.outside, None);
    assert!(!view.render_text().contains("outside any epic"));
}
