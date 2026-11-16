use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::app::schedule::lock::{LOCK_FILE, SchedulerLock};
use crate::app::schedule::ticks::TICKS_FILE;

fn id(number: u64) -> TicketId {
    number.to_string().parse().expect("a ticket id")
}

fn trace(dir: &Path, lines: &[&str]) {
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(dir.join(TICKS_FILE), text).expect("the trace");
}

#[test]
fn records_a_start() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    trace(
        dir.path(),
        &[
            r#"{"at":"1970-01-01T00:01:40Z","did":{"started":{"task":3,"action":"implement","units":2}}}"#,
        ],
    );

    let last = TickRecord::last(dir.path()).expect("a record");

    assert!(last.started_task(id(3)), "{last:?}");
    assert!(!last.started_task(id(4)), "{last:?}");
    assert_eq!(last.at, UNIX_EPOCH + Duration::from_secs(100));
}

#[test]
fn records_a_skip_with_reason() {
    // The trace holds no skip and its reason: a tick that starts nothing leaves a line that names
    // no task. The record then says that tick started nothing, at its time.
    let dir = tempfile::tempdir().expect("a temporary directory");
    trace(dir.path(), &[r#"{"at":"1970-01-01T00:01:40Z"}"#]);

    let last = TickRecord::last(dir.path()).expect("a record");

    assert!(!last.started_task(id(3)), "{last:?}");
    assert_eq!(last.did, Did::default());
    assert_eq!(last.at, UNIX_EPOCH + Duration::from_secs(100));
}

#[test]
fn last_reads_newest() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    trace(
        dir.path(),
        &[
            r#"{"at":"1970-01-01T00:01:40Z","did":{"started":{"task":3,"action":"implement","units":2}}}"#,
            r#"{"at":"1970-01-01T00:03:20Z","did":{"waits":[{"task":5,"action":"ci"}]}}"#,
            "half a line {",
        ],
    );

    let last = TickRecord::last(dir.path()).expect("a record");

    assert_eq!(last.at, UNIX_EPOCH + Duration::from_secs(200));
    assert!(last.started_task(id(5)), "{last:?}");
    assert!(!last.started_task(id(3)), "{last:?}");
}

#[test]
fn last_is_none_without_a_trace() {
    let dir = tempfile::tempdir().expect("a temporary directory");

    assert_eq!(TickRecord::last(dir.path()), None);
}

#[test]
fn watch_running_follows_the_loop() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    assert!(!watch_running(dir.path()), "no scheduler yet");

    let me = process::holder_of(std::process::id()).expect("this process");
    let (lock, _) =
        SchedulerLock::take(dir.path(), &me, "0.3.1", "0.4.0", SystemTime::now()).expect("taken");
    assert!(
        watch_running(dir.path()),
        "{:?}",
        dir.path().join(LOCK_FILE)
    );

    lock.release();
    assert!(!watch_running(dir.path()), "stopped");
}
