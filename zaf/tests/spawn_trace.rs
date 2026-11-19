//! The trace of where everything was, through the binary (docs/zaf-actions.md §5.1, R24): every
//! real tick appends one line to `.zaf/ticks.jsonl` (where the core places each ticket, what the
//! tick did, what moved), drops the lines older than 14 days, and prints each move with a count
//! line; `zaf spawn trace` reads the moves back. A dry run writes none, a tick that cannot gather
//! still leaves a line, and a trace that cannot be written is a warning that leaves the exit code.
//! A scheduler's tick that cannot read the facts keeps the placement it last read, starts nothing,
//! and says so in its block and its line; the next tick reads them again.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use common::{Project, Running, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [phases]\nTask = [\"merge\"]\n";

const SPAWN: [&str; 4] = ["spawn", "--once", "--version", "0.1"];

/// The count line of the scene's first tick: #7 in the lane, #8 waiting on it.
const FIRST_COUNT: &str = "2 tickets: 0 running, 1 in the lane, 1 waiting, 0 done";

/// A task body of size S naming `blocked_by` as its blockers.
fn task_body(blocked_by: &str) -> String {
    format!(
        "Goal: Members export their reading list as CSV.\n\
         Owned paths: src/export/**\n\
         Interfaces: none\n\
         Design pages: none\n\
         Architecture: app over ports\n\
         Acceptance:\n  - an empty list exports the header row only\n\
         Tests: beside the use case\n\
         Security trigger: no\n\
         Model: sonnet\n\
         Size: S\n\
         Blocked by: {blocked_by}\n"
    )
}

/// A project with current profiles and a `zaf`-kind `merge` as its one phase: task 7 (of epic 1)
/// is claimed, task 8 is blocked by it. Every other `gh` call answers an empty list. Its clock is
/// UTC, so the trace's times read alike wherever the suite runs.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.env("TZ", "UTC");
    let ticket = |number: u64, state: &str, blocked_by: &str| {
        serde_json::json!({
            "number": number,
            "title": format!("Task {number}"),
            "body": task_body(blocked_by),
            "labels": [{"name": "task"}, {"name": state}],
            "parent": {"number": 1},
            "state": "OPEN",
            "comments": [{"body": "claimed by team instance 1 on #1 at 2027-01-15T08:00:00Z"}],
            "issueType": {"name": "Task"},
        })
    };
    let (seven, eight) = (ticket(7, "claimed", "none"), ticket(8, "blocked", "#7"));
    project
        .on(
            "issue list --label task --state open *",
            &serde_json::json!([seven, eight]).to_string(),
        )
        .on("issue list --label claimed *", r#"[{"number":7}]"#)
        .on("issue view 7 *", &seven.to_string())
        .on("issue view 8 *", &eight.to_string())
        .on("*", "[]");
    project.provide_slice_actions();
    let file = project.root.join(".zaf/actions/merge");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
         # zaf kind: zaf\n# zaf ends: merged\ntrue\n",
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

fn trace_file(project: &Project) -> PathBuf {
    project.root.join(".zaf/ticks.jsonl")
}

/// The trace's lines, each parsed.
fn trace(project: &Project) -> Vec<serde_json::Value> {
    fs::read_to_string(trace_file(project))
        .expect("the trace")
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line of JSON"))
        .collect()
}

/// Runs one real tick, which must pass; gives its stdout.
fn tick(project: &Project, more: bool) -> String {
    let mut args = Vec::new();
    if more {
        args.push("--more");
    }
    args.extend(SPAWN);
    let (code, stdout, stderr) = outcome(&project.zaf(&args));
    assert_eq!(code, 0, "{stdout}{stderr}");
    stdout
}

/// A long-lived process of its own group, standing for a phase's process.
fn live_child() -> Child {
    Command::new("sh")
        .args(["-c", "sleep 60 & wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("child")
}

/// Writes a live phase run folder of task 7 at `merge` under `root`, held by `child`.
fn run_merge(root: &Path, child: &Child) {
    let run = root.join(".zaf/phases/merge-instance-9-on-1-task-7");
    fs::create_dir_all(&run).expect("run folder");
    let holder = zaf::adapters::process::holder_of(child.id()).expect("the child runs");
    fs::write(run.join("action"), "merge").expect("action");
    fs::write(run.join("head"), "").expect("head");
    fs::write(run.join("size"), "S").expect("size");
    fs::write(run.join("instance"), "merge instance 9 on #1").expect("instance");
    fs::write(run.join("pid"), holder.to_string()).expect("pid");
    fs::write(run.join("task"), "7").expect("task");
}

/// Whether a listing line is a date line (`2026-11-13`), not a move (`20:15 #7 ...`): a time of
/// day in the twentieth hour also starts with `20`.
fn is_date_line(line: &str) -> bool {
    line.as_bytes().get(4) == Some(&b'-')
}

/// Two ticks: the first sees #7 in the lane and #8 waiting on it, then #7 starts running.
fn two_ticks(project: &Project) -> (String, String) {
    let first = tick(project, false);
    let mut child = live_child();
    run_merge(&project.root, &child);
    let second = tick(project, false);
    let _ = child.kill();
    let _ = child.wait();
    (first, second)
}

#[test]
fn a_tick_appends_where_every_ticket_sits() {
    let project = project();

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["--json", "spawn", "--once", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    let lines = trace(&project);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(line["at"].as_str().expect("at").ends_with('Z'), "{line}");
    assert_eq!(line["zaf"], env!("CARGO_PKG_VERSION"));
    let tickets = line["tickets"].as_array().expect("tickets");
    assert_eq!(tickets.len(), 2, "{line}");
    assert_eq!(tickets[0]["ticket"], 7);
    assert_eq!(tickets[0]["place"], "lane");
    assert_eq!(tickets[0]["next"], "merge");
    assert!(tickets[0]["since"].is_string(), "{line}");
    assert!(tickets[0]["weight"].is_u64(), "{line}");
    assert_eq!(tickets[0]["short"], "lane(merge)");
    assert_eq!(
        tickets[1],
        serde_json::json!({
            "ticket": 8, "place": "waiting", "on": [{"ticket": 7}], "short": "waiting(ticket #7)",
        })
    );
    assert_eq!(line["unreadable"], serde_json::json!([]));
    // What the tick did: it ran #7's merge inline.
    assert_eq!(
        line["did"]["ran"],
        serde_json::json!([{"task": 7, "action": "merge", "exit": 0}])
    );
    assert_eq!(line["did"]["started"], serde_json::Value::Null);
    assert_eq!(line["skipped"], serde_json::json!([]));
    // The old scheduler is gone, and its word with it: the placement decides.
    assert!(line.get("old").is_none(), "{line}");
    // The first line of all: both tickets are new.
    assert_eq!(
        line["moved"],
        serde_json::json!([
            {"ticket": 7, "from": null, "to": "lane(merge)"},
            {"ticket": 8, "from": null, "to": "waiting(ticket #7)"},
        ])
    );
    assert!(line.get("error").is_none(), "{line}");

    // `--json` carries the same `moved` and `tickets`, beside its other keys.
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    assert_eq!(json["moved"], line["moved"]);
    assert_eq!(json["tickets"], line["tickets"]);
    assert!(json.get("tick").is_some(), "{json}");
}

#[test]
fn a_dry_run_writes_no_trace() {
    let project = project();

    let mut dry = SPAWN.to_vec();
    dry.push("--dry-run");
    let (code, stdout, stderr) = outcome(&project.zaf(&dry));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let mut json = vec!["--json"];
    json.extend(&dry);
    let (code, stdout, stderr) = outcome(&project.zaf(&json));
    assert_eq!(code, 0, "{stdout}{stderr}");

    assert!(!trace_file(&project).exists());
    assert!(!project.root.join(".zaf/ticks.lock").exists());
}

#[test]
fn the_tick_prints_each_move_and_a_count() {
    let project = project();

    let (first, second) = two_ticks(&project);

    let lines: Vec<&str> = first.lines().collect();
    assert!(lines[0].starts_with("tick "), "{first}");
    assert!(lines[1].starts_with("slots "), "{first}");
    assert_eq!(
        &lines[2..5],
        [
            "#7 new -> lane(merge)",
            "#8 new -> waiting(ticket #7)",
            FIRST_COUNT
        ],
        "{first}"
    );
    // The block's own lines are all still there, after the places.
    assert!(lines.contains(&"ran merge #7: exit 0"), "{first}");

    let lines: Vec<&str> = second.lines().collect();
    assert_eq!(
        &lines[2..4],
        [
            "#7 lane(merge) -> running(merge)",
            "2 tickets: 1 running, 0 in the lane, 1 waiting, 0 done",
        ],
        "{second}"
    );
    assert!(
        !second.contains("#8 "),
        "#8 stayed, so it has no line: {second}"
    );
    assert_eq!(trace(&project).len(), 2);
}

#[test]
fn more_prints_every_ticket() {
    let project = project();
    // #7 runs through both ticks, so the second moves nothing.
    let mut child = live_child();
    run_merge(&project.root, &child);
    tick(&project, false);

    let stdout = tick(&project, true);
    let _ = child.kill();
    let _ = child.wait();

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        &lines[2..5],
        [
            "#7 running(merge)",
            "#8 waiting(ticket #7)",
            "2 tickets: 1 running, 0 in the lane, 1 waiting, 0 done",
        ],
        "{stdout}"
    );
}

/// A trace line at `at` that placed #7 in the lane at merge.
fn line_at(at: SystemTime) -> String {
    serde_json::json!({
        "at": humantime::format_rfc3339_seconds(at).to_string(),
        "zaf": "0.0.0",
        "tickets": [{"ticket": 7, "place": "lane", "next": "merge", "short": "lane(merge)"}],
        "unreadable": [],
        "did": {},
        "skipped": [],
        "moved": [{"ticket": 7, "from": null, "to": "lane(merge)"}],
    })
    .to_string()
}

#[test]
fn lines_older_than_fourteen_days_are_dropped_when_a_tick_writes() {
    let project = project();
    let now = SystemTime::now();
    let old = line_at(now - Duration::from_secs(15 * 24 * 3600));
    let recent = line_at(now - Duration::from_secs(3600));
    fs::write(trace_file(&project), format!("{old}\n{recent}\n")).expect("an older trace");

    let stdout = tick(&project, false);

    let text = fs::read_to_string(trace_file(&project)).expect("the trace");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0], recent, "the recent line is kept as it was");
    // The new line moves against the recent one: #7 stayed in the lane, #8 is new.
    let line: serde_json::Value = serde_json::from_str(lines[1]).expect("the new line");
    assert_eq!(
        line["moved"],
        serde_json::json!([{"ticket": 8, "from": null, "to": "waiting(ticket #7)"}])
    );
    assert!(!stdout.contains("#7 "), "{stdout}");
    let left: Vec<String> = fs::read_dir(project.root.join(".zaf"))
        .expect("the state folder")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("ticks.jsonl."))
        .collect();
    assert!(left.is_empty(), "no staging file is left: {left:?}");
}

#[test]
fn a_trace_that_cannot_be_written_warns_and_the_tick_still_succeeds() {
    let project = project();
    fs::create_dir_all(trace_file(&project)).expect("a folder where the trace belongs");

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 0, "{stdout}{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("warning: the trace was not written"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    assert!(warnings[0].contains("ticks.jsonl"), "{stderr}");
    // The tick ran and printed its block, its count line among it; with the trace unread nothing
    // is a move.
    assert!(stdout.contains("ran merge #7: exit 0"), "{stdout}");
    assert!(stdout.contains(FIRST_COUNT), "{stdout}");
    assert!(!stdout.contains(" -> "), "{stdout}");
}

#[test]
fn a_tick_that_cannot_gather_still_leaves_a_line() {
    let mut project = project();
    project.fail_first("*", "HTTP 502: the board is down");

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 2, "{stdout}{stderr}");
    let lines = trace(&project);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    let error = line["error"].as_str().expect("an error");
    assert!(error.contains("the board is down"), "{line}");
    assert!(line.get("tickets").is_none(), "{line}");
    assert_eq!(line["moved"], serde_json::json!([]));
}

#[test]
fn spawn_trace_prints_the_moves_oldest_first() {
    let project = project();
    two_ticks(&project);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "trace"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    // Each line is `HH:MM <move>`; a date line appears only for a suite run across midnight.
    let moves: Vec<&str> = stdout
        .lines()
        .filter(|line| !is_date_line(line))
        .map(|line| {
            assert_eq!(line.as_bytes()[2], b':', "{stdout}");
            &line[6..]
        })
        .collect();
    assert_eq!(
        moves,
        [
            "#7 new -> lane(merge)",
            "#8 new -> waiting(ticket #7)",
            "#7 lane(merge) -> running(merge)",
        ],
        "{stdout}"
    );

    let (code, more, stderr) = outcome(&project.zaf(&["--more", "spawn", "trace"]));
    assert_eq!(code, 0, "{more}{stderr}");
    let lines: Vec<&str> = more.lines().filter(|line| !is_date_line(line)).collect();
    assert_eq!(lines[2], "      ran merge #7: exit 0", "{more}");

    let (code, json, stderr) = outcome(&project.zaf(&["--json", "spawn", "trace"]));
    assert_eq!(code, 0, "{json}{stderr}");
    let json: serde_json::Value = serde_json::from_str(&json).expect("one JSON document");
    let moves = json["moves"].as_array().expect("moves");
    assert_eq!(moves.len(), 3, "{json}");
    assert_eq!(moves[2]["ticket"], 7);
    assert_eq!(moves[2]["from"], "lane(merge)");
    assert_eq!(moves[2]["to"], "running(merge)");
}

#[test]
fn spawn_trace_keeps_one_ticket() {
    let project = project();
    two_ticks(&project);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "trace", "8"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    let moves: Vec<&str> = stdout.lines().filter(|line| !is_date_line(line)).collect();
    assert_eq!(moves.len(), 1, "{stdout}");
    assert!(
        moves[0].ends_with(" #8 new -> waiting(ticket #7)"),
        "{stdout}"
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "trace", "#9"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "no moves of #9\n");
}

#[test]
fn spawn_trace_since_keeps_the_later_ticks() {
    let project = project();
    let at = |text: &str| humantime::parse_rfc3339(text).expect("a time");
    let earlier = line_at(at("2026-11-12T17:15:00Z"));
    let later = serde_json::json!({
        "at": "2026-11-12T17:20:00Z",
        "tickets": [{"ticket": 7, "short": "running(merge)"}],
        "moved": [{"ticket": 7, "from": "lane(merge)", "to": "running(merge)"}],
    });
    fs::write(trace_file(&project), format!("{earlier}\n{later}\n")).expect("a trace");

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["spawn", "trace", "--since", "2026-11-12T17:16:00Z"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "17:20 #7 lane(merge) -> running(merge)\n");
    let (code, _, stderr) = outcome(&project.zaf(&["spawn", "trace", "--since", "soon"]));
    assert_eq!(code, 2, "a malformed command line: {stderr}");
    assert!(stderr.contains("`soon`"), "{stderr}");
}

#[test]
fn spawn_trace_with_no_file_says_so_and_exits_zero() {
    let project = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "trace"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        stdout,
        "no trace yet: no tick has written .zaf/ticks.jsonl\n"
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["--json", "spawn", "trace"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    assert_eq!(json, serde_json::json!({"moves": []}));

    fs::write(trace_file(&project), "").expect("an empty trace");
    let (code, stdout, _) = outcome(&project.zaf(&["spawn", "trace"]));
    assert_eq!(code, 0);
    assert!(stdout.starts_with("no trace yet"), "{stdout}");
    assert!(
        !project.root.join(".zaf/ticks.lock").exists(),
        "it takes no lock"
    );
}

#[test]
fn a_tick_that_cannot_read_the_facts_keeps_the_last_placement_and_starts_nothing() {
    let mut project = project();
    // `merge` fails, so #7 stays in the lane at it and each tick that reads the facts runs it
    // again.
    let file = project.root.join(".zaf/actions/merge");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: merge the task's slice\n# zaf 1: failed\n\
         # zaf kind: zaf\n# zaf ends: merged\necho run >> \"$ZAF_ROOT/runs\"\nexit 1\n",
    )
    .expect("action");
    let runs_file = project.root.join("runs");
    let runs = || {
        fs::read_to_string(&runs_file)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    // A fresh scheduler opens the build first.
    project.ready_to_build("0.1", true);
    let running = Running::start(
        project.command_in(&project.root, &["spawn", "--watch", "--version", "0.1"]),
    );
    running.wait_for_lines("next tick ", 1);
    assert_eq!(runs(), 1, "{}", running.stdout());
    let out = running.stdout();
    let first = out
        .lines()
        .find_map(|line| line.strip_prefix("tick "))
        .expect("the first tick's line")
        .to_owned();

    // The board goes down.
    project.fail_first(
        "issue list --label task --state open *",
        "HTTP 502: the board is down",
    );
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    running.wait_for_lines("next tick ", 2);

    let out = running.stdout();
    let said = out
        .lines()
        .find(|line| line.starts_with("could not read the facts: "))
        .unwrap_or_else(|| panic!("no word of it:\n{out}"));
    assert!(said.contains("the board is down"), "{said}");
    assert!(
        said.ends_with(&format!(
            "; kept the placement of {first} and started no phase"
        )),
        "{said}"
    );
    assert_eq!(runs(), 1, "nothing started: {out}");
    let lines = trace(&project);
    let error = lines
        .last()
        .and_then(|line| line["error"].as_str())
        .expect("the line names its error");
    assert!(error.contains("the board is down"), "{error}");
    assert!(error.contains("; kept the placement of "), "{error}");
    assert!(
        running
            .stderr()
            .contains("tick 2 failed with exit 2; the watch goes on"),
        "{}",
        running.stderr()
    );

    // The board is back: the facts win, and the next tick acts on them.
    project.on_first(
        "issue list --label task --state open *",
        &fs::read_to_string(project.temp.path().join("gh-scene/response-0")).expect("the list"),
    );
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.1"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    running.wait_for_lines("next tick ", 3);
    assert_eq!(runs(), 2, "{}", running.stdout());
}
