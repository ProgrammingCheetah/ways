use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::app::schedule::lock::SchedulerLock;

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn request(seconds: u64, who: &str) -> Request {
    Request {
        at: at(seconds),
        who: who.to_owned(),
    }
}

fn whos(requests: &[Request]) -> Vec<&str> {
    requests
        .iter()
        .map(|request| request.who.as_str())
        .collect()
}

fn me() -> Holder {
    Holder {
        pid: 4242,
        start: "1".to_owned(),
    }
}

#[test]
fn a_request_is_one_line_naming_when_and_who() {
    let line = request(1_000, "phase-end implement #7").line();

    assert_eq!(line, "1970-01-01T00:16:40Z phase-end implement #7\n");
    assert_eq!(
        Request::parse(&line),
        Some(request(1_000, "phase-end implement #7"))
    );
    assert_eq!(
        request(1, "two\nlines").line(),
        "1970-01-01T00:00:01Z two lines\n"
    );
    assert_eq!(Request::parse("not a time"), None);
    assert_eq!(Request::parse("1970-01-01T00:00:01Z "), None);
}

#[test]
fn with_no_scheduler_up_nothing_is_written() {
    let dir = tempfile::tempdir().expect("a temporary directory");

    let asked = ask(dir.path(), "ruling #45", at(1), &|_| true).expect("no failure");

    assert_eq!(asked, None);
    assert!(!dir.path().join(REQUEST_FILE).exists());
}

#[test]
fn with_a_scheduler_up_the_request_is_appended() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (_lock, _) = SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1)).expect("lock");

    let asked = ask(dir.path(), "ruling #45", at(2), &|_| true).expect("asked");
    ask(dir.path(), "sign #1", at(3), &|_| true).expect("asked");

    assert_eq!(asked.map(|holding| holding.pid), Some(4242));
    assert_eq!(
        std::fs::read_to_string(dir.path().join(REQUEST_FILE)).expect("the file"),
        "1970-01-01T00:00:02Z ruling #45\n1970-01-01T00:00:03Z sign #1\n"
    );
}

#[test]
fn a_look_reads_each_complete_line_once() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (mut requests, pending) = Requests::start(dir.path()).expect("started");
    assert!(pending.is_empty());
    assert!(
        requests.look().expect("a look").requests.is_empty(),
        "no file yet"
    );

    append(dir.path(), &request(1, "ruling #45")).expect("appended");
    append(dir.path(), &request(2, "sign #1")).expect("appended");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["ruling #45", "sign #1"]
    );
    assert!(
        requests.look().expect("a look").requests.is_empty(),
        "read once"
    );

    // A line a writer has not finished waits for the next look.
    let path = dir.path().join(REQUEST_FILE);
    let mut file = OpenOptions::new().append(true).open(&path).expect("open");
    file.write_all(b"1970-01-01T00:00:03Z phase-end")
        .expect("half");
    assert!(requests.look().expect("a look").requests.is_empty());
    file.write_all(b" implement #7\n").expect("the rest");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["phase-end implement #7"]
    );
}

#[test]
fn a_start_names_what_was_left_and_empties_the_file() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    append(dir.path(), &request(1, "ruling #45")).expect("appended");

    let (mut requests, pending) = Requests::start(dir.path()).expect("started");

    assert_eq!(whos(&pending), ["ruling #45"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(REQUEST_FILE)).expect("the file"),
        ""
    );
    append(dir.path(), &request(2, "sign #1")).expect("appended");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["sign #1"]
    );
}

#[test]
fn past_the_bound_the_file_is_emptied_and_nothing_is_lost() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (mut requests, _) = Requests::start(dir.path()).expect("started");
    let line = request(1, "claimed #7").line();
    let count = usize::try_from(EMPTY_PAST).expect("small") / line.len() + 1;
    for _ in 0..count {
        append(dir.path(), &request(1, "claimed #7")).expect("appended");
    }

    let looked = requests.look().expect("a look");

    assert_eq!(looked.not_emptied, None);
    let read = looked.requests;
    assert_eq!(read.len(), count, "every line read");
    let path = dir.path().join(REQUEST_FILE);
    assert_eq!(
        std::fs::metadata(&path).expect("the file").len(),
        0,
        "emptied"
    );
    append(dir.path(), &request(2, "sign #1")).expect("appended");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["sign #1"]
    );
}

#[test]
fn a_scheduler_that_lets_go_leaves_only_what_no_tick_served() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (mut requests, _) = Requests::start(dir.path()).expect("started");
    append(dir.path(), &request(1, "ruling #45")).expect("appended");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["ruling #45"]
    );
    // Asked after the last look, and a line still being written.
    append(dir.path(), &request(2, "sign #1")).expect("appended");
    let path = dir.path().join(REQUEST_FILE);
    let mut file = OpenOptions::new().append(true).open(&path).expect("open");
    file.write_all(b"1970-01-01T00:00:03Z phase-end")
        .expect("half");

    requests.let_go().expect("let go");

    assert_eq!(
        std::fs::read_to_string(&path).expect("the file"),
        "1970-01-01T00:00:02Z sign #1\n1970-01-01T00:00:03Z phase-end"
    );
    // The next start, a restart included, names what no tick served.
    file.write_all(b" implement #7\n").expect("the rest");
    let (_, pending) = Requests::start(dir.path()).expect("started again");
    assert_eq!(whos(&pending), ["sign #1", "phase-end implement #7"]);
}

#[test]
fn after_a_restart_that_failed_the_requests_left_are_read_again() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(REQUEST_FILE);
    let (mut requests, _) = Requests::start(dir.path()).expect("started");
    append(dir.path(), &request(1, "ruling #45")).expect("appended");
    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["ruling #45"]
    );
    let looked = std::fs::metadata(&path).expect("the file");
    // Asked after the last look, a line as long as the one read: not yet read when the scheduler
    // lets go to restart.
    append(dir.path(), &request(1, "ruling #46")).expect("appended");

    requests.let_go().expect("let go");
    // The file now looks as it did at the last look: as long, and, on a clock that did not move
    // since (a coarse one), changed at the same time.
    assert_eq!(
        std::fs::metadata(&path).expect("the file").len(),
        looked.len()
    );
    File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(looked.modified()?))
        .expect("the time set back");
    // The exec failed: the same image goes on, and nothing more is asked.

    assert_eq!(
        whos(&requests.look().expect("a look").requests),
        ["ruling #46"]
    );
    assert!(
        requests.look().expect("a look").requests.is_empty(),
        "read once"
    );
}

#[test]
fn a_running_scheduler_that_cannot_be_asked_is_named_in_the_error() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (_lock, _) = SchedulerLock::take(dir.path(), &me(), "0.3.1", "0.4.0", at(1)).expect("lock");
    // A request file that cannot be appended to.
    std::fs::create_dir(dir.path().join(REQUEST_FILE)).expect("in the way");

    let refused = ask(dir.path(), "ruling #45", at(2), &|_| true);

    let Err(not_asked) = refused else {
        panic!("asked: {refused:?}");
    };
    assert_eq!(not_asked.holding.pid, 4242);
    let said = not_asked.to_string();
    assert!(
        said.starts_with("the running scheduler (pid 4242, since ")
            && said.ends_with("; no tick was asked for"),
        "{said}"
    );
}

#[test]
fn requests_already_read_are_kept_when_the_file_cannot_be_emptied() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("a temporary directory");
    let (mut requests, _) = Requests::start(dir.path()).expect("started");
    let line = request(1, "claimed #7").line();
    let count = usize::try_from(EMPTY_PAST).expect("small") / line.len() + 1;
    for _ in 0..count {
        append(dir.path(), &request(1, "claimed #7")).expect("appended");
    }
    // Readable, but no longer writable: it cannot be emptied.
    let path = dir.path().join(REQUEST_FILE);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("read-only");
    if OpenOptions::new().write(true).open(&path).is_ok() {
        // Run with the power to write any file: nothing here can fail to empty.
        return;
    }

    let looked = requests.look().expect("a look");

    assert_eq!(looked.requests.len(), count, "every line read is kept");
    let why = looked.not_emptied.expect("the failure is said");
    assert!(why.starts_with("cannot empty "), "{why}");
    assert_eq!(
        std::fs::metadata(&path).expect("the file").len(),
        u64::try_from(count * line.len()).expect("small"),
        "not emptied"
    );
    // Nothing new: nothing is read again, and nothing more is said.
    assert_eq!(requests.look().expect("a look"), Looked::default());
    // A later request is read once, the cut tried again and said again.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("writable");
    append(dir.path(), &request(2, "sign #1")).expect("appended");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("read-only");
    let looked = requests.look().expect("a look");
    assert_eq!(whos(&looked.requests), ["sign #1"]);
    assert!(looked.not_emptied.is_some());
}
