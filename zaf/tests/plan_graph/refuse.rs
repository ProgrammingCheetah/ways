//! The refusals on the fixtures: each refused plan gives its code first, naming the node and the
//! name, and a plan of two problems names both.

use zaf::domain::plan::names::{check_names, normalise};
use zaf::domain::plan::refuse::refuse;
use zaf::domain::plan::{Refusal, RefusalCode};

use super::{Outcome, fixtures};

/// What the reference does before the optimiser: normalise, then the refusal pass.
fn refused(number: &str) -> (RefusalCode, Vec<Refusal>) {
    let fixture = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(&format!("{number}-")))
        .unwrap_or_else(|| panic!("fixture {number} is in the folder"))
        .1;
    let Outcome::Refused(code) = fixture.expect.outcome() else {
        panic!("fixture {number} expects a refusal");
    };
    let plan = normalise(&fixture.plan).expect("the plan normalises");
    (code, refuse(&plan).expect_err("the plan is refused"))
}

fn first_is(number: &str, detail: &[&str]) {
    let (code, refusals) = refused(number);
    assert_eq!(refusals[0].code, code, "{refusals:?}");
    for part in detail {
        assert!(
            refusals[0].detail.contains(part),
            "{} names {part}",
            refusals[0]
        );
    }
}

#[test]
fn fixture_07() {
    let (code, refusals) = refused("07");
    assert_eq!(code, RefusalCode::Cycle);
    assert_eq!(refusals, [Refusal::new(RefusalCode::Cycle, "A B")]);
}

#[test]
fn fixture_08() {
    first_is("08", &["A", "ghost"]);
}

#[test]
fn fixture_09() {
    first_is("09", &["B", "x", "A"]);
}

#[test]
fn fixture_10() {
    first_is("10", &["A", "3"]);
}

#[test]
fn fixture_23() {
    first_is("23", &["A", "mcp.catalog", "I@1004.1"]);
}

#[test]
fn fixture_28() {
    first_is("28", &["N", "q", "P"]);
}

#[test]
fn every_whole_fixture_is_let_through() {
    for (file, fixture) in fixtures() {
        if matches!(fixture.expect.outcome(), Outcome::Refused(_)) {
            continue;
        }
        let plan = normalise(&fixture.plan).unwrap_or_else(|r| panic!("{file} reads: {r:?}"));
        check_names(&plan).unwrap_or_else(|r| panic!("{file} names: {r:?}"));
        refuse(&plan).unwrap_or_else(|r| panic!("{file} is whole: {r:?}"));
    }
}
