use std::cell::Cell;
use std::time::UNIX_EPOCH;

use clap::Parser;

use super::*;
use crate::app::actions::Phase;
use crate::app::hub::{EpicRow, Snapshot, TicketRow, reports_file};
use crate::ports::PortError;

const MANIFEST: &str = "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n[spawn]\nslots = 2\n";

const SLICE: &str = ".zaf/workspaces/feature/0.2.0-103-322";

fn verb(args: &[&str]) -> Verb {
    Cli::try_parse_from(std::iter::once("zaf").chain(args.iter().copied()))
        .expect("parses")
        .verb
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn lines_of(state_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(reports_file(state_dir))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".zaf.toml"), MANIFEST).expect("manifest");
    dir
}

fn payload(tool: &str, command: Option<&str>, cwd: &Path) -> String {
    serde_json::json!({
        "tool_name": tool,
        "tool_input": { "command": command },
        "cwd": cwd,
    })
    .to_string()
}

#[test]
fn state_change_leaves_a_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases: [(&[&str], u8, &str); 13] = [
        (&["board", "claim", "7"], OK, "7 team claimed"),
        (&["board", "done", "7"], OK, "7 team finished"),
        (
            &["board", "question", "7", "which way?"],
            OK,
            "7 team question raised",
        ),
        (
            &["board", "ruling", "9", "that way"],
            OK,
            "- team question ruled",
        ),
        (&["slice", "open", "7"], OK, "7 team slice opened"),
        (&["slice", "pr", "7"], OK, "7 team pull request opened"),
        (&["slice", "merge", "7"], OK, "7 team merged"),
        (&["slice", "drop", "7"], OK, "7 team slice dropped"),
        (&["check", "7"], OK, "7 team testing"),
        (&["check", "pr:5:7"], OK, "7 team waiting on review"),
        // A check that finds a failure has still run.
        (&["check", "7"], REFUSED, "7 team testing"),
        (&["do", "slice-open", "7"], OK, "7 team slice opened"),
        (&["do", "slice-merge", "#7"], OK, "7 team merged"),
    ];
    for (index, (args, code, expected)) in cases.iter().enumerate() {
        assert!(
            leave_report_in(dir.path(), &verb(args), *code, None, at(100)),
            "{args:?} leaves a report"
        );
        let lines = lines_of(dir.path());
        assert_eq!(lines.len(), index + 1, "{args:?} adds exactly one line");
        assert_eq!(lines.last().expect("a line"), &format!("100 {expected}"));
    }
}

#[test]
fn a_verb_report_names_the_instance_role() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(leave_report_in(
        dir.path(),
        &verb(&["board", "claim", "7"]),
        OK,
        Some("programmer instance 2 on #103"),
        at(5),
    ));
    assert_eq!(lines_of(dir.path()), ["5 7 programmer claimed"]);
}

#[test]
fn a_refused_state_change_leaves_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    for args in [
        &["board", "claim", "7"][..],
        &["slice", "merge", "7"],
        &["board", "done", "7"],
    ] {
        assert!(!leave_report_in(
            dir.path(),
            &verb(args),
            REFUSED,
            None,
            at(1)
        ));
        assert!(!leave_report_in(
            dir.path(),
            &verb(args),
            BACKEND,
            None,
            at(1)
        ));
    }
    assert!(lines_of(dir.path()).is_empty());
}

#[test]
fn read_only_verb_leaves_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let reads: [&[&str]; 14] = [
        &["board", "ready"],
        &["board", "report", "7", "half done"],
        &["ask", "-t", "7", "fact: what?"],
        &["brief", "7"],
        &["check", "pr:5:7", "--status"],
        &["check", "7", "--status"],
        &["changes", "7"],
        &["asklog"],
        &["status"],
        &["report"],
        &["guard"],
        &["doctor"],
        &["do", "--show", "slice-open"],
        &["hub"],
    ];
    for args in reads {
        assert!(
            !leave_report_in(dir.path(), &verb(args), OK, None, at(1)),
            "{args:?} leaves nothing"
        );
    }
    // Nor does `zaf do` of an action that is not a state change, or the bare listing.
    for args in [&["do", "lint"][..], &["do"], &["hub", "report"]] {
        assert!(!leave_report_in(dir.path(), &verb(args), OK, None, at(1)));
    }
    assert!(!reports_file(dir.path()).exists(), "no file was even made");
}

#[test]
fn unwritable_report_never_fails_a_verb() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `hub` is a file, so the folder for the reports cannot be made.
    std::fs::write(dir.path().join("hub"), "in the way").expect("blocker");
    let claim = verb(&["board", "claim", "7"]);
    assert!(
        leave_report_in(dir.path(), &claim, OK, None, at(1)),
        "the report was due; failing to write it is not reported"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("hub")).expect("untouched"),
        "in the way"
    );
    // A state folder that is itself not a folder, and one that is gone.
    let file = dir.path().join("file");
    std::fs::write(&file, "x").expect("file");
    assert!(leave_report_in(&file, &claim, OK, None, at(1)));
    assert!(leave_report_in(
        &dir.path().join("gone").join("deeper"),
        &claim,
        OK,
        None,
        at(1)
    ));
    // The hook too: a project whose state folder cannot take a report.
    let project = project();
    std::fs::write(project.path().join(".zaf"), "in the way").expect("blocker");
    leave_tool_report(&payload("Read", None, project.path()), None, None, at(1));
}

#[test]
fn report_names_the_task_from_cwd() {
    let dir = project();
    let slice = dir.path().join(SLICE);
    let deep = slice.join("zaf").join("src");
    std::fs::create_dir_all(&deep).expect("workspace");
    let state = dir.path().join(".zaf");

    leave_tool_report(&payload("Edit", None, &deep), None, None, at(10));
    leave_tool_report(&payload("Read", None, &slice), None, None, at(11));
    // The workspace holds a copy of the manifest, as a clone of the project does.
    std::fs::write(slice.join(".zaf.toml"), MANIFEST).expect("copy");
    leave_tool_report(&payload("Grep", None, &deep), None, None, at(12));
    // Elsewhere in the project, the Engineer's own session, and a sibling that is no slice.
    leave_tool_report(&payload("Write", None, dir.path()), None, None, at(13));
    let other = dir.path().join(".zaf/workspaces/feature/not-a-slice");
    std::fs::create_dir_all(&other).expect("other");
    leave_tool_report(&payload("Read", None, &other), None, None, at(14));

    assert_eq!(
        lines_of(&state),
        [
            "10 322 programmer writing",
            "11 322 programmer reading",
            "12 322 programmer reading",
            "13 - team writing",
            "14 - team reading",
        ]
    );
}

#[test]
fn the_hooks_own_directory_stands_in_for_a_missing_cwd() {
    let dir = project();
    let slice = dir.path().join(SLICE);
    std::fs::create_dir_all(&slice).expect("workspace");
    leave_tool_report(r#"{"tool_name":"Glob"}"#, Some(&slice), None, at(3));
    leave_tool_report(
        r#"{"tool_name":"Glob","cwd":""}"#,
        Some(dir.path()),
        None,
        at(4),
    );
    assert_eq!(
        lines_of(&dir.path().join(".zaf")),
        ["3 322 programmer reading", "4 - team reading"]
    );
}

#[test]
fn report_carries_no_command_text() {
    let dir = project();
    let state = dir.path().join(".zaf");
    let secret = "SECRET_TOKEN_9f3a";
    for (tool, command) in [
        ("Bash", format!("cargo test --features {secret}")),
        (
            "Bash",
            format!("curl -H 'Authorization: {secret}' example.org"),
        ),
        ("Bash", format!("zaf check 322 {secret}")),
        ("Bash", format!("zaf check pr:5:322 {secret}")),
        ("Bash", format!("echo {secret}")),
        ("Read", secret.to_string()),
    ] {
        leave_tool_report(
            &payload(tool, Some(&command), dir.path()),
            None,
            None,
            at(7),
        );
    }
    let lines = lines_of(&state);
    assert_eq!(lines.len(), 6);
    for line in &lines {
        assert!(!line.contains(secret), "{line}");
        assert!(!line.contains("cargo") && !line.contains("curl"), "{line}");
        let report = crate::app::hub::Report::parse(line).expect("a report");
        assert!(
            ["testing", "waiting on review", "working", "reading"].contains(&report.what.as_str()),
            "only a general word: {line}"
        );
    }
}

#[test]
fn hook_never_fails() {
    let dir = project();
    let state = dir.path().join(".zaf");
    let outside = tempfile::tempdir().expect("outside");
    let here = Some(dir.path());
    let huge = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"{}"}}}}"#,
        "x".repeat(100_000)
    );
    let payloads = [
        "",
        "   ",
        "not json",
        "{",
        "[]",
        "null",
        "42",
        r#""a string""#,
        r#"{"tool_name":42,"cwd":7}"#,
        r#"{"tool_name":null,"tool_input":"x","cwd":null}"#,
        r#"{"tool_input":{"command":["a"]}}"#,
        "\u{0}\u{1}\u{2}",
        &huge,
    ];
    for text in payloads {
        leave_tool_report(text, here, None, at(1));
        leave_tool_report(text, None, None, at(1));
        leave_tool_report(text, Some(outside.path()), None, at(1));
    }
    // A cwd that is in no project leaves nothing anywhere.
    leave_tool_report(&payload("Read", None, outside.path()), None, None, at(1));
    assert!(!outside.path().join(".zaf").exists());
    // Every report that was left is a report the hub can read back.
    for line in lines_of(&state) {
        assert!(
            crate::app::hub::Report::parse(&line).is_some(),
            "unreadable: {line:?}"
        );
    }
    assert_eq!(lines_of(&state).len(), payloads.len(), "the `here` calls");
}

fn snapshot(epic: u64) -> Snapshot {
    Snapshot {
        epics: vec![EpicRow {
            number: epic,
            tickets: vec![TicketRow {
                number: 7,
                who: "sonnet programmer".to_owned(),
                phase: Phase::Build,
                word: "testing".to_owned(),
                since: Some(Duration::from_secs(10)),
            }],
            ..EpicRow::default()
        }],
    }
}

#[test]
fn piped_output_draws_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u32);
    let mut out = Vec::new();
    serve(
        &mut out,
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(103))
        },
        dir.path(),
        100,
        None,
    )
    .expect("serves");
    let text = String::from_utf8(out).expect("text");
    assert_eq!(calls.get(), 1, "the board is read once");
    assert_eq!(text, draw(&snapshot(103), 100), "one frame, as drawn");
    assert!(
        !text.contains('\x1b'),
        "no screen codes in a pipe: {text:?}"
    );
}

/// A viewer that does what a script says at each look, on a clock it owns.
struct Scripted<'a> {
    clock: SystemTime,
    state_dir: &'a Path,
    script: Vec<Look>,
}

#[derive(Clone, Copy)]
enum Look {
    /// Nothing happens.
    Nothing,
    /// This many seconds pass.
    Seconds(u64),
    /// A report is made.
    Report,
}

impl Pace for Scripted<'_> {
    fn now(&mut self) -> SystemTime {
        self.clock
    }

    fn width(&mut self) -> usize {
        100
    }

    fn wait(&mut self) -> bool {
        if self.script.is_empty() {
            return false;
        }
        match self.script.remove(0) {
            Look::Nothing => {}
            Look::Seconds(seconds) => self.clock += Duration::from_secs(seconds),
            Look::Report => record(
                self.state_dir,
                &Report {
                    at: self.clock,
                    task: None,
                    worker: "team".to_owned(),
                    what: "working".to_owned(),
                },
            ),
        }
        true
    }
}

fn frames_of(out: &[u8]) -> Vec<String> {
    String::from_utf8(out.to_vec())
        .expect("text")
        .split("\x1b[H\x1b[2J")
        .skip(1)
        .map(str::to_owned)
        .collect()
}

#[test]
fn report_redraws_the_frame() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    let mut pace = Scripted {
        clock: at(1_000),
        state_dir: dir.path(),
        script: vec![Look::Nothing, Look::Report, Look::Nothing],
    };
    let mut out = Vec::new();
    serve(
        &mut out,
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(100 + calls.get()))
        },
        dir.path(),
        100,
        Some(&mut pace),
    )
    .expect("serves");
    let frames = frames_of(&out);
    assert_eq!(frames.len(), 2, "the first frame, and the report's");
    assert!(frames[0].starts_with("epic #101"), "{}", frames[0]);
    assert!(
        frames[1].starts_with("epic #102"),
        "the board was read again, after the report: {}",
        frames[1]
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn no_network_between_reports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    let mut pace = Scripted {
        clock: at(1_000),
        state_dir: dir.path(),
        script: [Look::Seconds(1), Look::Nothing, Look::Seconds(30)]
            .into_iter()
            .cycle()
            .take(300)
            .collect(),
    };
    let mut out = Vec::new();
    serve(
        &mut out,
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(100 + calls.get()))
        },
        dir.path(),
        100,
        Some(&mut pace),
    )
    .expect("serves");
    assert_eq!(calls.get(), 1, "only the first frame read the board");
}

#[test]
fn clocks_tick_without_a_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    let mut pace = Scripted {
        clock: at(1_000),
        state_dir: dir.path(),
        script: vec![
            Look::Nothing,
            Look::Seconds(2),
            Look::Nothing,
            Look::Seconds(5),
        ],
    };
    let mut out = Vec::new();
    serve(
        &mut out,
        |_| {
            calls.set(calls.get() + 1);
            Ok(snapshot(103))
        },
        dir.path(),
        100,
        Some(&mut pace),
    )
    .expect("serves");
    let frames = frames_of(&out);
    let since: Vec<&str> = frames
        .iter()
        .map(|frame| {
            frame
                .lines()
                .find(|line| line.contains("#7"))
                .and_then(|line| line.split_whitespace().last())
                .expect("the ticket's clock")
        })
        .collect();
    assert_eq!(
        since,
        ["10s", "12s", "17s"],
        "the clocks moved, once per second drawn"
    );
    assert_eq!(calls.get(), 1, "and the board was not read");
}

#[test]
fn a_failing_board_leaves_the_last_frame_up() {
    let dir = tempfile::tempdir().expect("tempdir");
    let calls = Cell::new(0_u64);
    let mut pace = Scripted {
        clock: at(1_000),
        state_dir: dir.path(),
        script: vec![Look::Report, Look::Nothing, Look::Nothing],
    };
    let mut out = Vec::new();
    let outcome = serve(
        &mut out,
        |_| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                Ok(snapshot(103))
            } else {
                Err(HubError::Board(PortError::Unavailable("down".to_owned())))
            }
        },
        dir.path(),
        100,
        Some(&mut pace),
    );
    assert!(outcome.is_ok(), "the viewer lives on");
    assert_eq!(frames_of(&out).len(), 1, "no half frame");
    assert_eq!(
        calls.get(),
        2,
        "tried once for the report, not at every look"
    );
}
