//! The plan graph's fixtures: 29 small plans, each with its options and its expected result (the
//! rewritten plan and its measures, or the refusal). The files in `tests/data/plan-graph/` are a
//! copy of `.teams/releases/0.6.0/plan-graph/fixtures/`; the Rust optimiser must pass them all.
//!
//! This file holds the loader. The checks live in the modules beside it: `names`, `refuse`,
//! `optimise`, `measure` and `properties`, each filled by its own task.

use std::fs;
use std::path::{Path, PathBuf};

use zaf::domain::plan::{
    KeptWait, Measures, Name, Node, NodeId, Options, Order, PlanInput, RefusalCode,
};

mod before;
mod body;
mod input;
mod measure;
mod names;
mod optimise;
mod properties;
mod read;
mod refuse;
mod shape;
mod show;
mod text;
mod tickets;
mod verify;

/// The folder the fixtures are in.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/plan-graph")
}

/// The plan a fixture expects back: the lists the optimiser carries through, then its nodes and
/// waits. Each list is read when the fixture writes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPlan {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub orders: Vec<Order>,
    #[serde(default)]
    pub kept: Vec<KeptWait>,
    pub limit: Option<u32>,
    pub lanes: Option<u32>,
    pub record: Option<u32>,
    pub shared: Option<Vec<Name>>,
    pub no_signature: Option<Vec<Name>>,
    pub existing: Option<Vec<Name>>,
    pub outside: Option<std::collections::BTreeMap<Name, NodeId>>,
}

/// What a fixture expects: the plan refused with a code, or an expected plan and its measures
/// before and after. A fixture has one of the two, which [`Expect::outcome`] tells.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub refused: Option<RefusalCode>,
    pub before: Option<Measures>,
    pub after: Option<Measures>,
    pub plan: Option<ExpectedPlan>,
}

/// The two things a fixture can expect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<'a> {
    Refused(RefusalCode),
    Plan {
        before: &'a Measures,
        after: &'a Measures,
        plan: &'a ExpectedPlan,
    },
}

impl Expect {
    /// The expectation as one of its two kinds.
    ///
    /// # Panics
    /// When the fixture is both, or neither, or a plan without its measures.
    pub fn outcome(&self) -> Outcome<'_> {
        match (&self.refused, &self.before, &self.after, &self.plan) {
            (Some(code), None, None, None) => Outcome::Refused(*code),
            (None, Some(before), Some(after), Some(plan)) => Outcome::Plan {
                before,
                after,
                plan,
            },
            _ => panic!("a fixture expects a refusal, or a plan with both its measures"),
        }
    }
}

/// A fixture file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    pub why: String,
    #[serde(default)]
    pub options: Options,
    pub plan: PlanInput,
    pub expect: Expect,
}

/// Every fixture, by file name.
pub fn fixtures() -> Vec<(String, Fixture)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("the fixtures folder reads")
        .map(|entry| entry.expect("an entry reads").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).expect("a fixture reads");
            let fixture: Fixture = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{} does not load: {error}", path.display()));
            let file = path.file_name().expect("a file name").to_string_lossy();
            (file.into_owned(), fixture)
        })
        .collect()
}

#[test]
fn plan_graph_fixtures_load() {
    let all = fixtures();
    assert_eq!(all.len(), 29, "the folder holds 29 fixtures");
    for (file, fixture) in &all {
        assert_eq!(
            format!("{}.json", fixture.name),
            *file,
            "a fixture's name is its file's"
        );
        assert!(!fixture.why.is_empty(), "{file} says why");
        assert!(!fixture.plan.nodes.is_empty(), "{file} has nodes");
        // The optimiser takes every plan the fixtures hold, and the stand-in does not refuse.
        let _ = zaf::domain::plan::optimise::optimise(&fixture.plan, fixture.options);
    }
    let refused = all
        .iter()
        .filter(|(_, f)| matches!(f.expect.outcome(), Outcome::Refused(_)))
        .count();
    assert_eq!(refused, 8, "eight fixtures expect a refusal");
}
