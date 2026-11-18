use super::super::tests::*;
use super::*;
use crate::domain::schedule::{CheckName, CheckResult, Dependency, EpicFacts, PhaseEnd, PullFacts};
use crate::domain::{BodyHash, EpicTask, Signature};

/// The pack's ticket with a pull request at `h1` opened at `opened` and no check.
fn waiting_since(opened: u64) -> TicketFacts {
    let mut facts = facts(pack());
    facts.pull = Some(pull("h1", opened));
    facts
}

const HOURS_2: u64 = 2 * 60 * 60;

#[test]
fn the_check_clock_is_the_latest_of_the_opening_the_run_that_left_the_head_and_the_last_check() {
    let mut facts = waiting_since(20);
    assert_eq!(check_clock(&facts), Some(time(20)));
    facts
        .runs
        .push(run("fix", Some("h0"), 25).exits(0, 40).leaves(Some("h1")));
    facts.runs.push(
        run("review", Some("h0"), 50)
            .exits(0, 60)
            .leaves(Some("h0")),
    );
    assert_eq!(check_clock(&facts), Some(time(40)));
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![passed(45), pending()];
    }
    assert_eq!(check_clock(&facts), Some(time(45)));
    assert_eq!(check_clock(&self::facts(pack())), None);
}

const MINUTES_10: u64 = 10 * 60;

#[test]
fn no_check_is_not_overdue_before_ten_minutes() {
    let facts = waiting_since(20);
    assert_eq!(overdue(&facts, time(20 + MINUTES_10 - 60)), None);
    assert_eq!(overdue(&facts, time(20 + MINUTES_10 - 1)), None);
}

#[test]
fn no_check_is_overdue_after_ten_minutes() {
    let facts = waiting_since(20);
    assert_eq!(
        NO_CHECK_DEADLINE,
        std::time::Duration::from_secs(MINUTES_10)
    );
    assert_eq!(
        overdue(&facts, time(20 + MINUTES_10)),
        Some(OverdueCheck {
            head: commit("h1"),
            since: time(20),
            awaited: Awaited::NoCheck,
        })
    );
}

#[test]
fn pending_check_keeps_the_two_hour_deadline() {
    let mut facts = waiting_since(20);
    facts.pull = Some(PullFacts {
        checks: vec![check("lint", CheckResult::Pending)],
        ..pull("h1", 20)
    });
    assert_eq!(CHECK_DEADLINE, std::time::Duration::from_secs(HOURS_2));
    assert_eq!(overdue(&facts, time(20 + MINUTES_10)), None);
    assert_eq!(overdue(&facts, time(20 + HOURS_2 - 1)), None);
    assert_eq!(
        overdue(&facts, time(20 + HOURS_2)).map(|overdue| overdue.awaited),
        Some(Awaited::Pending(vec![CheckName::from("lint")]))
    );
}

#[test]
fn outside_wait_keeps_the_two_hour_deadline() {
    // The checks passed at h1; review exited 0 there at 40 and the review never came.
    let mut facts = facts(vec![
        phase("implement", &[PhaseEnd::PrOpen], None),
        phase("ci", &[PhaseEnd::CiDone], Some("fix")),
        phase("review", &[PhaseEnd::ReviewPosted], Some("fix")),
    ]);
    facts.pull = Some(PullFacts {
        checks: vec![passed(30)],
        ..pull("h1", 20)
    });
    facts.runs.push(
        run("review", Some("h1"), 35)
            .exits(0, 40)
            .leaves(Some("h1")),
    );
    assert_eq!(overdue(&facts, time(40 + MINUTES_10)), None);
    assert!(overdue(&facts, time(40 + HOURS_2)).is_some());
    // A base wait keeps it too.
    let held = held_by_the_base();
    assert_eq!(overdue(&held, time(32) + NO_CHECK_DEADLINE), None);
    assert!(overdue(&held, time(32) + CHECK_DEADLINE).is_some());
}

#[test]
fn a_ticket_not_waiting_on_a_check_is_never_overdue() {
    let mut green = waiting_since(20);
    if let Some(pull) = green.pull.as_mut() {
        pull.checks = vec![passed(30)];
    }
    assert_eq!(overdue(&green, now()), None);
    let mut running = waiting_since(20);
    running.runs.push(run("review", Some("h1"), 25).lives());
    assert_eq!(overdue(&running, now()), None);
    assert_eq!(overdue(&facts(pack()), now()), None);
}

#[test]
fn a_runner_that_is_down_leaves_a_pending_check_overdue() {
    let mut facts = waiting_since(20);
    facts.pull = Some(PullFacts {
        checks: vec![passed(30), check("lint", CheckResult::Pending)],
        ..pull("h1", 20)
    });
    assert_eq!(
        overdue(&facts, now()).map(|overdue| overdue.awaited),
        Some(Awaited::Pending(vec![CheckName::from("lint")]))
    );
}

#[test]
fn a_question_and_an_unsigned_epic_have_no_clock() {
    // Waiting on the checks at h1 since 20, far past the deadline, but also on a question.
    let mut questioned = waiting_since(20);
    questioned.open_questions.insert(id(45));
    assert_eq!(overdue(&questioned, now()), None);
    // Or on its epic, unsigned or not challenged yet: the epic's own deadline covers it.
    for (signed, challenged) in [(false, false), (true, false)] {
        let mut held = waiting_since(20);
        held.epic = Some(EpicFacts {
            id: id(1),
            signed,
            challenged,
        });
        assert_eq!(overdue(&held, now()), None, "signed {signed}");
    }
    // Waiting on another ticket as well keeps the clock: nobody has heard of that one.
    let mut blocked = waiting_since(20);
    blocked.dependencies.push(Dependency {
        ticket: id(9),
        standing: Standing::Open,
    });
    assert!(overdue(&blocked, now()).is_some());
}

#[test]
fn a_phase_awaiting_the_outside_names_what_it_awaits() {
    // The checks passed at h1; review, a wait ending at a review posted, exited 0 there at 40 and
    // the review never came.
    let mut facts = facts(vec![
        phase("implement", &[PhaseEnd::PrOpen], None),
        phase("ci", &[PhaseEnd::CiDone], Some("fix")),
        phase("review", &[PhaseEnd::ReviewPosted], Some("fix")),
    ]);
    facts.pull = Some(PullFacts {
        checks: vec![passed(30)],
        ..pull("h1", 20)
    });
    facts.runs.push(
        run("review", Some("h1"), 35)
            .exits(0, 40)
            .leaves(Some("h1")),
    );
    assert_eq!(overdue(&facts, time(40 + HOURS_2 - 1)), None);
    assert_eq!(
        overdue(&facts, time(40 + HOURS_2)),
        Some(OverdueCheck {
            head: commit("h1"),
            since: time(40),
            awaited: Awaited::Outside {
                phase: name("review"),
                ends: vec![PhaseEnd::ReviewPosted],
            },
        })
    );
}

fn depends(ticket: u64, on: &[(u64, Standing)]) -> TicketFacts {
    let mut facts = facts(pack());
    facts.ticket = id(ticket);
    facts.dependencies = on
        .iter()
        .map(|&(dependency, standing)| Dependency {
            ticket: id(dependency),
            standing,
        })
        .collect();
    facts
}

#[test]
fn a_dependency_closed_as_not_planned_or_missing_is_dead() {
    let facts = depends(
        10,
        &[
            (20, Standing::ClosedNotPlanned),
            (21, Standing::Missing),
            (22, Standing::Done),
            (23, Standing::Open),
        ],
    );
    let graph = dependency_graph(std::slice::from_ref(&facts));
    assert_eq!(
        dead_dependencies(&facts, &graph),
        [
            DeadDependency {
                dependency: id(20),
                reason: DeadReason::ClosedNotPlanned,
            },
            DeadDependency {
                dependency: id(21),
                reason: DeadReason::Missing,
            },
        ]
    );
    assert_eq!(
        DeadReason::ClosedNotPlanned.to_string(),
        "closed as not planned"
    );
}

#[test]
fn a_dependency_cycle_is_dead() {
    let first = depends(10, &[(11, Standing::Open)]);
    let second = depends(11, &[(12, Standing::Open)]);
    let third = depends(12, &[(10, Standing::Open)]);
    let tickets = [first.clone(), second, third];
    let graph = dependency_graph(&tickets);
    assert_eq!(
        dead_dependencies(&first, &graph),
        [DeadDependency {
            dependency: id(11),
            reason: DeadReason::Cycle,
        }]
    );
    let itself = depends(13, &[(13, Standing::Open)]);
    let graph = dependency_graph(std::slice::from_ref(&itself));
    assert_eq!(
        dead_dependencies(&itself, &graph),
        [DeadDependency {
            dependency: id(13),
            reason: DeadReason::Cycle,
        }]
    );
}

#[test]
fn an_open_dependency_off_any_cycle_and_a_done_one_are_alive() {
    let first = depends(10, &[(11, Standing::Open)]);
    let second = depends(11, &[(12, Standing::Open)]);
    // 12 names 10, but as done: a done dependency closes the cycle through it.
    let third = depends(12, &[(10, Standing::Done)]);
    let tickets = [first.clone(), second, third];
    let graph = dependency_graph(&tickets);
    assert!(dead_dependencies(&first, &graph).is_empty());
    assert_eq!(graph.get(&id(12)), Some(&Vec::new()));
}

const DAY: u64 = 24 * 60 * 60;

/// Epic #1, signed at 10 and planned at `planned` (none: planning left no time), with one task,
/// challenged as `challenged`.
fn planned_epic(planned: Option<u64>, challenged: bool) -> Epic {
    let body = "Goal: ship it".into();
    Epic {
        id: id(1),
        title: "Epic".into(),
        signature: Some(Signature {
            hash: BodyHash::of(&body),
            at: time(10),
        }),
        body,
        tasks: vec![EpicTask {
            number: id(2),
            title: "Task".into(),
            state: None,
            standing: Standing::Open,
            group: None,
            kind: None,
        }],
        planned_at: planned.map(time),
        challenged,
    }
}

#[test]
fn an_epic_not_challenged_for_a_day_is_overdue() {
    assert_eq!(CHALLENGE_DEADLINE, std::time::Duration::from_secs(DAY));
    let epic = planned_epic(Some(20), false);
    assert_eq!(unchallenged(&epic, false, time(20 + DAY - 1)), None);
    assert_eq!(
        unchallenged(&epic, false, time(20 + DAY)),
        Some(OverdueChallenge {
            epic: id(1),
            since: time(20),
        })
    );
    // With no time for the end of planning, the signature starts the clock.
    assert_eq!(
        unchallenged(&planned_epic(None, false), false, time(10 + DAY)).map(|late| late.since),
        Some(time(10))
    );
}

#[test]
fn only_a_planned_epic_with_no_question_has_a_challenge_clock() {
    let late = time(20 + 2 * DAY);
    assert_eq!(
        unchallenged(&planned_epic(Some(20), true), false, late),
        None
    );
    assert_eq!(
        unchallenged(&planned_epic(Some(20), false), true, late),
        None,
        "a question on the epic: the Engineer has it"
    );
    let mut unsigned = planned_epic(Some(20), false);
    unsigned.body = "Goal: edited since".into();
    assert_eq!(unchallenged(&unsigned, false, late), None, "unsigned");
    let mut empty = planned_epic(Some(20), false);
    empty.tasks.clear();
    assert_eq!(unchallenged(&empty, false, late), None, "planning's turn");
}

/// A ticket green and reviewed at `h1`, at its merge since 32, over an integration branch at
/// `b1` with `lint` passed and `ci` still pending.
fn held_by_the_base() -> TicketFacts {
    use crate::domain::schedule::BaseFacts;

    let mut facts = facts(pack());
    facts.pull = Some(PullFacts {
        checks: vec![passed(30)],
        ..pull("h1", 20)
    });
    facts.runs.push(
        run("review", Some("h1"), 31)
            .exits(0, 32)
            .leaves(Some("h1")),
    );
    facts.merging.base = BaseFacts {
        head: Some(commit("b1")),
        checks: vec![check("lint", CheckResult::Passed(time(31))), pending()],
    };
    facts
}

#[test]
fn a_merge_held_by_the_integration_branch_is_overdue_after_the_deadline_and_not_before() {
    let facts = held_by_the_base();
    let deadline = time(32) + CHECK_DEADLINE;
    assert_eq!(base_clock(&facts), Some(time(32)), "since its review ended");
    assert_eq!(
        place(&facts, deadline),
        waiting(&[WaitingOn::Base(commit("b1"))])
    );
    assert_eq!(overdue(&facts, deadline - Duration::from_secs(1)), None);
    assert_eq!(
        overdue(&facts, deadline),
        Some(OverdueCheck {
            head: commit("b1"),
            since: time(32),
            awaited: Awaited::Base(vec![CheckName::from("ci")]),
        })
    );
}

#[test]
fn a_merge_held_by_the_integration_branch_has_no_clock_while_a_question_is_open() {
    let mut facts = held_by_the_base();
    facts.open_questions.insert(id(61));
    let late = time(32) + CHECK_DEADLINE * 10;
    assert_eq!(place(&facts, late), waiting(&[WaitingOn::Question(id(61))]));
    assert_eq!(overdue(&facts, late), None);
}

#[test]
fn a_ruling_at_the_branchs_head_lets_the_ticket_through_and_one_at_another_head_does_not() {
    let late = time(32) + CHECK_DEADLINE * 10;
    let mut facts = held_by_the_base();
    facts.merging.ruled_at = Some(commit("b1"));
    assert_eq!(place(&facts, late), in_lane("merge", 32));
    assert_eq!(overdue(&facts, late), None);
    // The branch moved since the ruling: its new head holds the ticket again.
    facts.merging.ruled_at = Some(commit("b0"));
    assert_eq!(
        place(&facts, late),
        waiting(&[WaitingOn::Base(commit("b1"))])
    );
    assert!(overdue(&facts, late).is_some());
}

#[test]
fn a_wait_on_another_tickets_merge_has_no_clock() {
    let mut facts = held_by_the_base();
    facts.merging.base = Default::default();
    facts.merging.first = Some(id(12));
    let late = time(32) + CHECK_DEADLINE * 10;
    assert_eq!(place(&facts, late), waiting(&[WaitingOn::Merge(id(12))]));
    assert_eq!(overdue(&facts, late), None);
}
