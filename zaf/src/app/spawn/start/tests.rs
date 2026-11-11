use std::convert::Infallible;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use super::*;
use crate::app::actions::{PhaseEnd, StandingFacts};

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

/// implement (a model), ci (a wait whose on-fail is fix), fix (a model), merge (zaf).
fn list() -> Vec<ListedPhase> {
    vec![
        ListedPhase {
            action: name("implement"),
            kind: ActionKind::Model,
            ends: PhaseEnd::PrOpen,
            on_fail: None,
        },
        ListedPhase {
            action: name("ci"),
            kind: ActionKind::Wait,
            ends: PhaseEnd::CiDone,
            on_fail: Some(name("fix")),
        },
        ListedPhase {
            action: name("merge"),
            kind: ActionKind::Zaf,
            ends: PhaseEnd::Merged,
            on_fail: None,
        },
    ]
}

fn standing(task: u64, size: TaskSize, standing: Standing) -> TaskStanding {
    TaskStanding {
        task: TicketId::from(task),
        size,
        list: list(),
        standing,
    }
}

/// A task ready at implement since `since`.
fn ready(task: u64, size: TaskSize, since: u64) -> TaskStanding {
    standing(
        task,
        size,
        Standing::Ready {
            action: name("implement"),
            since: time(since),
        },
    )
}

/// A task whose ci failed at `since`, with `on_fail` as given.
fn failed(task: u64, size: TaskSize, since: u64, on_fail: Option<&str>) -> TaskStanding {
    standing(
        task,
        size,
        Standing::Failed {
            action: name("ci"),
            on_fail: on_fail.map(name),
            since: time(since),
        },
    )
}

/// A budget of `units`, the default reserve and sizes.
fn budget(units: u32) -> Budget {
    Budget {
        units: Some(units),
        ..Budget::default()
    }
}

/// `units` in use, no E live, nothing running of four slots.
fn in_use(units: u64) -> InUse {
    InUse {
        units: Units::whole(units),
        e_live: false,
        running_here: 0,
        slots: 4,
    }
}

fn planned(task: u64, action: &str, units: u32) -> PlannedPhase {
    PlannedPhase {
        task: TicketId::from(task),
        action: name(action),
        units,
    }
}

/// A start that records each phase it is handed and launches nothing.
fn recording(
    calls: &mut Vec<PlannedPhase>,
) -> impl FnMut(&PlannedPhase) -> Result<(), Infallible> + '_ {
    move |planned| {
        calls.push(planned.clone());
        Ok(())
    }
}

#[test]
fn the_longest_ready_starts() {
    let standings = [ready(441, TaskSize::S, 20), ready(440, TaskSize::S, 10)];
    assert_eq!(
        plan(&standings, &in_use(0), &budget(32)),
        Some(planned(440, "implement", 2)),
        "#440 has been ready longer, whatever the order it is read in"
    );
}

#[test]
fn a_phase_too_big_is_passed() {
    // 3 of 6 units in use: the M (4) waiting longest does not fit, the S (2) behind it does.
    let standings = [ready(440, TaskSize::M, 10), ready(441, TaskSize::S, 20)];
    assert_eq!(
        plan(&standings, &in_use(3), &budget(6)),
        Some(planned(441, "implement", 2)),
        "the M is passed for the S that fits"
    );
    // Passed, it keeps its weight: once the budget frees, it is first again.
    assert_eq!(
        plan(&standings, &in_use(0), &budget(6)),
        Some(planned(440, "implement", 4))
    );
    // Passed for a full project, it keeps its weight as well.
    let full = InUse {
        running_here: 4,
        ..in_use(0)
    };
    assert_eq!(plan(&standings, &full, &budget(6)), None);
}

#[test]
fn a_failed_wait_starts_its_on_fail() {
    let standings = [
        failed(440, TaskSize::XS, 5, None),
        failed(441, TaskSize::M, 10, Some("fix")),
        ready(442, TaskSize::XS, 20),
    ];
    assert_eq!(
        plan(&standings, &in_use(0), &budget(32)),
        Some(planned(441, "fix", 4)),
        "fix starts at the task's size, M; a failed wait with no on-fail is not an item"
    );
}

#[test]
fn nothing_fits_nothing_starts() {
    let standings = [
        ready(440, TaskSize::M, 10),
        failed(441, TaskSize::M, 5, Some("fix")),
    ];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &in_use(30),
        &budget(32),
        &mut recording(&mut calls),
    )
    .expect("the tick passes");
    assert!(calls.is_empty(), "nothing is started");
    assert!(outcome.started.is_none());
    assert!(outcome.is_idle(), "the tick ends idle, not in error");

    // Neither does a task waiting, running, done or ready at a wait or a zaf action.
    let others = [
        standing(442, TaskSize::XS, Standing::Waiting { action: name("ci") }),
        standing(
            443,
            TaskSize::XS,
            Standing::Running {
                action: name("implement"),
            },
        ),
        standing(444, TaskSize::XS, Standing::Done),
        standing(
            445,
            TaskSize::XS,
            Standing::Ready {
                action: name("merge"),
                since: time(1),
            },
        ),
        standing(
            446,
            TaskSize::XS,
            Standing::Ready {
                action: name("ci"),
                since: time(1),
            },
        ),
    ];
    assert_eq!(plan(&others, &in_use(0), &budget(32)), None);
}

#[test]
fn one_model_per_tick() {
    let standings = [
        ready(440, TaskSize::XS, 10),
        ready(441, TaskSize::XS, 20),
        failed(442, TaskSize::XS, 30, Some("fix")),
    ];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &in_use(0),
        &budget(32),
        &mut recording(&mut calls),
    )
    .expect("the tick passes");
    assert_eq!(
        calls,
        vec![planned(440, "implement", 1)],
        "three fit, one starts"
    );
    assert_eq!(outcome.units(), 1);
}

/// Kills the child when dropped, so a failed assertion leaves no process behind.
struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_tick_does_not_wait() {
    let standings = [ready(440, TaskSize::XS, 10)];
    let mut phase = None;
    let begun = Instant::now();
    let outcome = start_outcome(&standings, &in_use(0), &budget(32), &mut |_| {
        phase = Some(Reap(Command::new("sleep").arg("30").spawn()?));
        Ok::<(), std::io::Error>(())
    })
    .expect("the phase launches");
    let elapsed = begun.elapsed();
    let mut phase = phase.expect("the phase was started");
    assert!(outcome.started.is_some());
    assert!(
        elapsed < Duration::from_secs(10),
        "the tick returned in {elapsed:?}, not after the phase"
    );
    assert!(
        phase.0.try_wait().expect("reads the child").is_none(),
        "the phase still runs after the tick returned"
    );
}

#[test]
fn the_outcome_names_the_start() {
    let standings = [failed(441, TaskSize::S, 10, Some("fix"))];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &in_use(0),
        &budget(32),
        &mut recording(&mut calls),
    )
    .expect("the tick passes");
    assert_eq!(
        outcome.started,
        Some(Started {
            task: TicketId::from(441),
            action: name("fix"),
            units: 2,
        })
    );
    assert_eq!(outcome.render_text(), "started fix #441: 2 units\n");
    let json = outcome.to_json();
    assert_eq!(json["started"]["task"], 441);
    assert_eq!(json["started"]["action"], "fix");
    assert_eq!(json["started"]["units"], 2);
}

#[test]
fn a_failed_launch_records_no_start() {
    let standings = [ready(440, TaskSize::XS, 10)];
    let result = start_outcome(&standings, &in_use(0), &budget(32), &mut |_| {
        Err("no launch")
    });
    assert_eq!(result, Err("no launch"));
}

#[test]
fn the_standing_is_derived_from_the_task() {
    let task = TickTask {
        task: TicketId::from(440),
        list: list(),
        facts: StandingFacts {
            ready_since: time(7),
            pull: None,
            runs: Vec::new(),
        },
    };
    let standing = TaskStanding::of(&task, TaskSize::M);
    assert_eq!(
        plan(&[standing], &in_use(0), &budget(32)),
        Some(planned(440, "implement", 4))
    );
}

#[test]
fn with_no_budget_only_the_slots_decide() {
    let standings = [ready(440, TaskSize::M, 10)];
    let none = Budget::default();
    assert_eq!(
        plan(&standings, &in_use(1000), &none),
        Some(planned(440, "implement", 4))
    );
}
