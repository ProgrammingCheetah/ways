use std::collections::BTreeSet;
use std::convert::Infallible;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use super::*;
use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
use crate::app::schedule::tick::support;
use crate::app::spawn::inline::run_zaf_actions;
use crate::domain::CommitId;
use crate::domain::schedule::{
    CheckFact, CheckName, CheckResult, ExitCode, HeadEnd, PullFacts, RunExit, RunRecord,
    TicketFacts, weight,
};

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

/// implement (a model), ci (a wait whose on-fail is `on_fail`), merge (zaf).
fn list_failing_to(on_fail: Option<&str>) -> Vec<ListedPhase> {
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
            on_fail: on_fail.map(name),
        },
        ListedPhase {
            action: name("merge"),
            kind: ActionKind::Zaf,
            ends: PhaseEnd::Merged,
            on_fail: None,
        },
    ]
}

/// implement (a model), ci (a wait whose on-fail is fix), merge (zaf).
fn list() -> Vec<ListedPhase> {
    list_failing_to(Some("fix"))
}

/// The lane entry of `task` at `action`, of `size`, ready since `since`, weighed at second 100.
fn entry(task: u64, size: TaskSize, action: &str, since: u64) -> LaneEntry {
    LaneEntry {
        ticket: TicketId::from(task),
        next: name(action),
        since: time(since),
        size,
        weight: weight(time(since), None, time(100)),
    }
}

/// A task in the lane at implement since `since`.
fn ready(task: u64, size: TaskSize, since: u64) -> LaneEntry {
    entry(task, size, "implement", since)
}

/// A task whose ci failed at `since`, in the lane at its on-fail `fix`.
fn failed(task: u64, size: TaskSize, since: u64) -> LaneEntry {
    entry(task, size, "fix", since)
}

/// A task whose pull request opened at 20 at `head`, changing a file, its one check `ci` failed
/// at 30, its ci wait failing to `on_fail`.
fn failed_ci(task: u64, size: TaskSize, on_fail: Option<&str>) -> (TicketFacts, Vec<ListedPhase>) {
    let list = list_failing_to(on_fail);
    let pull = PullFacts {
        opened_at: time(20),
        merged_at: None,
        head: Some(CommitId::from("head")),
        changed_files: crate::domain::ChangedFiles::of(&["src/lib.rs"]),
        checks: vec![CheckFact {
            name: CheckName::from("ci"),
            result: CheckResult::Failed(time(30)),
        }],
        reviews: Vec::new(),
        mergeable: None,
        checks_expected: true,
    };
    let facts = support::facts(task, &list, size, time(1), Some(pull), Vec::new());
    (facts, list)
}

/// An epic whose `role` launch has waited since `since`.
fn epic(epic: u64, role: &str, since: u64) -> EpicCandidate {
    EpicCandidate {
        epic: TicketId::from(epic),
        role: RoleName::from(role),
        since: time(since),
    }
}

/// A budget of `units`, the default reserve and sizes.
fn budget(units: u32) -> Budget {
    Budget {
        units: Some(units),
        ..Budget::default()
    }
}

/// `units` in use, no E live, nothing running of four slots, at second 100: no phase of these
/// tests is old enough to starve.
fn in_use(units: u64) -> InUse {
    InUse {
        now: time(100),
        units: Units::whole(units),
        e_live: false,
        running_here: 0,
        slots: 4,
    }
}

fn planned(task: u64, action: &str, units: u32) -> PlannedModel {
    PlannedModel::Phase {
        task: TicketId::from(task),
        action: name(action),
        units,
    }
}

fn planned_epic(epic: u64, role: &str, units: u32) -> PlannedModel {
    PlannedModel::Epic {
        epic: TicketId::from(epic),
        role: RoleName::from(role),
        units,
    }
}

/// A start that records each model it is handed and launches nothing.
fn recording(
    calls: &mut Vec<PlannedModel>,
) -> impl FnMut(&PlannedModel) -> Result<(), Infallible> + '_ {
    move |planned| {
        calls.push(planned.clone());
        Ok(())
    }
}

#[test]
fn the_longest_ready_starts() {
    let standings = [ready(441, TaskSize::S, 20), ready(440, TaskSize::S, 10)];
    assert_eq!(
        plan(&standings, &[], &in_use(0), &budget(32)),
        Some(planned(440, "implement", 2)),
        "#440 has been ready longer, whatever the order it is read in"
    );
}

#[test]
fn a_phase_too_big_is_passed() {
    // 3 of 6 units in use: the M (4) waiting longest does not fit, the S (2) behind it does.
    let standings = [ready(440, TaskSize::M, 10), ready(441, TaskSize::S, 20)];
    assert_eq!(
        plan(&standings, &[], &in_use(3), &budget(6)),
        Some(planned(441, "implement", 2)),
        "the M is passed for the S that fits"
    );
    // Passed, it keeps its weight: once the budget frees, it is first again.
    assert_eq!(
        plan(&standings, &[], &in_use(0), &budget(6)),
        Some(planned(440, "implement", 4))
    );
    // Passed for a full project, it keeps its weight as well.
    let full = InUse {
        running_here: 4,
        ..in_use(0)
    };
    assert_eq!(plan(&standings, &[], &full, &budget(6)), None);
}

#[test]
fn a_failed_wait_starts_its_on_fail() {
    // Placed by the core and sorted by the tick's gate: the failed wait that names `fix` is a
    // model entry at fix, the one that names no on-fail is the Engineer's, not an item.
    let sorted = support::sorted(
        &support::gathered(vec![
            failed_ci(440, TaskSize::XS, None),
            failed_ci(441, TaskSize::M, Some("fix")),
        ]),
        time(100),
    );
    assert_eq!(
        sorted
            .failed_waits
            .iter()
            .map(|wait| wait.ticket.get())
            .collect::<Vec<_>>(),
        [440]
    );
    let mut entries = sorted.models;
    entries.push(ready(442, TaskSize::XS, 40));
    assert_eq!(
        plan(&entries, &[], &in_use(0), &budget(32)),
        Some(planned(441, "fix", 4)),
        "fix starts at the task's size, M; a failed wait with no on-fail is not an item"
    );
}

#[test]
fn nothing_fits_nothing_starts() {
    let standings = [ready(440, TaskSize::M, 10), failed(441, TaskSize::M, 5)];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &[],
        &in_use(30),
        &budget(32),
        &mut recording(&mut calls),
    )
    .expect("the tick passes");
    assert!(calls.is_empty(), "nothing is started");
    assert!(outcome.started.is_none());
    assert!(outcome.is_idle(), "the tick ends idle, not in error");

    // Neither does a task waiting, running, done or ready at a wait or a zaf action: the tick's
    // gate hands step 6 none of them.
    let (mut waiting, list) = failed_ci(442, TaskSize::XS, Some("fix"));
    waiting.pull.as_mut().expect("a pull").checks[0].result = CheckResult::Pending;
    let (mut running, _) = failed_ci(443, TaskSize::XS, Some("fix"));
    running.pull = None;
    running.runs.push(RunRecord {
        action: name("implement"),
        head: None,
        head_end: HeadEnd::Unknown,
        started_at: time(2),
        alive: true,
        exit: None,
        questions: BTreeSet::new(),
        last_line: None,
    });
    let (mut done, _) = failed_ci(444, TaskSize::XS, Some("fix"));
    done.pull.as_mut().expect("a pull").merged_at = Some(time(50));
    let (mut at_merge, _) = failed_ci(445, TaskSize::XS, Some("fix"));
    at_merge.pull.as_mut().expect("a pull").checks[0].result = CheckResult::Passed(time(30));
    let mut at_wait = failed_ci(446, TaskSize::XS, Some("fix"));
    at_wait.1[1].ends = PhaseEnd::Exit;
    at_wait.0.phases = crate::app::schedule::gather::phases_of(&at_wait.1);
    at_wait.0.pull.as_mut().expect("a pull").checks.clear();
    let sorted = support::sorted(
        &support::gathered(vec![
            (waiting, list.clone()),
            (running, list.clone()),
            (done, list.clone()),
            (at_merge, list),
            at_wait,
        ]),
        time(100),
    );
    assert!(sorted.models.is_empty(), "{sorted:?}");
    assert_eq!(sorted.zaf.len(), 1, "{sorted:?}");
    assert_eq!(sorted.waits.len(), 1, "{sorted:?}");
    assert_eq!(plan(&sorted.models, &[], &in_use(0), &budget(32)), None);
}

#[test]
fn one_model_per_tick() {
    let standings = [
        ready(440, TaskSize::XS, 10),
        ready(441, TaskSize::XS, 20),
        failed(442, TaskSize::XS, 30),
    ];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &[],
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

#[test]
fn an_older_phase_starts_before_an_epic() {
    let standings = [ready(440, TaskSize::M, 10)];
    let epics = [epic(421, "planning", 20)];
    assert_eq!(
        plan(&standings, &epics, &in_use(0), &budget(32)),
        Some(planned(440, "implement", 4)),
        "the phase has waited longer than the epic's stage"
    );
    // The other way round, the epic's stage has waited longer: it starts, the phase waits.
    let older = [epic(421, "challenger", 5)];
    assert_eq!(
        plan(&standings, &older, &in_use(0), &budget(32)),
        Some(planned_epic(421, "challenger", 1)),
        "the epic has waited longer than the phase"
    );
}

#[test]
fn a_planning_launch_costs_xs() {
    let epics = [epic(421, "planning", 10)];
    let none = Budget {
        units: Some(32),
        ..Budget::default()
    };
    assert_eq!(
        plan(&[], &epics, &in_use(0), &none),
        Some(planned_epic(
            421,
            "planning",
            whole_units(none.cost(EPIC_SIZE))
        )),
        "an epic launch is charged at the XS size, whatever the epic"
    );
    assert_eq!(EPIC_SIZE, TaskSize::XS);
    assert_eq!(
        plan(&[], &epics, &in_use(0), &budget(32)).map(|model| model.units()),
        Some(1)
    );
    // 5 of 6 units in use: an XS fits where the M (4) waiting longer does not.
    let standings = [ready(440, TaskSize::M, 1)];
    assert_eq!(
        plan(&standings, &epics, &in_use(5), &budget(6)),
        Some(planned_epic(421, "planning", 1)),
        "the launch is passed over to by size, not by role"
    );
}

#[test]
fn epics_share_the_one_model_start() {
    let standings = [ready(440, TaskSize::XS, 30)];
    let epics = [epic(421, "planning", 20), epic(422, "challenger", 10)];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &epics,
        &in_use(0),
        &budget(32),
        &mut recording(&mut calls),
    )
    .expect("the tick passes");
    assert_eq!(
        calls,
        vec![planned_epic(422, "challenger", 1)],
        "three fit, one starts: the epic that has waited longest"
    );
    assert_eq!(
        outcome.started,
        Some(Started {
            task: TicketId::from(422),
            action: name("challenger"),
            units: 1,
        })
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
    let outcome = start_outcome(&standings, &[], &in_use(0), &budget(32), &mut |_| {
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
    let standings = [failed(441, TaskSize::S, 10)];
    let mut calls = Vec::new();
    let outcome = start_outcome(
        &standings,
        &[],
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
    let result = start_outcome(&standings, &[], &in_use(0), &budget(32), &mut |_| {
        Err("no launch")
    });
    assert_eq!(result, Err("no launch"));
}

#[test]
fn the_entry_is_placed_from_the_task() {
    let facts = support::facts(440, &list(), TaskSize::M, time(7), None, Vec::new());
    let sorted = support::sorted(&support::gathered(vec![(facts, list())]), time(100));
    assert_eq!(sorted.models, [ready(440, TaskSize::M, 7)]);
    assert_eq!(
        plan(&sorted.models, &[], &in_use(0), &budget(32)),
        Some(planned(440, "implement", 4))
    );
}

#[test]
fn with_no_budget_only_the_slots_decide() {
    let standings = [ready(440, TaskSize::M, 10)];
    let none = Budget::default();
    assert_eq!(
        plan(&standings, &[], &in_use(1000), &none),
        Some(planned(440, "implement", 4))
    );
}

fn minutes(count: u64) -> u64 {
    count * 60
}

/// 3 of 6 units in use at `now_minutes`: an M (4) does not fit, an S (2) does.
fn crowded(now_minutes: u64) -> InUse {
    InUse {
        now: time(minutes(now_minutes)),
        ..in_use(3)
    }
}

#[test]
fn starve_after_is_thirty_minutes() {
    assert_eq!(STARVE_AFTER, Duration::from_secs(30 * 60));
}

#[test]
fn a_starved_phase_holds_the_budget() {
    // The M has been ready since minute 0 and does not fit; at minute 31 it is starved, so the
    // S behind it, which does fit, does not start.
    let standings = [
        ready(440, TaskSize::M, 0),
        ready(441, TaskSize::S, minutes(20)),
    ];
    assert_eq!(plan(&standings, &[], &crowded(31), &budget(6)), None);
    // Exactly 30 minutes is starved too.
    assert_eq!(plan(&standings, &[], &crowded(30), &budget(6)), None);
    // Once the budget frees up it is first, and starts.
    assert_eq!(
        plan(
            &standings,
            &[],
            &InUse {
                now: time(minutes(31)),
                ..in_use(0)
            },
            &budget(6)
        ),
        Some(planned(440, "implement", 4))
    );
    // A failed wait with an on-fail that does not fit holds the budget the same way.
    let failed_wait = [
        failed(440, TaskSize::M, 0),
        ready(441, TaskSize::S, minutes(20)),
    ];
    assert_eq!(plan(&failed_wait, &[], &crowded(31), &budget(6)), None);
    // And a full project holds it: nothing smaller starts past the slots either way.
    let full = InUse {
        running_here: 4,
        ..crowded(31)
    };
    assert_eq!(plan(&standings, &[], &full, &budget(6)), None);
}

#[test]
fn a_starved_epic_holds_the_budget_like_a_phase() {
    // Both are XS-or-smaller against 5 of 6 units: the M phase does not fit, the XS epic does.
    // The M has been ready since minute 0, so at minute 31 it is starved and the epic behind it
    // does not start; at minute 29 it is only passed.
    let standings = [ready(440, TaskSize::M, 0)];
    let epics = [epic(421, "planning", minutes(20))];
    let crowded_at = |now_minutes: u64| InUse {
        now: time(minutes(now_minutes)),
        ..in_use(5)
    };
    assert_eq!(plan(&standings, &epics, &crowded_at(31), &budget(6)), None);
    assert_eq!(
        plan(&standings, &epics, &crowded_at(29), &budget(6)),
        Some(planned_epic(421, "planning", 1))
    );
    // A starved epic holds it the same way against a phase behind it.
    let epics = [epic(421, "challenger", 0)];
    let behind = [ready(440, TaskSize::XS, minutes(20))];
    let full = InUse {
        running_here: 4,
        ..crowded_at(31)
    };
    assert_eq!(plan(&behind, &epics, &full, &budget(6)), None);
}

/// [`ready`] with the priority at `level`, weighed at `now`.
fn urgent(task: u64, size: TaskSize, since: u64, level: u8, now: u64) -> LaneEntry {
    LaneEntry {
        weight: weight(
            time(since),
            crate::domain::schedule::Priority::new(level),
            time(now),
        ),
        ..ready(task, size, since)
    }
}

#[test]
fn a_prioritised_phase_starts_before_an_older_one() {
    // #441 is ten minutes younger, but priority 1 is worth three hours of age.
    let standings = [
        ready(440, TaskSize::S, 0),
        urgent(441, TaskSize::S, minutes(10), 1, minutes(20)),
    ];
    let at = InUse {
        now: time(minutes(20)),
        ..in_use(0)
    };
    assert_eq!(
        plan(&standings, &[], &at, &budget(32)),
        Some(planned(441, "implement", 2))
    );
    // Priority 3 is worth one hour: still more than ten minutes.
    let standings = [
        ready(440, TaskSize::S, 0),
        urgent(441, TaskSize::S, minutes(10), 3, minutes(20)),
    ];
    assert_eq!(
        plan(&standings, &[], &at, &budget(32)),
        Some(planned(441, "implement", 2))
    );
    // An epic launch has no priority: the prioritised phase goes first.
    let epics = [epic(421, "planning", 0)];
    assert_eq!(
        plan(&standings, &epics, &at, &budget(32)),
        Some(planned(441, "implement", 2))
    );
}

#[test]
fn a_starved_big_ticket_still_holds_the_line() {
    // The M has waited 40 minutes and does not fit; the S behind it carries priority 1 and does.
    // Starvation is on raw age, so the S does not start: the budget frees up for the M.
    let standings = [
        ready(440, TaskSize::M, 0),
        urgent(441, TaskSize::S, minutes(39), 1, minutes(40)),
    ];
    assert_eq!(plan(&standings, &[], &crowded(40), &budget(6)), None);
    // Before the M starves, the prioritised S is passed through.
    let standings = [
        ready(440, TaskSize::M, 0),
        urgent(441, TaskSize::S, minutes(20), 1, minutes(29)),
    ];
    assert_eq!(
        plan(&standings, &[], &crowded(29), &budget(6)),
        Some(planned(441, "implement", 2))
    );
}

#[test]
fn a_young_phase_does_not() {
    // At minute 29 the M is not starved: it is passed for the S that fits.
    let standings = [
        ready(440, TaskSize::M, 0),
        ready(441, TaskSize::S, minutes(20)),
    ];
    assert_eq!(
        plan(&standings, &[], &crowded(29), &budget(6)),
        Some(planned(441, "implement", 2))
    );
    // A ready time after the tick's clock is young, not starved.
    assert_eq!(
        plan(&standings, &[], &crowded(0), &budget(6)),
        Some(planned(441, "implement", 2))
    );
}

#[test]
fn a_starved_phase_lets_zaf_actions_run() {
    let standings = [ready(440, TaskSize::M, 0)];
    let merge_list = vec![
        ListedPhase {
            action: name("implement"),
            kind: ActionKind::Model,
            ends: PhaseEnd::Exit,
            on_fail: None,
        },
        ListedPhase {
            action: name("merge"),
            kind: ActionKind::Zaf,
            ends: PhaseEnd::Merged,
            on_fail: None,
        },
    ];
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
    // D1: the list has no `pr-open` phase, so implement's `exit` is keyed on the head: the run
    // left the pull request's head.
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
    let at_merge = support::facts(
        441,
        &merge_list,
        TaskSize::S,
        time(1),
        Some(pull),
        vec![implemented],
    );
    let sorted = support::sorted(&support::gathered(vec![(at_merge, merge_list)]), time(100));
    assert_eq!(
        plan(&standings, &[], &crowded(31), &budget(6)),
        None,
        "the M is starved and holds the budget"
    );
    let mut calls = Vec::new();
    let ran = run_zaf_actions(&sorted.zaf, &mut |task, action| {
        calls.push((task.get(), action.as_str().to_owned()));
        0
    });
    assert_eq!(
        calls,
        vec![(441, "merge".to_owned())],
        "the merge still runs"
    );
    assert!(ran.iter().all(crate::app::spawn::outcome::Ran::passed));
}
