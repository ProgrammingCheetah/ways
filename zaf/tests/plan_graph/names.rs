//! The names step on the fixtures: the vocabulary gives the lists (25), an unknown name and a
//! planner-written list are refused (24, 26), and a re-plan's done ticket leaves (27).
//!
//! The reading of `Blocked by` as needs (21) is another task's.

use std::collections::BTreeSet;

use zaf::domain::plan::names::{check_names, normalise};
use zaf::domain::plan::{Name, Node, PlanInput, Refusal, RefusalCode};

use super::{Outcome, fixtures};

/// The fixture whose file name starts with this number.
fn fixture(number: &str) -> super::Fixture {
    fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(&format!("{number}-")))
        .unwrap_or_else(|| panic!("fixture {number} is in the folder"))
        .1
}

/// What the reference does before the refusal pass: normalise, then check the names.
fn read(plan: &PlanInput) -> Result<PlanInput, Vec<Refusal>> {
    let plan = normalise(plan)?;
    check_names(&plan)?;
    Ok(plan)
}

fn set(names: &[Name]) -> BTreeSet<Name> {
    names.iter().cloned().collect()
}

/// A node's id with the names it provides and needs across its behaviours.
type Lists = (String, BTreeSet<Name>, BTreeSet<Name>);

fn lists(nodes: &[Node]) -> Vec<Lists> {
    nodes
        .iter()
        .map(|n| {
            let provides = n.behaviours.iter().flat_map(|b| b.provides.clone());
            let needs = n.behaviours.iter().flat_map(|b| b.needs.clone());
            (n.id.to_string(), provides.collect(), needs.collect())
        })
        .collect()
}

#[test]
fn fixture_21() {
    let fixture = fixture("21");
    let Outcome::Plan { plan: expected, .. } = fixture.expect.outcome() else {
        panic!("fixture 21 expects a plan");
    };
    let out = read(&fixture.plan).expect("fixture 21 reads");
    assert_eq!(lists(&out.nodes), lists(&expected.nodes));
    // The expected plan writes neither list; the reference's are what the reading makes.
    let outside: BTreeSet<&str> = out.outside.keys().map(Name::as_str).collect();
    assert_eq!(outside, BTreeSet::from(["#10", "#11"]));
    assert!(out.outside.iter().all(|(k, v)| k.as_str() == v.as_str()));
    let made: BTreeSet<Name> = out
        .nodes
        .iter()
        .flat_map(|n| &n.behaviours)
        .flat_map(|b| b.provides.iter().chain(&b.needs))
        .cloned()
        .collect();
    assert_eq!(out.no_signature, Some(made));
}

#[test]
fn fixture_24() {
    let fixture = fixture("24");
    assert_eq!(
        fixture.expect.outcome(),
        Outcome::Refused(RefusalCode::UnknownName)
    );
    let refused = read(&fixture.plan).expect_err("an unknown name is refused");
    assert_eq!(refused[0].code, RefusalCode::UnknownName);
    assert_eq!(refused[0].detail, "D needs mcp.catalgo");
    assert_eq!(refused.len(), 1, "{refused:?}");
}

#[test]
fn fixture_25() {
    let fixture = fixture("25");
    let Outcome::Plan { plan: expected, .. } = fixture.expect.outcome() else {
        panic!("fixture 25 expects a plan");
    };
    let out = read(&fixture.plan).expect("fixture 25 reads");
    let expected_no_signature = set(expected.no_signature.as_deref().expect("it shows the list"));
    let expected_existing = set(expected.existing.as_deref().expect("it shows the list"));
    assert_eq!(out.no_signature, Some(expected_no_signature));
    assert_eq!(out.existing, Some(expected_existing));
    assert_eq!((out.limit.0, out.lanes, out.record), (2, 4, 1));
}

#[test]
fn fixture_26() {
    let fixture = fixture("26");
    assert_eq!(
        fixture.expect.outcome(),
        Outcome::Refused(RefusalCode::PlannerList)
    );
    let refused = normalise(&fixture.plan).expect_err("a planner-written list is refused");
    assert_eq!(refused[0].code, RefusalCode::PlannerList);
}

#[test]
fn fixture_27() {
    let fixture = fixture("27");
    let out = read(&fixture.plan).expect("fixture 27 reads");
    let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["A", "C", "E", "I@1004.1", "X", "Y2"], "B is done");
    assert_eq!(out.done_names, set(&[Name::from("d")]));
    assert_eq!(out.existing, Some(set(&[Name::from("d")])));
    assert_eq!(out.record, 2);
}

#[test]
fn every_other_fixture_reads_to_the_same_lists_the_reference_gives() {
    for (file, fixture) in fixtures() {
        let Outcome::Plan { plan: expected, .. } = fixture.expect.outcome() else {
            continue;
        };
        let out = read(&fixture.plan).unwrap_or_else(|r| panic!("{file} reads: {r:?}"));
        if let Some(list) = &expected.existing {
            assert_eq!(out.existing, Some(set(list)), "{file} existing");
        }
        if let Some(list) = &expected.no_signature {
            assert_eq!(out.no_signature, Some(set(list)), "{file} no_signature");
        }
    }
}
