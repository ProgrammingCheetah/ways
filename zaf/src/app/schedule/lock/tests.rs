use std::fs::File;
use std::time::{Duration, UNIX_EPOCH};

use super::*;

fn me() -> Holder {
    Holder {
        pid: 4242,
        start: "123456".to_owned(),
    }
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn line_in(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(LOCK_FILE)).expect("the lock file")
}

#[test]
fn a_free_lock_is_taken_and_its_line_names_the_holder() {
    let dir = tempfile::tempdir().expect("a temporary directory");

    let (lock, taken) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");

    assert_eq!(taken, Taken::Fresh);
    assert_eq!(
        line_in(dir.path()),
        "{\"pid\":4242,\"start\":\"123456\",\"since\":\"1970-01-01T00:16:40Z\",\"zaf\":\"0.3.1\",\
         \"version\":\"0.4.0\"}\n"
    );
    assert_eq!(lock.holding().holder(), me());
}

#[test]
fn a_second_scheduler_is_told_where_the_first_is() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (_first, _) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");
    let other = Holder {
        pid: 5151,
        start: "9".to_owned(),
    };

    let refused = SchedulerLock::take(dir.path(), &other, "0.3.1", "0.4.0", at(2_000));

    let Err(LockError::Held(Some(first))) = refused else {
        panic!("not refused with the first's line: {refused:?}");
    };
    assert_eq!(first.pid, 4242);
    assert_eq!(
        LockError::Held(Some(first)).to_string(),
        "a scheduler already runs here: pid 4242, since 1970-01-01T00:16:40Z (zaf 0.3.1, \
         --version 0.4.0)"
    );
    // The first's line is left as it was.
    assert!(line_in(dir.path()).contains("\"pid\":4242"));
}

#[test]
fn a_line_left_by_a_scheduler_that_died_is_taken_over() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let dead = Holding {
        pid: 999,
        start: "1".to_owned(),
        since: "1970-01-01T00:00:10Z".to_owned(),
        zaf: "0.3.0".to_owned(),
        version: "0.3.0".to_owned(),
    };
    std::fs::write(dir.path().join(LOCK_FILE), dead.line()).expect("a stale line");

    let (lock, taken) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");

    assert_eq!(taken, Taken::TookOver(dead));
    assert_eq!(lock.holding().since, "1970-01-01T00:16:40Z");
    assert!(line_in(dir.path()).contains("\"pid\":4242"));
}

#[test]
fn the_same_process_taking_it_again_is_a_restart_that_keeps_since() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (old, _) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");
    // The old image's file goes with the exec, the line stays.
    drop(old);

    let (lock, taken) =
        SchedulerLock::take(dir.path(), &me(), "0.3.2", "0.4.0", at(5_000)).expect("taken");

    assert_eq!(
        taken,
        Taken::Restarted {
            from: "0.3.1".to_owned()
        }
    );
    assert_eq!(lock.holding().since, "1970-01-01T00:16:40Z");
    assert_eq!(lock.holding().zaf, "0.3.2");
}

#[test]
fn a_reader_sees_a_scheduler_only_while_its_process_lives() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    assert_eq!(running(dir.path(), &|_| true), None, "no file");
    let (lock, _) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");

    assert_eq!(
        running(dir.path(), &|holder| *holder == me()).map(|holding| holding.pid),
        Some(4242)
    );
    assert_eq!(
        running(dir.path(), &|_| false),
        None,
        "a holder that is gone"
    );

    lock.release();
    assert_eq!(
        line_in(dir.path()),
        "",
        "a stopped scheduler clears its line"
    );
    assert_eq!(running(dir.path(), &|_| true), None);
    // And the lock is free again.
    SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(2_000)).expect("free again");
}

#[test]
fn a_live_line_whose_lock_nobody_holds_is_no_scheduler() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    // A line naming a live process (`alive` says every process lives), but no lock behind it: an
    // older binary that holds none, or a pid and start another process has after a reboot.
    let line = Holding {
        pid: 4242,
        start: "123456".to_owned(),
        since: "1970-01-01T00:16:40Z".to_owned(),
        zaf: "0.3.1".to_owned(),
        version: "0.4.0".to_owned(),
    };
    std::fs::write(dir.path().join(LOCK_FILE), line.line()).expect("a live line");

    assert_eq!(running(dir.path(), &|_| true), None);
    // The look let the lock go: a scheduler takes it, and is then seen.
    let (_lock, taken) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(2_000)).expect("taken");
    assert_eq!(
        taken,
        Taken::Restarted {
            from: "0.3.1".to_owned()
        }
    );
    assert_eq!(
        running(dir.path(), &|_| true).map(|holding| holding.pid),
        Some(4242)
    );
}

#[test]
fn a_start_rides_over_a_readers_moment_on_the_lock() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(dir.path().join(LOCK_FILE), "").expect("the lock file");
    // A reader's shared lock, held a little longer than any real look holds it.
    let reader = File::open(dir.path().join(LOCK_FILE)).expect("open");
    reader.lock_shared().expect("a shared lock");
    let looking = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        drop(reader);
    });

    let taken = SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000));

    looking.join().expect("the reader");
    assert!(taken.is_ok(), "{taken:?}");
}

#[test]
fn a_reader_never_finds_the_line_half_rewritten() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (mut lock, _) =
        SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1_000)).expect("taken");
    // Two lines of different lengths, rewritten over each other as fast as the writer can.
    let short = lock.holding().line();
    let long = Holding {
        version: "0.4.0-a-milestone-with-a-long-name".to_owned(),
        ..lock.holding().clone()
    }
    .line();
    let writer = std::thread::spawn(move || {
        for turn in 0..20_000 {
            let line = if turn % 2 == 0 { &long } else { &short };
            rewrite(&mut lock.file, line).expect("rewritten");
        }
        lock
    });

    let path = dir.path().join(LOCK_FILE);
    let mut reads = 0;
    while !writer.is_finished() {
        let text = std::fs::read_to_string(&path).expect("the lock file");
        // The head of the line is the same in both, so a read torn between them still shows it.
        assert!(text.starts_with("{\"pid\":4242,"), "read {text:?}");
        reads += 1;
    }
    let lock = writer.join().expect("the writer");
    assert!(reads > 10, "only {reads} reads while it rewrote");
    assert_eq!(line_in(dir.path()), lock.holding().line());
}

#[test]
fn the_lock_and_the_request_file_are_ignored_by_the_state_folders_allow_list() {
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
    for name in [LOCK_FILE, crate::app::schedule::request::REQUEST_FILE] {
        std::fs::write(state.join(name), "").expect("a file");
        let checked = git(&["check-ignore", "--quiet", &format!(".zaf/{name}")]).expect("git");
        assert!(checked.status.success(), "{name} is not ignored");
    }
}
