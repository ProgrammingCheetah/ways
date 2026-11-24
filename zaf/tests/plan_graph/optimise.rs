//! The optimiser on the fixtures: each plan that is no refusal gives its expected nodes, waits and
//! kept waits (ids included) and its `after` measures; the plan listed in another order gives the
//! same result, and an output run again gives itself.

use std::collections::BTreeSet;

use zaf::domain::plan::measure::measure;
use zaf::domain::plan::optimise::optimise;
use zaf::domain::plan::{Optimised, PlanInput};

use super::{ExpectedPlan, Fixture, Outcome, fixtures};

/// The fixture whose file name starts with `number-`.
fn fixture(number: &str) -> (String, Fixture) {
    fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(&format!("{number}-")))
        .unwrap_or_else(|| panic!("fixture {number} is in the folder"))
}

/// The optimiser's result, which must not be a refusal.
fn run(file: &str, fixture: &Fixture) -> Optimised {
    optimise(&fixture.plan, fixture.options)
        .unwrap_or_else(|refusals| panic!("{file} is refused: {refusals:?}"))
}

/// Checks a result against what the fixture expects of its plan.
fn assert_plan(file: &str, got: &Optimised, expected: &ExpectedPlan) {
    assert_eq!(got.nodes, expected.nodes, "{file}: the nodes");
    let orders: BTreeSet<_> = expected.orders.iter().cloned().collect();
    assert_eq!(got.orders, orders, "{file}: the waits");
    let kept: BTreeSet<_> = expected.kept.iter().cloned().collect();
    assert_eq!(got.kept, kept, "{file}: the kept waits");
}

#[test]
fn optimise_fixtures() {
    let mut checked = 0;
    for (file, fixture) in fixtures() {
        let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
            continue;
        };
        assert_plan(&file, &run(&file, &fixture), plan);
        checked += 1;
    }
    assert_eq!(checked, 21, "every fixture that is no refusal is checked");
}

#[test]
fn refused_fixtures_are_refused() {
    for (file, fixture) in fixtures() {
        let Outcome::Refused(code) = fixture.expect.outcome() else {
            continue;
        };
        let refusals =
            optimise(&fixture.plan, fixture.options).expect_err(&format!("{file} is refused"));
        assert_eq!(refusals[0].code, code, "{file}: {refusals:?}");
    }
}

#[test]
fn after_measures() {
    let mut checked = 0;
    for (file, fixture) in fixtures() {
        let Outcome::Plan { after, .. } = fixture.expect.outcome() else {
            continue;
        };
        let got = measure(&run(&file, &fixture), fixture.plan.lanes);
        assert_eq!(&got, after, "{file}: the measures after");
        checked += 1;
    }
    assert_eq!(checked, 21, "every fixture that is no refusal is checked");
}

#[test]
fn fixture_17() {
    let (file, fixture) = fixture("17");
    let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
        panic!("fixture 17 expects a plan");
    };
    let straight = run(&file, &fixture);
    assert_plan(&file, &straight, plan);

    let mut reversed = fixture.clone();
    reversed.plan.nodes.reverse();
    assert_ne!(reversed.plan.nodes, fixture.plan.nodes, "the order changed");
    let other = run(&file, &reversed);
    assert_eq!(other, straight, "listing order does not matter");
    assert_plan(&file, &other, plan);

    let mut rotated = fixture.clone();
    rotated.plan.nodes.rotate_left(1);
    assert_eq!(run(&file, &rotated), straight, "nor does a rotation");
}

#[test]
fn fixture_19() {
    let (file, fixture) = fixture("19");
    let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
        panic!("fixture 19 expects a plan");
    };
    let first = run(&file, &fixture);
    assert_plan(&file, &first, plan);

    // The output as the next run's input: its nodes and the plan's lists, no waits (D9).
    let again = PlanInput {
        nodes: first.nodes.clone(),
        ..fixture.plan.clone()
    };
    let second = optimise(&again, fixture.options).expect("the output is a plan");
    assert_eq!(second, first, "a rerun keeps the ids and the waits");
}
