use super::*;
use crate::domain::plan::{Behaviour, Size, Weight};

fn behaviour(size: u32, provides: &[&str], needs: &[&str]) -> Behaviour {
    Behaviour {
        text: "does a thing".into(),
        size: Size::Weight(Weight(size)),
        writes: BTreeSet::new(),
        provides: provides.iter().map(|n| Name::from(*n)).collect(),
        needs: needs.iter().map(|n| Name::from(*n)).collect(),
    }
}

fn node(id: &str, behaviours: Vec<Behaviour>) -> Node {
    Node {
        id: NodeId::from(id),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours,
    }
}

fn codes(plan: &PlanInput) -> Vec<RefusalCode> {
    refuse(plan)
        .expect_err("refused")
        .iter()
        .map(|r| r.code)
        .collect()
}

#[test]
fn a_whole_plan_is_let_through() {
    let plan = PlanInput::of(vec![
        node("A", vec![behaviour(1, &["a"], &[])]),
        node("B", vec![behaviour(2, &[], &["a"])]),
    ]);
    assert_eq!(refuse(&plan), Ok(()));
}

#[test]
fn a_plan_with_two_problems_names_both_the_references_first_first() {
    let plan = PlanInput::of(vec![
        node("A", vec![behaviour(3, &[], &["ghost"])]),
        node("B", vec![]),
    ]);
    assert_eq!(
        codes(&plan),
        [
            RefusalCode::TooLarge,
            RefusalCode::NoBehaviour,
            RefusalCode::Unprovided
        ]
    );
}

#[test]
fn duplicate_ids_and_a_blank_text_are_named() {
    let mut blank = behaviour(1, &[], &[]);
    blank.text = "  ".into();
    let plan = PlanInput::of(vec![
        node("A", vec![blank]),
        node("A", vec![behaviour(1, &[], &[])]),
    ]);
    assert_eq!(
        codes(&plan),
        [RefusalCode::NoBehaviour, RefusalCode::DuplicateId]
    );
}

#[test]
fn a_merged_id_holds_a_behaviour_for_each_part() {
    let plan = PlanInput::of(vec![node("A+B", vec![behaviour(1, &[], &[])])]);
    assert_eq!(codes(&plan), [RefusalCode::BadId]);
}

#[test]
fn a_size_that_is_not_a_whole_number_of_at_least_one_is_bad() {
    let mut zero = behaviour(1, &[], &[]);
    zero.size = Size::Weight(Weight(0));
    let mut text = behaviour(1, &[], &[]);
    text.size = Size::Other(serde_json::json!("L"));
    let plan = PlanInput::of(vec![node("A", vec![zero]), node("B", vec![text])]);
    let refusals = refuse(&plan).expect_err("refused");
    assert_eq!(
        refusals
            .iter()
            .map(|r| r.detail.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert!(refusals.iter().all(|r| r.code == RefusalCode::BadSize));
}

#[test]
fn an_interface_node_has_no_limit() {
    let mut interface = node("I@1.1", vec![behaviour(5, &["s"], &[])]);
    interface.kind = NodeKind::Interface;
    assert_eq!(refuse(&PlanInput::of(vec![interface])), Ok(()));
}

#[test]
fn a_behaviour_that_needs_what_it_provides_is_named_with_the_name() {
    let plan = PlanInput::of(vec![node("A", vec![behaviour(1, &["x"], &["x"])])]);
    let refusals = refuse(&plan).expect_err("refused");
    assert_eq!(refusals[0].code, RefusalCode::NeedsItself);
    assert!(refusals[0].detail.contains('A') && refusals[0].detail.contains('x'));
}

#[test]
fn a_name_a_done_ticket_provided_has_two_providers() {
    let mut plan = PlanInput::of(vec![node("A", vec![behaviour(1, &["d"], &[])])]);
    plan.done_names.insert(Name::from("d"));
    assert_eq!(codes(&plan), [RefusalCode::TwoProviders]);
}

#[test]
fn a_name_in_the_existing_list_or_outside_is_provided() {
    let mut plan = PlanInput::of(vec![node("A", vec![behaviour(1, &[], &["e", "o"])])]);
    plan.existing = Some(BTreeSet::from([Name::from("e")]));
    plan.outside.insert(Name::from("o"), NodeId::from("#9"));
    assert_eq!(refuse(&plan), Ok(()));
}

#[test]
fn a_signature_name_from_an_open_filed_task_is_fixed_but_one_without_is_not() {
    let mut filed = node("P", vec![behaviour(1, &["q", "r"], &[])]);
    filed.filed = Some(Filed::Open);
    let mut plan = PlanInput::of(vec![filed, node("N", vec![behaviour(1, &[], &["q", "r"])])]);
    plan.no_signature = Some(BTreeSet::from([Name::from("r")]));
    let refusals = refuse(&plan).expect_err("refused");
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].code, RefusalCode::FixedProvider);
    assert!(refusals[0].detail.contains('q'));
}

#[test]
fn a_cycle_is_named_after_the_other_problems() {
    let plan = PlanInput::of(vec![
        node("A", vec![behaviour(1, &["a"], &["b"])]),
        node("B", vec![behaviour(1, &["b"], &["a", "ghost"])]),
    ]);
    assert_eq!(codes(&plan), [RefusalCode::Unprovided, RefusalCode::Cycle]);
}

/// D13's third guard: a name another epic's filed ticket waits on this epic for must be among
/// the plan's provides, or have been provided by a done ticket.
#[test]
fn a_name_another_epic_waits_for_and_the_plan_leaves_out_is_unprovided() {
    let mut plan = PlanInput::of(vec![node("A", vec![behaviour(1, &["a"], &[])])]);
    plan.owed = BTreeSet::from([Name::from("a"), Name::from("mcp.catalog")]);
    let refusals = refuse(&plan).expect_err("refused");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, RefusalCode::Unprovided);
    assert!(refusals[0].detail.contains("mcp.catalog"), "{refusals:?}");

    // A done ticket of the epic provided it: the name is code by now.
    plan.done_names.insert(Name::from("mcp.catalog"));
    assert_eq!(refuse(&plan), Ok(()));
}
