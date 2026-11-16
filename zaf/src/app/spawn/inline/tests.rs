use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
use crate::app::schedule::tick::support;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::schedule::{
    ExitCode, HeadEnd, PullFacts, ReviewFact, RunExit, RunRecord, TicketFacts,
};
use crate::domain::{CommitId, Instance, InstanceNumber, RoleName, TaskSize};
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
fn at_merge(task: u64) -> (TicketFacts, Vec<ListedPhase>) {
    let pull = PullFacts {
        opened_at: time(10),
        merged_at: None,
        head: Some(CommitId::from("head")),
        changes_a_file: true,
        checks: Vec::new(),
        reviews: vec![ReviewFact {
            head: Some(CommitId::from("head")),
            posted_at: time(30),
        }],
    };
    // D1: the list has no `pr-open` phase, so implement's `exit` is keyed on the head: the run
    // left the head the review was made on.
    let implemented = RunRecord {
        action: name("implement"),
        head: None,
        head_end: HeadEnd::Left(Some(CommitId::from("head"))),
        started_at: time(2),
        alive: false,
        exit: Some(RunExit {
            code: ExitCode::from(0),
            at: time(9),
        }),
        questions: BTreeSet::new(),
        last_line: None,
    };
    let facts = support::facts(
        task,
        &list(),
        TaskSize::S,
        time(1),
        Some(pull),
        vec![implemented],
    );
    (facts, list())
}

/// A task with no pull request yet: it is in the lane at the model action, not at merge.
fn at_implement(task: u64) -> (TicketFacts, Vec<ListedPhase>) {
    let facts = support::facts(task, &list(), TaskSize::S, time(1), None, Vec::new());
    (facts, list())
}

/// The lane entries of `tickets` the tick's gate hands step 3.
fn ready(tickets: Vec<(TicketFacts, Vec<ListedPhase>)>) -> Vec<LaneEntry> {
    support::sorted(&support::gathered(tickets), time(100)).zaf
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
    let tasks = ready(vec![at_merge(441), at_merge(442)]);
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
    merged.0.pull.as_mut().expect("a pull").merged_at = Some(time(40));
    let tasks = ready(vec![at_implement(440), merged, at_merge(441)]);
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
    let tasks = ready(vec![at_merge(441), at_merge(442), at_merge(443)]);
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
    // The failed task is left as it was: at the next tick it is in the lane at merge again.
    let again = ready(vec![at_merge(441)]);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].next, name("merge"));
}

#[test]
fn a_merge_that_never_ran_is_listed_with_its_125() {
    let tasks = ready(vec![at_merge(441), at_merge(442)]);
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

    let tasks = ready(vec![at_merge(441), at_merge(442)]);
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
    let tasks = ready(vec![at_merge(441), at_implement(440), at_merge(442)]);
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
    let outcome = run_inline(
        &ready(vec![at_implement(440)]),
        &mut recording(&mut calls, |_| 0),
    );
    assert!(calls.is_empty());
    assert!(outcome.is_idle());
    assert_eq!(outcome, TickOutcome::default());
    assert_eq!(outcome.render_text(), "");
    assert_eq!(outcome.to_json()["idle"], true);
    assert!(run_inline(&[], &mut |_, _| 0).is_idle());
}
