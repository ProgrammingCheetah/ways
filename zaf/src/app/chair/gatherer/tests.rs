use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{Cadence, GITHUB_FLOOR, GatherEvent, Gatherer, floor_of};
use crate::manifest::BoardKind;

/// A cadence that never comes round by itself during a test.
const SLOW: Cadence = Cadence {
    every: Duration::from_secs(3_600),
    poll: Duration::from_millis(5),
    min_gap: Duration::ZERO,
};

const LONG: Duration = Duration::from_secs(10);

/// A gatherer over a counter: each read returns how many reads came before it.
fn counting(
    cadence: Cadence,
    changed: impl FnMut() -> bool + Send + 'static,
) -> (Gatherer, mpsc::Receiver<GatherEvent<usize>>) {
    let reads = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let gatherer = Gatherer::spawn(
        "test-gatherer",
        cadence,
        Duration::ZERO,
        move |_| reads.fetch_add(1, Ordering::SeqCst),
        changed,
        move |event| sender.send(event).is_ok(),
    )
    .expect("starts");
    (gatherer, receiver)
}

/// The state of the next read that ends.
fn next_state(receiver: &mpsc::Receiver<GatherEvent<usize>>) -> usize {
    loop {
        match receiver.recv_timeout(LONG).expect("a read ends") {
            GatherEvent::Started(_) => {}
            GatherEvent::Gathered(state) => return *state,
        }
    }
}

#[test]
fn reads_at_once_and_again_when_asked() {
    let (gatherer, receiver) = counting(SLOW, || false);

    assert!(matches!(
        receiver.recv_timeout(LONG),
        Ok(GatherEvent::Started(_))
    ));
    assert_eq!(next_state(&receiver), 0);
    gatherer.now();
    assert_eq!(next_state(&receiver), 1);
    assert!(
        receiver.recv_timeout(Duration::from_millis(50)).is_err(),
        "nothing more without being asked"
    );
}

#[test]
fn reads_again_on_its_cadence() {
    let fast = Cadence {
        every: Duration::from_millis(10),
        ..SLOW
    };
    let (_gatherer, receiver) = counting(fast, || false);

    assert_eq!(next_state(&receiver), 0);
    assert_eq!(next_state(&receiver), 1);
    assert_eq!(next_state(&receiver), 2);
}

#[test]
fn a_new_cadence_takes_over() {
    let (gatherer, receiver) = counting(SLOW, || false);
    assert_eq!(next_state(&receiver), 0);

    gatherer.set_cadence(Cadence {
        every: Duration::from_millis(10),
        ..SLOW
    });

    assert_eq!(next_state(&receiver), 1);
}

#[test]
fn a_watched_change_starts_a_read() {
    let flag = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&flag);
    let (_gatherer, receiver) = counting(SLOW, move || seen.swap(false, Ordering::SeqCst));
    assert_eq!(next_state(&receiver), 0);

    flag.store(true, Ordering::SeqCst);

    assert_eq!(next_state(&receiver), 1);
}

#[test]
fn a_watched_change_waits_out_the_least_gap() {
    let gapped = Cadence {
        min_gap: Duration::from_millis(300),
        ..SLOW
    };
    let (_gatherer, receiver) = counting(gapped, || true);
    assert_eq!(next_state(&receiver), 0);
    let ended = std::time::Instant::now();

    assert_eq!(next_state(&receiver), 1);

    assert!(
        ended.elapsed() >= Duration::from_millis(250),
        "{:?}",
        ended.elapsed()
    );
}

#[test]
fn the_thread_ends_with_its_handle() {
    let (gatherer, receiver) = counting(SLOW, || false);
    assert_eq!(next_state(&receiver), 0);

    drop(gatherer);

    // The thread drops its sink as it ends, which closes the channel.
    assert!(matches!(
        receiver.recv_timeout(LONG),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn the_thread_ends_when_nobody_listens() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let _gatherer = Gatherer::spawn(
        "test-gatherer",
        Cadence {
            every: Duration::from_millis(1),
            ..SLOW
        },
        Duration::ZERO,
        move |_| counted.fetch_add(1, Ordering::SeqCst),
        || false,
        |_: GatherEvent<usize>| false,
    )
    .expect("starts");

    std::thread::sleep(Duration::from_millis(50));

    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "a sink that is gone stops it before any read"
    );
}

#[test]
fn a_job_runs_on_the_thread_between_reads_and_moves_no_read() {
    let (gatherer, receiver) = counting(SLOW, || false);
    assert_eq!(next_state(&receiver), 0);
    let (told, heard) = mpsc::channel();
    let thread = std::thread::current().id();

    gatherer.run(Box::new(move || {
        let _ = told.send(std::thread::current().id() != thread);
    }));

    assert_eq!(
        heard.recv_timeout(LONG),
        Ok(true),
        "it ran, on the gatherer's thread"
    );
    assert!(
        receiver.recv_timeout(Duration::from_millis(50)).is_err(),
        "a job starts no read"
    );
    gatherer.now();
    assert_eq!(next_state(&receiver), 1);
}

/// A gatherer with a `floor`, telling the instant each read started.
fn floored(
    cadence: Cadence,
    floor: Duration,
    changed: impl FnMut() -> bool + Send + 'static,
) -> (Gatherer, mpsc::Receiver<Instant>) {
    let (sender, receiver) = mpsc::channel();
    let gatherer = Gatherer::spawn(
        "test-gatherer",
        cadence,
        floor,
        |_| (),
        changed,
        move |event| match event {
            GatherEvent::Started(_) => sender.send(Instant::now()).is_ok(),
            GatherEvent::Gathered(_) => true,
        },
    )
    .expect("starts");
    (gatherer, receiver)
}

#[test]
fn chair_github_tab_floor_20s() {
    assert_eq!(GITHUB_FLOOR, Duration::from_secs(20));
    assert_eq!(floor_of(BoardKind::GithubIssues), Duration::from_secs(20));
    assert_eq!(floor_of(BoardKind::GithubProjects), Duration::from_secs(20));

    // A focused tab wants a read each `every` (5 s in the chair); the floor, scaled down here,
    // keeps each read's start that far from the last one's.
    let floor = Duration::from_millis(300);
    let focused = Cadence {
        every: Duration::from_millis(10),
        ..SLOW
    };
    let (_gatherer, starts) = floored(focused, floor, || false);
    let first = starts.recv_timeout(LONG).expect("first read");
    let second = starts.recv_timeout(LONG).expect("second read");
    let third = starts.recv_timeout(LONG).expect("third read");

    assert!(second - first >= floor, "{:?}", second - first);
    assert!(third - second >= floor, "{:?}", third - second);
}

#[test]
fn chair_github_report_waits_for_floor() {
    let floor = Duration::from_millis(400);
    let flag = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&flag);
    let (_gatherer, starts) = floored(SLOW, floor, move || seen.swap(false, Ordering::SeqCst));
    let first = starts.recv_timeout(LONG).expect("first read");

    // A report a quarter of the floor in, and another after it.
    std::thread::sleep(floor / 4);
    flag.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    flag.store(true, Ordering::SeqCst);
    let second = starts.recv_timeout(LONG).expect("one read after the floor");

    assert!(second - first >= floor, "{:?}", second - first);
    assert!(
        second - first < floor + Duration::from_millis(300),
        "it came at the floor, not after another cadence: {:?}",
        second - first
    );
    assert!(
        starts
            .recv_timeout(floor + Duration::from_millis(100))
            .is_err(),
        "the reports inside the floor made one read"
    );
}

#[test]
fn chair_forgejo_tab_keeps_5s() {
    assert_eq!(floor_of(BoardKind::ForgejoIssues), Duration::ZERO);
    assert_eq!(Cadence::SHOWN.every, Duration::from_secs(5));
    assert_eq!(Cadence::BEHIND.every, Duration::from_secs(60));

    // With no floor a read a write asks for starts at once.
    let (gatherer, starts) = floored(SLOW, floor_of(BoardKind::ForgejoIssues), || false);
    starts.recv_timeout(LONG).expect("first read");
    let asked = Instant::now();
    gatherer.now();
    let second = starts.recv_timeout(LONG).expect("second read");
    assert!(second - asked < Duration::from_millis(200));
}
