use std::collections::BTreeSet;

use super::{check_names, normalise};
use crate::domain::plan::{
    Area, Behaviour, Filed, Name, NewNames, Node, NodeId, NodeKind, PlanInput, RefusalCode,
    Vocabulary,
};

fn name(text: &str) -> Name {
    Name::from(text)
}

fn names(texts: &[&str]) -> BTreeSet<Name> {
    texts.iter().map(|t| name(t)).collect()
}

fn areas(texts: &[&str]) -> BTreeSet<Area> {
    texts.iter().map(|t| Area::from(*t)).collect()
}

fn node(id: &str, writes: &[&str], provides: &[&str], needs: &[&str]) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours: vec![Behaviour {
            text: "a behaviour".to_owned(),
            size: Default::default(),
            writes: areas(writes),
            provides: names(provides),
            needs: names(needs),
        }],
    }
}

fn filed(mut node: Node, filed: Filed) -> Node {
    node.filed = Some(filed);
    node
}

fn vocabulary() -> Vocabulary {
    Vocabulary {
        areas: areas(&["a", "b"]),
        signature: names(&["sig"]),
        no_signature: names(&["doc"]),
        existing: names(&["cli"]),
        epic: Default::default(),
    }
}

fn with_vocabulary(nodes: Vec<Node>) -> PlanInput {
    let mut plan = PlanInput::of(nodes);
    plan.vocabulary = Some(vocabulary());
    plan
}

fn codes<T>(result: Result<T, Vec<crate::domain::plan::Refusal>>) -> Vec<(RefusalCode, String)> {
    match result {
        Ok(_) => Vec::new(),
        Err(all) => all.into_iter().map(|r| (r.code, r.detail)).collect(),
    }
}

#[test]
fn a_plan_with_no_vocabulary_keeps_its_own_lists_and_gets_the_defaults() {
    let mut plan = PlanInput::of(vec![node("A", &[], &[], &[])]);
    plan.no_signature = Some(names(&["x"]));
    let out = normalise(&plan).expect("normalises");
    assert_eq!((out.limit.0, out.lanes, out.record), (2, 4, 1));
    assert_eq!(out.no_signature, Some(names(&["x"])));
    assert_eq!(out.existing, Some(BTreeSet::new()));
    assert!(out.done_names.is_empty());
    assert!(check_names(&out).is_ok(), "no vocabulary, nothing to check");
}

#[test]
fn the_vocabulary_gives_the_lists_and_the_plan_adds_no_signature_names() {
    let mut plan = with_vocabulary(vec![node("A", &[], &[], &[])]);
    plan.new_names = Some(NewNames {
        no_signature: names(&["docs.new"]),
        signature: names(&["sig.new"]),
        ..NewNames::default()
    });
    let out = normalise(&plan).expect("normalises");
    assert_eq!(out.no_signature, Some(names(&["doc", "docs.new"])));
    assert_eq!(out.existing, Some(names(&["cli"])));
}

#[test]
fn a_plan_that_writes_a_list_with_a_vocabulary_is_a_planner_list() {
    let mut plan = with_vocabulary(vec![node("A", &[], &[], &[])]);
    plan.no_signature = Some(BTreeSet::new());
    plan.existing = Some(names(&["ghost"]));
    let refused = codes(normalise(&plan));
    assert_eq!(refused.len(), 2, "each list is named: {refused:?}");
    assert!(refused.iter().all(|(c, _)| *c == RefusalCode::PlannerList));
    assert!(refused[0].1.contains("no_signature"));
    assert!(refused[1].1.contains("existing"));
}

#[test]
fn a_plan_that_adds_another_kind_of_name_is_a_planner_list() {
    let mut plan = with_vocabulary(vec![node("A", &[], &[], &[])]);
    let mut new = NewNames::default();
    new.other.insert("existing".to_owned(), names(&["ghost"]));
    new.other.insert("another".to_owned(), names(&["x"]));
    plan.new_names = Some(new);
    let refused = codes(normalise(&plan));
    assert_eq!(
        refused,
        [
            (
                RefusalCode::PlannerList,
                "a plan may not add another names".to_owned()
            ),
            (
                RefusalCode::PlannerList,
                "a plan may not add existing names".to_owned()
            ),
        ]
    );
}

#[test]
fn a_done_ticket_leaves_the_plan_and_its_names_become_existing() {
    let plan = PlanInput::of(vec![
        node("C", &[], &[], &["d"]),
        filed(node("B", &["store"], &["d"], &[]), Filed::Done),
        filed(node("A", &["cli"], &["s"], &[]), Filed::Open),
    ]);
    let out = normalise(&plan).expect("normalises");
    let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["A", "C"]);
    assert_eq!(out.nodes[0].filed, Some(Filed::Open), "an open one stays");
    assert_eq!(out.done_names, names(&["d"]));
    assert_eq!(out.existing, Some(names(&["d"])));
}

#[test]
fn the_done_names_join_the_vocabulary_s_existing() {
    let plan = with_vocabulary(vec![filed(node("B", &[], &["d"], &[]), Filed::Done)]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(out.existing, Some(names(&["cli", "d"])));
}

#[test]
fn nodes_come_out_in_natural_id_order() {
    let plan = PlanInput::of(vec![
        node("A10", &[], &[], &[]),
        node("A2", &[], &[], &[]),
        node("A1", &[], &[], &[]),
    ]);
    let out = normalise(&plan).expect("normalises");
    let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["A1", "A2", "A10"]);
}

#[test]
fn an_unknown_name_is_refused_naming_the_node_and_the_name() {
    let plan = with_vocabulary(vec![
        node("C", &["a"], &["sig"], &["cli"]),
        node("D", &["b"], &[], &["sg"]),
    ]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(
        codes(check_names(&out)),
        [(RefusalCode::UnknownName, "D needs sg".to_owned())]
    );
}

#[test]
fn every_unknown_name_is_named_not_only_the_first() {
    let plan = with_vocabulary(vec![
        node("A", &["nowhere"], &["made-up"], &["cli"]),
        node("B", &["a"], &[], &["ghost"]),
    ]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(
        codes(check_names(&out)),
        [
            (RefusalCode::UnknownName, "A writes nowhere".to_owned()),
            (RefusalCode::UnknownName, "A provides made-up".to_owned()),
            (RefusalCode::UnknownName, "B needs ghost".to_owned()),
        ]
    );
}

#[test]
fn a_name_is_known_from_the_plan_s_new_names_outside_existing_or_an_open_ticket() {
    let mut plan = with_vocabulary(vec![
        node(
            "A",
            &["a", "extra"],
            &["n.sig"],
            &["n.doc", "out", "d", "open"],
        ),
        filed(
            node("O", &["unchecked"], &["open"], &["whatever"]),
            Filed::Open,
        ),
        filed(node("B", &[], &["d"], &[]), Filed::Done),
    ]);
    plan.new_names = Some(NewNames {
        areas: areas(&["extra"]),
        signature: names(&["n.sig"]),
        no_signature: names(&["n.doc"]),
        ..NewNames::default()
    });
    plan.outside.insert(name("out"), NodeId::from("Z"));
    let out = normalise(&plan).expect("normalises");
    assert_eq!(codes(check_names(&out)), []);
}

#[test]
fn a_filed_ticket_s_own_names_are_not_checked() {
    let plan = with_vocabulary(vec![filed(
        node("O", &["nowhere"], &["ghost"], &["phantom"]),
        Filed::Open,
    )]);
    let out = normalise(&plan).expect("normalises");
    assert!(check_names(&out).is_ok());
}

#[test]
fn an_area_is_never_known_from_the_names() {
    let plan = with_vocabulary(vec![node("A", &["cli"], &[], &[])]);
    let out = normalise(&plan).expect("normalises");
    assert_eq!(
        codes(check_names(&out)),
        [(RefusalCode::UnknownName, "A writes cli".to_owned())]
    );
}
