use std::time::Duration;

use super::*;
use crate::app::schedule::gather::Unreadable;
use crate::app::schedule::shadow::Placed;
use crate::app::schedule::trace::{Did, KEEP_FOR, Seen};
use crate::domain::TicketId;
use crate::domain::schedule::{ActionName, Place, Weight};

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// 2026-11-12 17:15:00 UTC.
const AT: u64 = 1_794_503_700;

fn lane(ticket: u64, next: &str) -> Placed {
    Placed {
        ticket: TicketId::from(ticket),
        place: Place::InLane {
            next: ActionName::from(next),
            since: time(AT),
        },
        weight: Some(Weight::default()),
    }
}

fn entry(at: u64, placed: Vec<Placed>) -> Entry {
    Entry {
        at: time(at),
        seen: Ok(Seen {
            placed,
            unreadable: Vec::new(),
            done: 0,
        }),
        did: Did::default(),
        skipped: Vec::new(),
    }
}

fn lines(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(TICKS_FILE))
        .expect("the trace")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_first_tick_writes_one_line_on_which_every_ticket_is_new() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let appended = append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    assert_eq!(appended.failed, None);
    assert_eq!(appended.line.moved.len(), 1);
    assert_eq!(appended.line.moved[0].from, None);
    let written = lines(dir.path());
    assert_eq!(written, [appended.line.text()]);
    assert!(dir.path().join(TICKS_LOCK).exists());
}

#[test]
fn a_tick_appends_and_moves_against_the_line_before() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    // A tick that could not place leaves its line and moves nothing; the next still compares with
    // the last line that had tickets.
    let stopped = append(
        dir.path(),
        &Entry::stopped(time(AT + 60), "down"),
        time(AT + 60),
    );
    assert_eq!(stopped.line.moved, []);
    let appended = append(
        dir.path(),
        &entry(AT + 300, vec![lane(7, "implement"), lane(8, "brief")]),
        time(AT + 300),
    );
    let moved: Vec<String> = appended
        .line
        .moved
        .iter()
        .map(|moved| moved.line())
        .collect();
    assert_eq!(moved, ["#8 new -> lane(brief)"], "#7 stayed");
    assert_eq!(lines(dir.path()).len(), 3);
}

#[test]
fn lines_older_than_the_cut_are_dropped_and_the_file_is_written_whole() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let now = AT + KEEP_FOR.as_secs() + 600;
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    append(
        dir.path(),
        &entry(now - 60, vec![lane(7, "review")]),
        time(now - 60),
    );
    let appended = append(dir.path(), &entry(now, vec![lane(7, "review")]), time(now));
    assert_eq!(appended.failed, None);
    let written = lines(dir.path());
    assert_eq!(written.len(), 2, "{written:?}");
    let first = Recorded::parse(&written[0]).expect("a line");
    assert_eq!(first.at, time(now - 60));
    assert_eq!(
        appended.line.moved,
        [],
        "the kept line is the one compared with"
    );
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .expect("the folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert!(
        left.iter()
            .all(|name| !name.to_string_lossy().ends_with(".new")),
        "no staging file is left: {left:?}"
    );
}

#[test]
fn a_half_written_line_is_dropped_and_the_next_line_stays_whole() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    let path = dir.path().join(TICKS_FILE);
    let mut text = std::fs::read_to_string(&path).expect("the trace");
    text.push_str(r#"{"at":"2026-11-12T17:16:00Z","tick"#);
    std::fs::write(&path, text).expect("a crash mid-line");
    append(
        dir.path(),
        &entry(AT + 300, vec![lane(7, "implement")]),
        time(AT + 300),
    );
    let written = lines(dir.path());
    assert_eq!(written.len(), 2, "{written:?}");
    assert!(written.iter().all(|line| Recorded::parse(line).is_some()));
}

#[test]
fn a_last_line_cut_inside_a_character_fails_no_later_append_or_read() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    let path = dir.path().join(TICKS_FILE);
    let mut bytes = std::fs::read(&path).expect("the trace");
    // A crash mid-line, in the middle of a two-byte character.
    bytes.extend_from_slice(b"{\"at\":\"2026-11-12T17:16:00Z\",\"note\":\"caf\xc3");
    std::fs::write(&path, bytes).expect("a crash mid-character");

    let appended = append(
        dir.path(),
        &entry(AT + 300, vec![lane(7, "review")]),
        time(AT + 300),
    );

    assert_eq!(appended.failed, None);
    assert_eq!(appended.line.moved.len(), 1, "moved against the first line");
    let recorded = read(dir.path()).expect("reads").expect("a trace");
    assert_eq!(
        recorded.iter().map(|line| line.at).collect::<Vec<_>>(),
        [time(AT), time(AT + 300)]
    );
}

#[test]
fn a_trace_that_cannot_be_read_is_named_and_moves_nothing() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::create_dir(dir.path().join(TICKS_FILE)).expect("a folder where the file belongs");
    let appended = append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    let failed = appended.failed.expect("not written");
    assert!(failed.contains(TICKS_FILE), "{failed}");
    assert_eq!(appended.line.moved, []);
    assert!(read(dir.path()).is_err());
}

#[test]
fn a_trace_reads_back_passing_over_what_is_no_line() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    assert_eq!(read(dir.path()), Ok(None), "no file is no trace yet");
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    let path = dir.path().join(TICKS_FILE);
    let mut text = std::fs::read_to_string(&path).expect("the trace");
    text.push_str("not json\n");
    std::fs::write(&path, text).expect("garbage");
    let recorded = read(dir.path()).expect("reads").expect("a trace");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].at, time(AT));
}

#[test]
fn ticks_that_write_at_once_leave_every_line_whole() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::thread::scope(|scope| {
        for tick in 0..8 {
            let dir = dir.path();
            scope.spawn(move || {
                let mut placed = vec![lane(7, "implement")];
                placed.extend((0..20).map(|ticket| lane(100 + ticket, "brief")));
                let unreadable = Unreadable {
                    ticket: TicketId::from(50 + tick),
                    reason: "x".repeat(4096),
                };
                let mut entry = entry(AT + tick, placed);
                if let Ok(seen) = &mut entry.seen {
                    seen.unreadable.push(unreadable);
                }
                let appended = append(dir, &entry, time(AT + tick));
                assert_eq!(appended.failed, None);
            });
        }
    });
    let written = lines(dir.path());
    assert_eq!(written.len(), 8);
    assert!(written.iter().all(|line| Recorded::parse(line).is_some()));
}

#[test]
fn the_trace_and_its_lock_are_ignored_by_the_state_folders_allow_list() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
    };
    let Ok(init) = git(&["init", "--quiet"]) else {
        return;
    };
    assert!(init.status.success());
    let state = dir.path().join(".zaf");
    std::fs::create_dir_all(&state).expect("the state folder");
    let ignore: String = crate::app::setup::IGNORE_LINES
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(state.join(crate::app::setup::IGNORE_FILE), ignore).expect("the allow-list");
    for name in [TICKS_FILE, TICKS_LOCK] {
        std::fs::write(state.join(name), "").expect("a file");
        let checked = git(&["check-ignore", "--quiet", &format!(".zaf/{name}")]).expect("git");
        assert!(checked.status.success(), "{name} is not ignored");
    }
}

#[test]
fn the_last_tick_is_read_from_the_end_of_the_trace() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    assert_eq!(last_at(dir.path()), None, "no trace yet");
    append(dir.path(), &entry(AT, vec![lane(7, "implement")]), time(AT));
    append(
        dir.path(),
        &entry(AT + 60, vec![lane(7, "implement")]),
        time(AT + 60),
    );
    // A line being written is passed over.
    let path = dir.path().join(TICKS_FILE);
    let mut text = std::fs::read_to_string(&path).expect("the trace");
    text.push_str(r#"{"at":"2026-11-12T17:20:00Z","tick"#);
    std::fs::write(&path, text).expect("a line in flight");

    assert_eq!(last_at(dir.path()), Some(time(AT + 60)));
}
