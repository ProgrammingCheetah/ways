use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;
use crate::app::spawn::record::watch_running;

/// A tick that counts itself and says who asked; with a gate, it waits for the test to let it end.
#[derive(Default)]
struct Ticks {
    ran: AtomicUsize,
    asked: Mutex<Vec<String>>,
    said: Mutex<Vec<String>>,
    gate: Mutex<Option<Receiver<()>>>,
}

impl Ticks {
    fn gated() -> (Arc<Self>, Sender<()>) {
        let (open, gate) = channel();
        let ticks = Arc::new(Self {
            gate: Mutex::new(Some(gate)),
            ..Self::default()
        });
        (ticks, open)
    }

    fn ran(&self) -> usize {
        self.ran.load(Ordering::SeqCst)
    }
}

impl Tick for Ticks {
    fn tick(&self, asked: &Asked) -> u8 {
        self.ran.fetch_add(1, Ordering::SeqCst);
        self.asked.lock().unwrap().push(asked.line());
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv_timeout(Duration::from_secs(20));
        }
        0
    }

    fn say(&self, line: &str) {
        self.said.lock().unwrap().push(line.to_owned());
    }
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let limit = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < limit, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

const DAY: Duration = Duration::from_secs(86_400);

#[test]
fn a_start_runs_a_tick_at_once_and_a_stop_ends_the_watch() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let handle = watches.handle();
    let ticks = Arc::new(Ticks::default());
    assert!(!watch_running(dir.path()));

    let started = handle
        .start(dir.path(), "0.4.0", DAY, ticks.clone())
        .expect("started");

    assert_eq!(started.version, "0.4.0");
    assert!(watch_running(dir.path()), "the lock is held while it runs");
    until("the first tick", || ticks.ran() == 1);
    assert_eq!(
        *ticks.asked.lock().unwrap(),
        ["asked by: hub"],
        "asked by the hub's start, not by the timer"
    );

    assert_eq!(handle.stop(dir.path()), Ok(Stopped::Ended));
    assert!(!watch_running(dir.path()));
    assert!(!handle.runs(dir.path()));
    assert_eq!(ticks.ran(), 1, "a stop starts no tick");
}

#[test]
fn the_holder_is_the_hub_and_its_line_names_the_version() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let ticks = Arc::new(Ticks::default());
    watches
        .handle()
        .start(dir.path(), "0.4.0", DAY, ticks)
        .expect("started");

    let holding = lock::running(dir.path(), &process::alive).expect("held");

    assert_eq!(holding.pid, std::process::id());
    assert_eq!(holding.version, "0.4.0");
}

#[test]
fn a_second_start_is_refused_with_the_reason() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let handle = watches.handle();
    let ticks = Arc::new(Ticks::default());
    handle
        .start(dir.path(), "0.4.0", DAY, ticks.clone())
        .expect("started");

    let again = handle.start(dir.path(), "0.5.0", DAY, ticks.clone());

    let Err(WatchError::Refused(why)) = again else {
        panic!("a second start is a refusal, got {again:?}");
    };
    assert!(why.contains("already runs"), "{why}");
    assert!(why.contains("0.4.0"), "it names the one that runs: {why}");
    until("the first tick", || ticks.ran() == 1);
    assert_eq!(ticks.ran(), 1, "the refused start ticked nothing");
    assert!(watch_running(dir.path()), "the first still runs");
}

#[test]
fn a_scheduler_another_process_runs_refuses_a_start_and_is_never_stopped() {
    let dir = tempfile::tempdir().expect("state");
    // A `zaf spawn --watch` in a terminal: its lock, held by a holder that lives.
    let me = process::this().expect("this process");
    let (terminal, _) =
        SchedulerLock::take(dir.path(), &me, "0.3.1", "0.4.0", SystemTime::now()).expect("taken");
    let watches = Watches::new();
    let handle = watches.handle();
    let ticks = Arc::new(Ticks::default());

    let started = handle.start(dir.path(), "0.4.0", DAY, ticks.clone());
    let stopped = handle.stop(dir.path());

    let Err(WatchError::Refused(why)) = started else {
        panic!("a start beside a scheduler is a refusal, got {started:?}");
    };
    assert!(why.contains("a scheduler already runs here"), "{why}");
    let Err(WatchError::Refused(why)) = stopped else {
        panic!("a stop of another's scheduler is a refusal, got {stopped:?}");
    };
    assert!(why.contains("not started by this hub"), "{why}");
    assert_eq!(ticks.ran(), 0);
    assert!(watch_running(dir.path()), "the other scheduler runs on");
    terminal.release();
    assert!(!watch_running(dir.path()));
}

#[test]
fn a_stop_with_no_watch_is_refused() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();

    let stopped = watches.handle().stop(dir.path());

    assert_eq!(
        stopped,
        Err(WatchError::Refused("no watch runs here".to_owned()))
    );
}

#[test]
fn a_stop_waits_for_the_current_tick_and_the_watch_ends_after_it() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let handle = watches.handle();
    let (ticks, open) = Ticks::gated();
    handle
        .start(dir.path(), "0.4.0", DAY, ticks.clone())
        .expect("started");
    until("the tick to begin", || ticks.ran() == 1);

    // The tick is in progress: the stop is asked and the watch still holds its lock.
    let stopper = {
        let handle = handle.clone();
        let state = dir.path().to_owned();
        thread::spawn(move || handle.stop(&state))
    };
    thread::sleep(Duration::from_millis(200));
    assert!(watch_running(dir.path()), "it ends after its current tick");
    assert!(!stopper.is_finished(), "the stop waits for the tick");

    open.send(()).expect("the tick ends");

    assert_eq!(stopper.join().expect("joined"), Ok(Stopped::Ended));
    assert!(!watch_running(dir.path()));
    assert_eq!(ticks.ran(), 1, "no tick began after the stop was asked");
}

#[test]
fn dropping_the_owner_ends_every_watch() {
    let (one, two) = (
        tempfile::tempdir().expect("one"),
        tempfile::tempdir().expect("two"),
    );
    let watches = Watches::new();
    let handle = watches.handle();
    for dir in [&one, &two] {
        handle
            .start(dir.path(), "0.4.0", DAY, Arc::new(Ticks::default()))
            .expect("started");
    }
    assert!(watch_running(one.path()) && watch_running(two.path()));

    drop(watches);

    assert!(!watch_running(one.path()));
    assert!(!watch_running(two.path()));
}

#[test]
fn no_watch_starts_once_the_hub_has_ended() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let handle = watches.handle();
    drop(watches);

    let started = handle.start(dir.path(), "0.4.0", DAY, Arc::new(Ticks::default()));

    let Err(WatchError::Refused(why)) = started else {
        panic!("a start after the end is a refusal, got {started:?}");
    };
    assert!(why.contains("ending"), "{why}");
    assert!(!watch_running(dir.path()));
}

#[test]
fn a_request_left_in_the_file_is_served_by_a_tick() {
    let dir = tempfile::tempdir().expect("state");
    let watches = Watches::new();
    let handle = watches.handle();
    let ticks = Arc::new(Ticks::default());
    // A phase that ended asks the scheduler for a tick while it runs.
    handle
        .start(dir.path(), "0.4.0", DAY, ticks.clone())
        .expect("started");
    until("the first tick", || ticks.ran() == 1);

    let asked = request::ask(
        dir.path(),
        "phase-end implement #7",
        SystemTime::now(),
        &process::alive,
    )
    .expect("asked");
    assert!(asked.is_some(), "a scheduler runs, so the ask is taken");

    until("the asked tick", || ticks.ran() == 2);
    assert_eq!(
        ticks.asked.lock().unwrap()[1],
        "asked by: phase-end implement #7"
    );
}

#[test]
fn a_tick_is_scheduled_by_the_scheduler_whose_pid_it_names() {
    assert!(is_scheduled_by(Some("4242"), 4242));
    assert!(is_scheduled_by(Some(" 4242\n"), 4242));
    assert!(!is_scheduled_by(Some("4243"), 4242));
    assert!(!is_scheduled_by(Some("not a pid"), 4242));
    assert!(!is_scheduled_by(None, 4242));
}
