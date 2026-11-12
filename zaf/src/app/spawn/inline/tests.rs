use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{PhaseEnd, PhaseRun, PullFacts, ReviewFact, RunExit};
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

/// A task that implemented, got its review at the head, and waits at merge.
fn at_merge(task: u64) -> TickTask {
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
                reviews: vec![ReviewFact {
                    head: Some("head".to_owned()),
                    posted_at: time(30),
                }],
            }),
            runs: vec![PhaseRun {
                action: name("implement"),
                head: None,
                started_at: time(2),
                alive: false,
                exit: Some(RunExit {
                    code: 0,
                    at: time(9),
                }),
                last_line: None,
            }],
        },
    }
}

/// A task with no pull request yet: it is ready at the model action, not at merge.
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

/// A runner that records each call and gives `exit_of(task)`.
fn recording<'a>(
    calls: &'a mut Vec<(u64, String)>,
    exit_of: impl Fn(u64) -> u8 + 'a,
) -> impl FnMut(TicketId, &ActionName) -> u8 + 'a {
    move |task, action| {
        calls.push((task.get(), action.as_str().to_owned()));
        exit_of(task.get())
    }
}

#[test]
fn every_ready_merge_runs() {
    let tasks = [at_merge(441), at_merge(442)];
    let mut calls = Vec::new();
    let ran = run_zaf_actions(&tasks, &mut recording(&mut calls, |_| 0));
    assert_eq!(
        calls,
        vec![(441, "merge".to_owned()), (442, "merge".to_owned())],
        "both merges run in one tick, one after the other"
    );
    assert_eq!(ran.len(), 2);
    assert!(ran.iter().all(Ran::passed));
}

#[test]
fn only_a_task_ready_at_a_zaf_action_runs() {
    let mut merged = at_merge(443);
    merged.facts.pull.as_mut().expect("a pull").merged_at = Some(time(40));
    let tasks = [at_implement(440), merged, at_merge(441)];
    let mut calls = Vec::new();
    run_zaf_actions(&tasks, &mut recording(&mut calls, |_| 0));
    assert_eq!(
        calls,
        vec![(441, "merge".to_owned())],
        "a model phase and a merged task run nothing"
    );
}

#[test]
fn a_failed_merge_does_not_stop_others() {
    let tasks = [at_merge(441), at_merge(442), at_merge(443)];
    let mut calls = Vec::new();
    let ran = run_zaf_actions(
        &tasks,
        &mut recording(&mut calls, |task| match task {
            441 => 1,
            _ => 0,
        }),
    );
    assert_eq!(calls.len(), 3, "the failure of the first stops no other");
    assert_eq!(
        ran.iter().map(|ran| ran.exit).collect::<Vec<_>>(),
        vec![1, 0, 0]
    );
    // The failed task is left as it was: at the next tick it is ready at merge again.
    assert_eq!(tasks[0].ready_zaf_action(), Some(name("merge")));
}

#[test]
fn a_merge_that_never_ran_is_listed_with_its_125() {
    let tasks = [at_merge(441), at_merge(442)];
    let mut calls = Vec::new();
    let ran = run_zaf_actions(
        &tasks,
        &mut recording(&mut calls, |task| if task == 441 { 125 } else { 0 }),
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(ran[0].exit, 125);
    assert!(!ran[0].passed());
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
fn zaf_actions_cost_nothing() {
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

    let tasks = [at_merge(441), at_merge(442)];
    let mut calls = Vec::new();
    let outcome = run_inline(&tasks, &mut recording(&mut calls, |_| 0));

    let (after, _) = crate::app::spawn::derive(dir.path(), &alive).expect("derives");
    assert_eq!(calls.len(), 2, "the merges ran");
    assert_eq!(before.running(), 1);
    assert_eq!(after.running(), before.running(), "no slot taken or freed");
    assert_eq!(after.launches, before.launches);
    assert_eq!(
        runs::read_all(dir.path()).expect("reads").len(),
        folders_before,
        "no run folder opened"
    );
    assert_eq!(outcome.units(), 0, "no units charged");
    assert!(outcome.started.is_none());
}

#[test]
fn the_outcome_lists_what_ran() {
    let tasks = [at_merge(441), at_implement(440), at_merge(442)];
    let mut calls = Vec::new();
    let outcome = run_inline(
        &tasks,
        &mut recording(&mut calls, |task| if task == 442 { 1 } else { 0 }),
    );
    assert_eq!(
        outcome.ran,
        vec![
            Ran {
                task: TicketId::from(441),
                action: name("merge"),
                exit: 0,
            },
            Ran {
                task: TicketId::from(442),
                action: name("merge"),
                exit: 1,
            },
        ]
    );
    assert!(!outcome.is_idle());
    let json = outcome.to_json();
    assert_eq!(json["idle"], false);
    assert_eq!(json["ran"][1]["task"], 442);
    assert_eq!(json["ran"][1]["exit"], 1);
    assert_eq!(
        outcome.render_text(),
        "ran merge #441: exit 0\nran merge #442: exit 1\n"
    );
}

#[test]
fn a_tick_with_nothing_ready_is_idle() {
    let mut calls = Vec::new();
    let outcome = run_inline(&[at_implement(440)], &mut recording(&mut calls, |_| 0));
    assert!(calls.is_empty());
    assert!(outcome.is_idle());
    assert_eq!(outcome, TickOutcome::default());
    assert_eq!(outcome.render_text(), "idle\n");
    assert_eq!(outcome.to_json()["idle"], true);
    assert!(run_inline(&[], &mut |_, _| 0).is_idle());
}
