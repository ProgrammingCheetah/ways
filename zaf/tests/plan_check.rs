//! `zaf plan check <epic>` through the built binary on a fake board: the measures of a good plan
//! before and after the optimiser (the numbers are the fixtures' own), a cycle named, and a run
//! that leaves the board and the repository as they were.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::UNIX_EPOCH;

use common::{Project, outcome};
use serde_json::{Value, json};

const EPIC: &str = "1157";

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [vcs]\nkind = \"git\"\n";

const DESIGN: &str = "# Design\n\n```toml vocabulary\n\
    [areas]\n\"a\" = \"a\"\n\"b\" = \"b\"\n\"c\" = \"c\"\n\"app/update\" = \"update\"\n\
    \"prompts\" = \"prompts\"\n\"docs\" = \"docs\"\n\n\
    [names.\"a.api\"]\nsignature = \"fn a()\"\n\n\
    [names.\"b.api\"]\nsignature = \"fn b()\"\n```\n";

/// A project whose design of 0.6.0 is signed and whose board holds the epic.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let epic = json!([{
        "number": 1157,
        "title": "The vocabulary",
        "body": "Release: 0.6.0\n\nThe epic.",
        "labels": [{"name": "epic"}],
        "comments": [],
    }]);
    project
        .on_first("issue list --label epic *", &epic.to_string())
        .on("*", "[]");
    let design = zaf::domain::DocumentSignature::sign(DESIGN, UNIX_EPOCH);
    common::write(&project.root, ".teams/releases/0.6.0/design.md", &design);
    project
}

/// One section of a plan, as a planner writes it.
fn section(node: &str, size: &str, writes: &str, provides: &str, needs: &str) -> String {
    format!(
        "Task: Build {node}\nNode: {node}\nGoal: {node} works\nScope: the part\n\
         Out of scope: the rest\nInterfaces: none\nDesign pages: D2\nAcceptance:\n- it works\n\
         Tests: it_works\nSecurity trigger: no\nModel: sonnet\nSize: {size}\n\
         Writes: {writes}\nProvides: {provides}\nNeeds: {needs}\n\n"
    )
}

/// `zaf plan check 1157` with `plan` on stdin.
fn plan_check(project: &Project, args: &[&str], plan: &str) -> Output {
    let mut command = project.command_in(&project.root, args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("run zaf");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(plan.as_bytes())
        .expect("write the plan");
    child.wait_with_output().expect("zaf output")
}

fn measures(project: &Project, plan: &str) -> Value {
    let output = plan_check(project, &["plan", "check", EPIC, "--json"], plan);
    let (code, stdout, stderr) = outcome(&output);
    assert_eq!(code, 0, "{stdout}{stderr}");
    serde_json::from_str(&stdout).expect("JSON measures")
}

/// The three plans of fixtures 13, 02 and 05, as a planner writes them.
fn plans() -> [(&'static str, String); 3] {
    let chain = [
        section("A", "S", "a", "a.api", "none"),
        section("B", "S", "b", "b.api", "a.api"),
        section("C", "S", "c", "none", "b.api"),
    ]
    .concat();
    let conflict = [
        section("A", "S", "app/update", "none", "none"),
        section("B", "S", "app/update", "none", "none"),
    ]
    .concat();
    let no_signature = format!(
        "New names:\n- no signature prompt.fix\n\n{}",
        [
            section("A", "S", "prompts", "prompt.fix", "none"),
            section("B", "S", "docs", "none", "prompt.fix"),
        ]
        .concat()
    );
    [
        ("13-interfaces-in-one-node", chain),
        ("02-conflict-merges", conflict),
        ("05-no-signature-stays-an-edge", no_signature),
    ]
}

#[test]
fn plan_check_prints_measures_of_o4_plans() {
    let project = project();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/plan-graph");
    for (name, plan) in plans() {
        let fixture: Value = serde_json::from_str(
            &fs::read_to_string(fixtures.join(format!("{name}.json"))).unwrap(),
        )
        .expect("a fixture");
        let got = measures(&project, &plan);
        for side in ["before", "after"] {
            let want = &fixture["expect"][side];
            for key in ["nodes", "interfaces", "chain", "width", "work", "critical"] {
                assert_eq!(got[side][key], want[key], "{name} {side} {key}");
            }
        }
    }
    // The terse form, for the chain: one line before and one after.
    let (_, chain) = &plans()[0];
    let output = plan_check(&project, &["plan", "check", EPIC], chain);
    let (code, stdout, _) = outcome(&output);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "before: nodes 3, interfaces 0, chain 3, width 1 1 1, critical 3 (A B C)\n\
         after: nodes 4, interfaces 1, chain 2, width 1 3, critical 3 (I@1157.1 A)\n"
    );
}

#[test]
fn plan_check_refuses_a_cycle() {
    let project = project();
    let plan = [
        section("A", "S", "a", "a.api", "b.api"),
        section("B", "S", "b", "b.api", "a.api"),
    ]
    .concat();
    let output = plan_check(&project, &["plan", "check", EPIC], &plan);
    let (code, stdout, stderr) = outcome(&output);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.lines().any(|line| line.contains("cycle: A B")),
        "{stderr}"
    );
    let output = plan_check(&project, &["plan", "check", EPIC, "--json"], &plan);
    let (code, stdout, _) = outcome(&output);
    assert_eq!(code, 1);
    let refused: Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(refused["refused"][0]["kind"], "cycle");
}

#[test]
fn plan_check_names_every_refusal_one_line_each() {
    let project = project();
    let plan = [
        section("A", "S", "a", "none", "nobody.provides"),
        section("B", "S", "b", "none", "a.api"),
    ]
    .concat();
    let output = plan_check(&project, &["plan", "check", EPIC], &plan);
    let (code, _, stderr) = outcome(&output);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("unknown-name: A needs nobody.provides"),
        "{stderr}"
    );
    assert!(stderr.contains("a.api"), "{stderr}");
}

#[test]
fn plan_check_needs_an_epic_that_is_open() {
    let project = project();
    let (_, plan) = &plans()[1];
    let output = plan_check(&project, &["plan", "check", "9999"], plan);
    let (code, _, stderr) = outcome(&output);
    assert_eq!(code, 1);
    assert!(stderr.contains("is not an open epic"), "{stderr}");
    let output = project.zaf(&["plan", "check"]);
    assert_eq!(outcome(&output).0, 2, "the epic is not optional");
}

/// Every file under `dir` with its bytes, but the fake `gh`'s own scene.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(here) = stack.pop() {
        for entry in fs::read_dir(&here).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(dir).unwrap().display().to_string();
                files.insert(name, fs::read(&path).expect("read"));
            }
        }
    }
    files
}

#[test]
fn plan_check_writes_nothing() {
    let project = project();
    // The first run of any verb makes the project's `.zaf/`, as it does for every verb.
    assert_eq!(outcome(&project.zaf(&["asklog"])).0, 0);
    let before = tree(&project.root);
    for (_, plan) in plans() {
        assert_eq!(
            outcome(&plan_check(&project, &["plan", "check", EPIC], &plan)).0,
            0
        );
    }
    let cycle = [
        section("A", "S", "a", "a.api", "b.api"),
        section("B", "S", "b", "b.api", "a.api"),
    ]
    .concat();
    assert_eq!(
        outcome(&plan_check(&project, &["plan", "check", EPIC], &cycle)).0,
        1
    );
    assert_eq!(tree(&project.root), before, "the repository is as it was");
    let calls = project.gh_calls();
    assert!(!calls.is_empty(), "the board was read");
    for call in &calls {
        assert!(
            call.starts_with("issue list") || call.starts_with("api "),
            "a call that is no read: {call}"
        );
    }
}
