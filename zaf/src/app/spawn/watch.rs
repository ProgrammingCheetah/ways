//! `zaf spawn --watch`: the foreground loop that runs one tick each `[spawn] tick_minutes`
//! (docs/zaf-actions.md §5.1, #421 ruling 25, the ruling on #428).
//!
//! [`watch`] runs the tick it is handed, then waits one interval, and so on until the wait says
//! to stop. A tick that fails is logged and the loop goes on: the next tick reads the same forge
//! again, so one bad read costs one interval. The tick handed in is the one of `zaf spawn --once
//! --no-wait`, so a tick that finds `spawn.lock` held by another does nothing and exits 0, and
//! the loop never waits on a lock. zaf writes no system timer: the zaf service (#353) replaces
//! this loop later.
//!
//! The module is pure: the caller hands in the `tick`, the `wait` that lets one interval pass
//! (it gives `false` to end the loop, which nothing but a test does) and a `log`.

use std::time::Duration;

/// Runs `tick` at once and then after each `interval` that `wait` lets pass, and gives back how
/// many ticks ran. `tick` gives the tick's exit code: a non-zero one goes to `log` and the loop
/// goes on. `wait(interval)` gives `false` to end the loop, so a real watch, whose `wait` always
/// gives `true`, runs until the process is stopped.
pub fn watch(
    interval: Duration,
    tick: &mut dyn FnMut() -> u8,
    wait: &mut dyn FnMut(Duration) -> bool,
    log: &mut dyn FnMut(&str),
) -> u64 {
    let mut ticks = 0;
    loop {
        ticks += 1;
        let code = tick();
        if code != 0 {
            log(&format!(
                "tick {ticks} failed with exit {code}; the watch goes on"
            ));
        }
        if !wait(interval) {
            return ticks;
        }
    }
}

#[cfg(test)]
mod tests;
