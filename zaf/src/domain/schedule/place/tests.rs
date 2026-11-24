use std::collections::BTreeSet;

use super::super::tests::*;
use super::*;
use crate::domain::TaskSize;
use crate::domain::schedule::{CheckFact, Dependency, EpicFacts, PhaseEnd, PullFacts, TicketFacts};

/// How the epic stands in a combination.
#[derive(Debug, Clone, Copy)]
enum EpicIs {
    None,
    Unsigned,
    Unchallenged,
    Cleared,
}

/// How the checks stand in a combination.
#[derive(Debug, Clone, Copy)]
enum ChecksAre {
    NoPull,
    None,
    Pending,
    Passed,
    Failed,
}

/// The review run of a combination.
#[derive(Debug, Clone, Copy)]
enum RunIs {
    None,
    ExitZero,
    ExitOne,
    Dead,
}

/// One combination of facts.
#[derive(Debug, Clone, Copy)]
struct Combination {
    live: bool,
    merged: bool,
    epic: EpicIs,
    question: bool,
    dependency: Option<Standing>,
    checks: ChecksAre,
    run: RunIs,
}

/// Every combination, with merged only where there is a pull request to merge.
fn combinations() -> Vec<Combination> {
    let mut all = Vec::new();
    for live in [false, true] {
        for merged in [false, true] {
            for epic in [
                EpicIs::None,
                EpicIs::Unsigned,
                EpicIs::Unchallenged,
                EpicIs::Cleared,
            ] {
                for question in [false, true] {
                    for dependency in [
                        None,
                        Some(Standing::Open),
                        Some(Standing::Done),
                        Some(Standing::ClosedNotPlanned),
                        Some(Standing::Missing),
                    ] {
                        for checks in [
                            ChecksAre::NoPull,
                            ChecksAre::None,
                            ChecksAre::Pending,
                            ChecksAre::Passed,
                            ChecksAre::Failed,
                        ] {
                            for run in [RunIs::None, RunIs::ExitZero, RunIs::ExitOne, RunIs::Dead] {
                                if merged && matches!(checks, ChecksAre::NoPull) {
                                    continue;
                                }
                                all.push(Combination {
                                    live,
                                    merged,
                                    epic,
                                    question,
                                    dependency,
                                    checks,
                                    run,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    all
}

const EPIC: u64 = 1;
const DEPENDENCY: u64 = 2;
const QUESTION: u64 = 3;

/// The facts of `combination` for ticket `number`, on the pack's list.
fn facts_of(number: u64, combination: Combination) -> TicketFacts {
    let mut facts = facts(pack());
    facts.ticket = id(number);
    facts.epic = match combination.epic {
        EpicIs::None => None,
        EpicIs::Unsigned => Some((false, true)),
        EpicIs::Unchallenged => Some((true, false)),
        EpicIs::Cleared => Some((true, true)),
    }
    .map(|(signed, challenged)| EpicFacts {
        id: id(EPIC),
        signed,
        challenged,
        voided: false,
    });
    if combination.question {
        facts.open_questions.insert(id(QUESTION));
    }
    facts.dependencies = combination
        .dependency
        .map(|standing| Dependency::blocked_by(id(DEPENDENCY), standing))
        .into_iter()
        .collect();
    let checks: Vec<CheckFact> = match combination.checks {
        ChecksAre::NoPull | ChecksAre::None => Vec::new(),
        ChecksAre::Pending => vec![pending()],
        ChecksAre::Passed => vec![passed(30)],
        ChecksAre::Failed => vec![failed(30)],
    };
    if !matches!(combination.checks, ChecksAre::NoPull) {
        facts.pull = Some(PullFacts {
            checks,
            merged_at: combination.merged.then(|| time(60)),
            ..pull("h1", 20)
        });
    }
    let review = run("review", Some("h1"), 40);
    match combination.run {
        RunIs::None => {}
        RunIs::ExitZero => facts.runs.push(review.exits(0, 45)),
        RunIs::ExitOne => facts.runs.push(review.exits(1, 45)),
        RunIs::Dead => facts.runs.push(review),
    }
    if combination.live {
        facts.runs.push(run("implement", None, 5).lives());
    }
    facts
}

/// Which of the four places.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Running,
    Waiting(BTreeSet<WaitingOn>),
    Lane(ActionName),
    Done,
}

/// The place a combination must get, worked out apart from [`place()`].
fn expected(number: u64, combination: Combination) -> Kind {
    if combination.live {
        return Kind::Running;
    }
    if combination.merged {
        return Kind::Done;
    }
    let mut waits = BTreeSet::new();
    if matches!(combination.epic, EpicIs::Unsigned | EpicIs::Unchallenged) {
        waits.insert(WaitingOn::Epic(id(EPIC)));
    }
    if combination.question {
        waits.insert(WaitingOn::Question(id(QUESTION)));
    }
    if combination
        .dependency
        .is_some_and(|standing| standing != Standing::Done)
    {
        waits.insert(WaitingOn::Ticket(id(DEPENDENCY)));
    }
    if matches!(combination.checks, ChecksAre::None | ChecksAre::Pending) {
        waits.insert(WaitingOn::Check(id(number), commit("h1")));
    }
    if !waits.is_empty() {
        return Kind::Waiting(waits);
    }
    let next = match (combination.checks, combination.run) {
        (ChecksAre::NoPull, _) => "implement",
        (ChecksAre::Failed, _) | (ChecksAre::Passed, RunIs::ExitOne) => "fix",
        (ChecksAre::Passed, RunIs::ExitZero) => "merge",
        _ => "review",
    };
    Kind::Lane(name(next))
}

fn kind_of(place: &Place) -> Kind {
    match place {
        Place::Running(_) => Kind::Running,
        Place::Waiting(waits) => Kind::Waiting(waits.clone()),
        Place::InLane { next, .. } => Kind::Lane(next.clone()),
        Place::Done => Kind::Done,
    }
}

/// The scenes of every combination, each its own ticket from 100 up.
fn scenes() -> Vec<(u64, Combination, TicketFacts)> {
    combinations()
        .into_iter()
        .zip(100..)
        .map(|(combination, number)| (number, combination, facts_of(number, combination)))
        .collect()
}

#[test]
fn every_ticket_sits_in_exactly_one_place() {
    let scenes = scenes();
    let tickets: Vec<TicketFacts> = scenes.iter().map(|(_, _, facts)| facts.clone()).collect();
    let placement = Placement::of(&tickets, now());
    let waiting = placement.waiting_tickets();
    let mut wrong = Vec::new();
    for (number, combination, facts) in &scenes {
        let ticket = id(*number);
        let places = [
            placement.running.contains_key(&ticket),
            waiting.contains(&ticket),
            placement.lane.contains(ticket),
            placement.done.contains(&ticket),
        ];
        let count = places.iter().filter(|holds| **holds).count();
        if count != 1 {
            wrong.push(format!("{combination:?} sits in {count} places"));
        }
        let placed = kind_of(&place(facts, now()));
        let want = expected(*number, *combination);
        if placed != want {
            wrong.push(format!(
                "{combination:?}: placed {placed:?}, expected {want:?}"
            ));
        }
        if let Kind::Waiting(waits) = &placed {
            for waiting_on in waits {
                let listed = placement
                    .waiting
                    .get(waiting_on)
                    .is_some_and(|tickets| tickets.contains(&ticket));
                if !listed {
                    wrong.push(format!(
                        "{combination:?} is not listed under {waiting_on:?}"
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let total =
        placement.running.len() + waiting.len() + placement.lane.len() + placement.done.len();
    assert_eq!(total, scenes.len());
}

#[test]
fn running_comes_first_even_with_an_open_question() {
    let mut facts = facts(pack());
    facts.runs.push(run("implement", None, 5).lives());
    facts.open_questions.insert(id(QUESTION));
    facts.epic = Some(EpicFacts {
        id: id(EPIC),
        signed: false,
        challenged: false,
        voided: false,
    });
    facts
        .dependencies
        .push(Dependency::blocked_by(id(DEPENDENCY), Standing::Open));
    facts.pull = Some(PullFacts {
        checks: vec![pending()],
        ..pull("h1", 20)
    });
    assert_eq!(
        place(&facts, now()),
        Place::Running(Run {
            phase: name("implement"),
            head: None,
            started_at: time(5),
        })
    );
}

#[test]
fn placement_is_total_and_deterministic() {
    for (_, combination, facts) in scenes() {
        let first = place(&facts, now());
        assert_eq!(place(&facts, now()), first, "{combination:?}");
        let mut reversed = facts.clone();
        reversed.runs.reverse();
        assert_eq!(place(&reversed, now()), first, "{combination:?}");
    }
    let tickets: Vec<TicketFacts> = scenes().into_iter().map(|(_, _, facts)| facts).collect();
    let mut backwards = tickets.clone();
    backwards.reverse();
    assert_eq!(
        Placement::of(&tickets, now()),
        Placement::of(&backwards, now())
    );
}

#[test]
fn a_ticket_whose_phases_all_ended_is_done_and_waits_on_nothing() {
    let mut facts = facts(vec![phase("implement", &[PhaseEnd::PrOpen], None)]);
    facts.pull = Some(pull("h1", 20));
    facts
        .dependencies
        .push(Dependency::blocked_by(id(DEPENDENCY), Standing::Open));
    assert_eq!(place(&facts, now()), Place::Done);
}

#[test]
fn a_ticket_with_no_phase_is_done_as_today() {
    assert_eq!(place(&facts(Vec::new()), now()), Place::Done);
}

#[test]
fn a_ticket_waits_on_every_thing_that_holds() {
    let mut facts = facts(pack());
    facts.dependencies = vec![
        Dependency::blocked_by(id(5), Standing::Open),
        Dependency::blocked_by(id(6), Standing::Missing),
        Dependency::blocked_by(id(7), Standing::Done),
    ];
    facts.open_questions.insert(id(QUESTION));
    let placed = place(&facts, now());
    assert_eq!(
        placed,
        waiting(&[
            WaitingOn::Question(id(QUESTION)),
            WaitingOn::Ticket(id(5)),
            WaitingOn::Ticket(id(6)),
        ])
    );
    let Place::Waiting(waits) = placed else {
        unreachable!("it waits");
    };
    assert_eq!(
        waits.first(),
        Some(&WaitingOn::Question(id(QUESTION))),
        "the first of the set is the first that holds"
    );
}

#[test]
fn since_falls_back_to_the_time_handed_in() {
    let mut facts = facts(pack());
    facts.ready_since = None;
    assert_eq!(place(&facts, now()), in_lane("implement", NOW));
}

#[test]
fn a_ticket_given_twice_is_placed_once_by_its_first_facts() {
    let first = facts(pack());
    let mut second = facts(pack());
    second.runs.push(run("implement", None, 5).lives());
    let placement = Placement::of(&[first, second], now());
    assert!(placement.running.is_empty());
    assert_eq!(placement.lane.len(), 1);
}

#[test]
fn the_lane_is_ordered_by_weight() {
    let mut older = facts(pack());
    older.ticket = id(10);
    older.ready_since = Some(time(1));
    let mut younger = facts(pack());
    younger.ticket = id(11);
    younger.ready_since = Some(time(50));
    let placement = Placement::of(&[younger, older], now());
    let order: Vec<TicketId> = placement
        .lane
        .entries()
        .iter()
        .map(|entry| entry.ticket)
        .collect();
    assert_eq!(order, [id(10), id(11)]);
    assert_eq!(
        placement.lane.front().map(|entry| entry.size),
        Some(TaskSize::M)
    );
}

/// An epic signed and challenged once, its challenge voided since.
fn voided_epic() -> EpicFacts {
    EpicFacts {
        id: id(EPIC),
        signed: true,
        challenged: false,
        voided: true,
    }
}

/// A ticket between two phases: its `implement` ran and opened the pull request, and its `ci`
/// has not finished.
fn between_phases() -> TicketFacts {
    let mut facts = facts(pack());
    facts.runs.push(run("implement", None, 5).exits(0, 10));
    facts.pull = Some(PullFacts {
        checks: vec![passed(30)],
        ..pull("h1", 20)
    });
    facts
}

#[test]
fn voided_unstarted_ticket_waits_on_epic() {
    let mut facts = facts(pack());
    facts.epic = Some(voided_epic());
    assert_eq!(
        place(&facts, now()),
        Place::Waiting([WaitingOn::Epic(id(EPIC))].into())
    );
}

#[test]
fn voided_ticket_between_phases_goes_on() {
    let mut facts = between_phases();
    facts.epic = Some(voided_epic());
    let Place::InLane { next, .. } = place(&facts, now()) else {
        panic!("a voided challenge held a ticket in its phases");
    };
    assert_eq!(next, name("review"));
    // It still waits on what else holds.
    facts.open_questions.insert(id(QUESTION));
    assert_eq!(
        place(&facts, now()),
        Place::Waiting([WaitingOn::Question(id(QUESTION))].into())
    );
}

#[test]
fn unsigned_epic_stops_every_ticket() {
    let mut facts = between_phases();
    for voided in [false, true] {
        facts.epic = Some(EpicFacts {
            id: id(EPIC),
            signed: false,
            challenged: true,
            voided,
        });
        let Place::Waiting(waits) = place(&facts, now()) else {
            panic!("an unsigned epic let a ticket between phases go on (voided {voided})");
        };
        assert!(
            waits.contains(&WaitingOn::Epic(id(EPIC))),
            "voided {voided}"
        );
    }
}
