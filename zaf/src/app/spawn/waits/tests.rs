use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{ListedPhase, PhaseEnd, PhaseRun, PullFacts, RunExit, StandingFacts};
use crate::app::spawn::outcome::Started;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName};
use crate::ports::Holder;

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn listed(action: &str, kind: ActionKind, ends: PhaseEnd) -> ListedPhase {
    ListedPhase {
        action: name(action),
        kind,
        ends,
        on_fail: None,
    }
}

/// implement (a model), review (a wait), merge (zaf).
fn list() -> Vec<ListedPhase> {
    vec![
        listed("implement", ActionKind::Model, PhaseEnd::Exit),
        listed("review", ActionKind::Wait, PhaseEnd::ReviewPosted),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged),
    ]
}

fn implemented() -> PhaseRun {
    PhaseRun {
        action: name("implement"),
        head: None,
        started_at: time(2),
        alive: false,
        exit: Some(RunExit {
            code: 0,
            at: time(9),
        }),
        last_line: None,
    }
}

/// A task that implemented and opened its pull request: it is ready at review.
fn at_review(task: u64) -> TickTask {
    TickTask {
        task: TicketId::from(task),
        list: list(),
        facts: StandingFacts {
            ready_since: time(1),
            pull: Some(PullFacts {
                opened_at: time(10),
                merged_at: None,
                head: Some("head".to_owned()),
                checks: Vec::new(),
                failed_checks: Vec::new(),
                reviews: Vec::new(),
            }),
            runs: vec![implemented()],
        },
    }
}

/// A task with no pull request yet: it is ready at the model action.
fn at_implement(task: u64) -> TickTask {
    TickTask {
        task: TicketId::from(task),
        list: list(),
        facts: StandingFacts {
            ready_since: time(1),
            pull: None,
            runs: Vec::new(),
        },
    }
}

/// The review wait's run at `head`, started, with `alive` and `exit` as given.
fn review_run(head: &str, alive: bool, exit: Option<i32>) -> PhaseRun {
    PhaseRun {
        action: name("review"),
        head: Some(head.to_owned()),
        started_at: time(11),
        alive,
        exit: exit.map(|code| RunExit { code, at: time(15) }),
        last_line: None,
    }
}

/// A launcher that records each call and gives `result_of(task)`.
fn recording<'a>(
    calls: &'a mut Vec<(u64, String)>,
    result_of: impl Fn(u64) -> Result<(), String> + 'a,
) -> impl FnMut(TicketId, &ActionName) -> Result<(), String> + 'a {
    move |task, action| {
        calls.push((task.get(), action.as_str().to_owned()));
        result_of(task.get())
    }
}

#[test]
fn every_ready_wait_launches() {
    let tasks = [at_review(441), at_review(442)];
    let mut calls = Vec::new();
    let mut logged = Vec::new();
    let waits = launch_waits(
        &tasks,
        &mut recording(&mut calls, |_| Ok(())),
        &mut |line| logged.push(line.to_owned()),
    );
    assert_eq!(
        calls,
        vec![(441, "review".to_owned()), (442, "review".to_owned())],
        "both reviews launch in one tick"
    );
    assert_eq!(
        waits,
        vec![
            Launched {
                task: TicketId::from(441),
                action: name("review"),
            },
            Launched {
                task: TicketId::from(442),
                action: name("review"),
            },
        ]
    );
    assert!(logged.is_empty());
}

#[test]
fn the_tick_returns_without_waiting_for_the_wait() {
    // The launch call is the whole of the step: it is never handed anything to wait on, and the
    // waits it lists are still `Waiting` once their run is alive, not ended.
    let mut task = at_review(441);
    let waits = launch_waits(std::slice::from_ref(&task), &mut |_, _| Ok(()), &mut |_| {});
    assert_eq!(waits.len(), 1);
    task.facts.runs.push(review_run("head", true, None));
    assert_eq!(
        derive_standing(&task.list, &task.facts),
        Standing::Running {
            action: name("review")
        }
    );
    assert_eq!(task.ready_wait_action(), None);
}

#[test]
fn only_a_task_ready_at_a_wait_launches() {
    let mut merged = at_review(443);
    merged.facts.pull.as_mut().expect("a pull").merged_at = Some(time(40));
    let tasks = [at_implement(440), merged, at_review(441)];
    let mut calls = Vec::new();
    launch_waits(&tasks, &mut recording(&mut calls, |_| Ok(())), &mut |_| {});
    assert_eq!(
        calls,
        vec![(441, "review".to_owned())],
        "a model phase and a merged task launch no wait"
    );
}

#[test]
fn a_wait_run_at_head_is_not_relaunched() {
    let mut ran = at_review(441);
    ran.facts.runs.push(review_run("head", false, Some(0)));
    let mut waiting = at_review(442);
    waiting.facts.runs.push(review_run("head", false, None));
    let tasks = [ran, waiting, at_review(443)];
    let mut calls = Vec::new();
    let waits = launch_waits(&tasks, &mut recording(&mut calls, |_| Ok(())), &mut |_| {});
    assert_eq!(
        calls,
        vec![(443, "review".to_owned())],
        "a review run at the current head waits for its fact and is not started again"
    );
    assert_eq!(waits.len(), 1);
}

#[test]
fn a_wait_run_at_an_old_head_launches_again() {
    let mut task = at_review(441);
    task.facts.runs.push(review_run("old", false, Some(0)));
    let mut calls = Vec::new();
    launch_waits(&[task], &mut recording(&mut calls, |_| Ok(())), &mut |_| {});
    assert_eq!(calls, vec![(441, "review".to_owned())], "a push reopens it");
}

#[test]
fn a_failed_wait_launch_does_not_stop_others() {
    let tasks = [at_review(441), at_review(442), at_review(443)];
    let mut calls = Vec::new();
    let mut logged = Vec::new();
    let waits = launch_waits(
        &tasks,
        &mut recording(&mut calls, |task| match task {
            441 => Err("cannot start the launcher".to_owned()),
            _ => Ok(()),
        }),
        &mut |line| logged.push(line.to_owned()),
    );
    assert_eq!(calls.len(), 3, "the failure of the first stops no other");
    assert_eq!(
        waits.iter().map(|wait| wait.task.get()).collect::<Vec<_>>(),
        vec![442, 443],
        "the failed one is not listed as launched"
    );
    assert_eq!(logged.len(), 1, "the failure is logged");
    assert!(logged[0].contains("#441") && logged[0].contains("cannot start the launcher"));
    // The failed task is left as it was: at the next tick it is ready at review again.
    assert_eq!(tasks[0].ready_wait_action(), Some(name("review")));
}

fn planned(number: u32, epic: u64, _tasks: &[u64]) -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(epic),
            number: InstanceNumber::from(number),
        },
    }
}

#[test]
fn waits_cost_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let holder = Holder {
        pid: 100,
        start: "1000".to_owned(),
    };
    runs::reserve(
        dir.path(),
        &planned(1, 421, &[440]),
        &holder,
        "2026-11-09T08:00:00Z",
    )
    .expect("reserves");
    let alive = |_: &Holder| true;
    let (before, _) = crate::app::spawn::derive(dir.path(), &alive).expect("derives");
    let folders_before = runs::read_all(dir.path()).expect("reads").len();

    let tasks = [at_review(441), at_review(442)];
    let mut calls = Vec::new();
    let outcome = launch_outcome(&tasks, &mut recording(&mut calls, |_| Ok(())), &mut |_| {});

    let (after, _) = crate::app::spawn::derive(dir.path(), &alive).expect("derives");
    assert_eq!(calls.len(), 2, "the waits launched");
    assert_eq!(before.running(), 1);
    assert_eq!(after.running(), before.running(), "no slot taken or freed");
    assert_eq!(after.launches, before.launches);
    assert_eq!(
        runs::read_all(dir.path()).expect("reads").len(),
        folders_before,
        "no instance run folder opened"
    );
    assert_eq!(outcome.units(), 0, "no units charged");
    assert!(outcome.started.is_none());
}

#[test]
fn a_wait_is_not_the_model_start() {
    // Two waits launch and a model phase is ready: the waits take no part of the one model start,
    // so the model phase is still the tick's candidate and the units are its alone.
    let tasks = [at_review(441), at_implement(440), at_review(442)];
    let mut calls = Vec::new();
    let mut outcome = launch_outcome(&tasks, &mut recording(&mut calls, |_| Ok(())), &mut |_| {});
    assert_eq!(outcome.waits.len(), 2);
    assert!(outcome.started.is_none(), "a wait is not a model start");
    assert_eq!(
        tasks[1].ready_wait_action(),
        None,
        "the model phase is no wait: nothing launched it"
    );

    outcome.started = Some(Started {
        task: TicketId::from(440),
        action: name("implement"),
        units: 2,
    });
    assert_eq!(outcome.waits.len(), 2, "the start leaves the waits listed");
    assert_eq!(outcome.units(), 2, "the units are the model start's alone");
}

#[test]
fn the_outcome_lists_the_waits() {
    let tasks = [at_review(441), at_implement(440), at_review(442)];
    let outcome = launch_outcome(&tasks, &mut |_, _| Ok(()), &mut |_| {});
    assert_eq!(
        outcome.waits,
        vec![
            Launched {
                task: TicketId::from(441),
                action: name("review"),
            },
            Launched {
                task: TicketId::from(442),
                action: name("review"),
            },
        ]
    );
    assert!(!outcome.is_idle());
    let json = outcome.to_json();
    assert_eq!(json["idle"], false);
    assert_eq!(json["waits"][1]["task"], 442);
    assert_eq!(json["waits"][1]["action"], "review");
    assert_eq!(
        outcome.render_text(),
        "wait review #441\nwait review #442\n"
    );
}

#[test]
fn a_tick_with_no_wait_ready_is_idle() {
    let mut calls = Vec::new();
    let outcome = launch_outcome(
        &[at_implement(440)],
        &mut recording(&mut calls, |_| Ok(())),
        &mut |_| {},
    );
    assert!(calls.is_empty());
    assert!(outcome.is_idle());
    assert_eq!(outcome, TickOutcome::default());
}
