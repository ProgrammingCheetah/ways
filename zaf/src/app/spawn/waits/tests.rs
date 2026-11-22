use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
use crate::app::schedule::tick::support;
use crate::app::spawn::outcome::Started;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::schedule::{
    ExitCode, HeadEnd, Place, PullFacts, RunExit, RunRecord, TicketFacts, place,
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

/// D1: the list has no `pr-open` phase, so implement's `exit` is keyed on the head: its run left
/// the pull request's head.
fn implemented() -> RunRecord {
    RunRecord {
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
    }
}

/// A task that implemented and opened its pull request: it is in the lane at review.
fn at_review(task: u64) -> (TicketFacts, Vec<ListedPhase>) {
    let pull = PullFacts {
        opened_at: time(10),
        merged_at: None,
        head: Some(CommitId::from("head")),
        changed_files: crate::domain::ChangedFiles::of(&["src/lib.rs"]),
        checks: Vec::new(),
        reviews: Vec::new(),
        mergeable: None,
        checks_expected: true,
    };
    let facts = support::facts(
        task,
        &list(),
        TaskSize::S,
        time(1),
        Some(pull),
        vec![implemented()],
    );
    (facts, list())
}

/// A task with no pull request yet: it is in the lane at the model action.
fn at_implement(task: u64) -> (TicketFacts, Vec<ListedPhase>) {
    let facts = support::facts(task, &list(), TaskSize::S, time(1), None, Vec::new());
    (facts, list())
}

/// The lane entries of `tickets` the tick's gate hands step 4.
fn ready(tickets: Vec<(TicketFacts, Vec<ListedPhase>)>) -> Vec<LaneEntry> {
    support::sorted(&support::gathered(tickets), time(100)).waits
}

/// The review wait's run at `head`, started, with `alive` and `exit` as given. A run that ended
/// left the head it started at; one with no exit recorded no end head.
fn review_run(head: &str, alive: bool, exit: Option<i32>) -> RunRecord {
    RunRecord {
        action: name("review"),
        head: Some(CommitId::from(head)),
        head_end: match exit {
            Some(_) => HeadEnd::Left(Some(CommitId::from(head))),
            None => HeadEnd::Unknown,
        },
        started_at: time(11),
        alive,
        exit: exit.map(|code| RunExit {
            code: ExitCode::from(code),
            at: time(15),
        }),
        questions: BTreeSet::new(),
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
    let tasks = ready(vec![at_review(441), at_review(442)]);
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
    // waits it lists run once their run is alive, not ended.
    let mut task = at_review(441);
    let waits = launch_waits(&ready(vec![task.clone()]), &mut |_, _| Ok(()), &mut |_| {});
    assert_eq!(waits.len(), 1);
    task.0.runs.push(review_run("head", true, None));
    assert!(
        matches!(place(&task.0, time(100)), Place::Running(run) if run.phase == name("review"))
    );
    assert!(ready(vec![task]).is_empty());
}

#[test]
fn only_a_task_ready_at_a_wait_launches() {
    let mut merged = at_review(443);
    merged.0.pull.as_mut().expect("a pull").merged_at = Some(time(40));
    let tasks = ready(vec![at_implement(440), merged, at_review(441)]);
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
    ran.0.runs.push(review_run("head", false, Some(0)));
    let mut died = at_review(442);
    died.0.runs.push(review_run("head", false, None));
    let tasks = ready(vec![ran, died, at_review(443)]);
    let mut calls = Vec::new();
    let waits = launch_waits(&tasks, &mut recording(&mut calls, |_| Ok(())), &mut |_| {});
    // D7: a wait whose run died with no exit code is run again (#442); it used to wait forever.
    assert_eq!(
        calls,
        vec![(442, "review".to_owned()), (443, "review".to_owned())],
        "a review run at the current head waits for its fact and is not started again"
    );
    assert_eq!(waits.len(), 2);
}

#[test]
fn a_wait_run_at_an_old_head_launches_again() {
    let mut task = at_review(441);
    task.0.runs.push(review_run("old", false, Some(0)));
    let mut calls = Vec::new();
    launch_waits(
        &ready(vec![task]),
        &mut recording(&mut calls, |_| Ok(())),
        &mut |_| {},
    );
    assert_eq!(calls, vec![(441, "review".to_owned())], "a push reopens it");
}

#[test]
fn a_failed_wait_launch_does_not_stop_others() {
    let tasks = ready(vec![at_review(441), at_review(442), at_review(443)]);
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
    // The failed task is left as it was: at the next tick it is in the lane at review again.
    let again = ready(vec![at_review(441)]);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].next, name("review"));
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

    let tasks = ready(vec![at_review(441), at_review(442)]);
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
    let tickets = vec![at_review(441), at_implement(440), at_review(442)];
    let sorted = support::sorted(&support::gathered(tickets), time(100));
    let mut calls = Vec::new();
    let mut outcome = launch_outcome(
        &sorted.waits,
        &mut recording(&mut calls, |_| Ok(())),
        &mut |_| {},
    );
    assert_eq!(outcome.waits.len(), 2);
    assert!(outcome.started.is_none(), "a wait is not a model start");
    assert_eq!(
        sorted
            .models
            .iter()
            .map(|entry| entry.ticket.get())
            .collect::<Vec<_>>(),
        [440],
        "the model phase is no wait: nothing launched it, and it is still step 6's"
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
    let tasks = ready(vec![at_review(441), at_implement(440), at_review(442)]);
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
        &ready(vec![at_implement(440)]),
        &mut recording(&mut calls, |_| Ok(())),
        &mut |_| {},
    );
    assert!(calls.is_empty());
    assert!(outcome.is_idle());
    assert_eq!(outcome, TickOutcome::default());
}
