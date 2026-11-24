//! The reading of `Blocked by` lines as needs (D4, D13, Q11).

use std::collections::BTreeSet;

use super::normalise;
use crate::domain::plan::{Behaviour, FiledWith, Name, Node, NodeId, NodeKey, NodeKind, PlanInput};

fn name(text: &str) -> Name {
    Name::from(text)
}

fn names(texts: &[&str]) -> BTreeSet<Name> {
    texts.iter().map(|t| name(t)).collect()
}

fn behaviour(text: &str) -> Behaviour {
    Behaviour {
        text: text.to_owned(),
        size: Default::default(),
        writes: BTreeSet::new(),
        provides: BTreeSet::new(),
        needs: BTreeSet::new(),
    }
}

fn node(id: &str, blocked_by: &[&str]) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: blocked_by.iter().map(|k| NodeKey::from(*k)).collect(),
        epic: None,
        behaviours: vec![behaviour("first"), behaviour("second")],
    }
}

fn filed_with_blocked_by(nodes: Vec<Node>) -> PlanInput {
    let mut plan = PlanInput::of(nodes);
    plan.filed_with = Some(FiledWith::BlockedBy);
    plan
}

fn only<'a>(plan: &'a PlanInput, id: &str) -> &'a Node {
    plan.nodes
        .iter()
        .find(|n| n.id.as_str() == id)
        .expect("the node is there")
}

#[test]
fn each_task_provides_its_own_name_and_each_line_is_a_need() {
    let plan = filed_with_blocked_by(vec![node("20", &[]), node("21", &["20"])]);
    let out = normalise(&plan).expect("normalises");
    let behaviours = &only(&out, "21").behaviours;
    assert_eq!(behaviours[0].provides, names(&["#21"]));
    assert_eq!(behaviours[0].needs, names(&["#20"]));
    assert!(behaviours[1].provides.is_empty() && behaviours[1].needs.is_empty());
    assert_eq!(only(&out, "20").behaviours[0].provides, names(&["#20"]));
    assert_eq!(out.no_signature, Some(names(&["#20", "#21"])));
    assert!(out.outside.is_empty());
}

#[test]
fn a_line_naming_an_epic_needs_each_of_its_tasks_and_they_are_outside() {
    let mut plan = filed_with_blocked_by(vec![node("22", &["E3"])]);
    plan.epics.insert(
        NodeKey::from("E3"),
        vec![NodeId::from("10"), NodeId::from("11")],
    );
    let out = normalise(&plan).expect("normalises");
    assert_eq!(only(&out, "22").behaviours[0].needs, names(&["#10", "#11"]));
    let outside: Vec<(&str, &str)> = out
        .outside
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(outside, [("#10", "#10"), ("#11", "#11")]);
    assert_eq!(out.no_signature, Some(names(&["#10", "#11", "#22"])));
}

#[test]
fn a_line_naming_an_epic_with_no_task_stays_a_need_of_that_epic_s_own_name() {
    let mut plan = filed_with_blocked_by(vec![node("22", &["E9", "E4"])]);
    plan.epics.insert(NodeKey::from("E4"), Vec::new());
    let out = normalise(&plan).expect("normalises");
    assert_eq!(only(&out, "22").behaviours[0].needs, names(&["#E4", "#E9"]));
    assert_eq!(out.outside.get(&name("#E9")), Some(&NodeId::from("#E9")));
}

#[test]
fn a_task_never_needs_itself_even_through_its_epic() {
    let mut plan = filed_with_blocked_by(vec![node("A", &["A", "E"])]);
    plan.epics.insert(
        NodeKey::from("E"),
        vec![NodeId::from("A"), NodeId::from("B")],
    );
    let out = normalise(&plan).expect("normalises");
    assert_eq!(only(&out, "A").behaviours[0].needs, names(&["#B"]));
}

#[test]
fn a_name_a_task_of_the_plan_provides_is_not_outside() {
    let plan = filed_with_blocked_by(vec![node("A", &[]), node("B", &["A", "Z"])]);
    let out = normalise(&plan).expect("normalises");
    let outside: Vec<&str> = out.outside.keys().map(Name::as_str).collect();
    assert_eq!(outside, ["#Z"]);
}

#[test]
fn the_names_join_the_no_signature_list_already_there() {
    let mut plan = filed_with_blocked_by(vec![node("A", &["Z"])]);
    plan.no_signature = Some(names(&["x"]));
    let out = normalise(&plan).expect("normalises");
    assert_eq!(out.no_signature, Some(names(&["x", "#A", "#Z"])));
}

#[test]
fn a_plan_with_no_filed_with_is_left_unchanged() {
    let mut plan = PlanInput::of(vec![node("A", &["B"]), node("B", &[])]);
    plan.epics
        .insert(NodeKey::from("E"), vec![NodeId::from("A")]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(out.nodes, plan.nodes, "no need is read, no line is dropped");
    assert!(out.outside.is_empty());
    assert_eq!(out.epics, plan.epics);
    assert_eq!(out.no_signature, Some(BTreeSet::new()));
}

#[test]
fn the_reading_takes_its_marks_out_so_a_second_pass_changes_nothing() {
    let plan = filed_with_blocked_by(vec![node("A", &["B"])]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(out.filed_with, None);
    assert!(only(&out, "A").blocked_by.is_empty());
    assert!(out.epics.is_empty());
    assert_eq!(normalise(&out).expect("again"), out);
}
