use std::time::Duration;

use super::*;

/// Runs `watch` for `ticks` ticks, each giving the next of `codes` (the last one repeats), and
/// gives back the ticks run, the waits asked for and the log lines.
fn run_for(interval: Duration, ticks: usize, codes: &[u8]) -> (u64, Vec<Duration>, Vec<String>) {
    let mut done = 0;
    let mut waits = Vec::new();
    let mut lines = Vec::new();
    let ran = watch(
        interval,
        &mut || {
            let code = codes[done.min(codes.len() - 1)];
            done += 1;
            code
        },
        &mut |asked| {
            waits.push(asked);
            waits.len() < ticks
        },
        &mut |line| lines.push(line.to_owned()),
    );
    (ran, waits, lines)
}

#[test]
fn watch_ticks_each_interval() {
    let interval = Duration::from_secs(120);

    let (ran, waits, lines) = run_for(interval, 3, &[0]);

    assert_eq!(ran, 3, "a tick for each interval that passed");
    assert_eq!(waits, vec![interval; 3], "one interval after every tick");
    assert!(lines.is_empty(), "a clean tick logs nothing: {lines:?}");
}

#[test]
fn the_first_tick_runs_before_any_wait() {
    let mut order = Vec::new();
    let order_cell = std::cell::RefCell::new(&mut order);
    watch(
        Duration::from_secs(60),
        &mut || {
            order_cell.borrow_mut().push("tick");
            0
        },
        &mut |_| {
            order_cell.borrow_mut().push("wait");
            false
        },
        &mut |_| {},
    );
    assert_eq!(order, ["tick", "wait"]);
}

#[test]
fn a_failed_tick_does_not_end_the_watch() {
    let (ran, waits, lines) = run_for(Duration::from_secs(60), 4, &[2, 1, 0, 2]);

    assert_eq!(ran, 4, "the loop went on past every failure");
    assert_eq!(waits.len(), 4);
    assert_eq!(
        lines,
        [
            "tick 1 failed with exit 2; the watch goes on",
            "tick 2 failed with exit 1; the watch goes on",
            "tick 4 failed with exit 2; the watch goes on",
        ]
    );
}
