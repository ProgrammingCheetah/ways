//! The plan's input from the other epics of its release (D13, Q10, Q19): fixtures 22, 23 and 29
//! with their `outside` and `shared` taken out, and the board's epics giving them back through
//! `app::plan::input::fill`.

use std::collections::BTreeSet;

use zaf::app::plan::input::fill;
use zaf::domain::plan::optimise::optimise;
use zaf::domain::plan::{KeptWait, Name, NodeId, PlanInput, RefusalCode, Vocabulary};
use zaf::domain::{Epic, NewTask, Ticket, TicketBody, TicketId, TicketState, TicketType};
use zaf::fakes::FakeBoard;
use zaf::ports::Board;

use super::body::ticket_of;
use super::{ExpectedPlan, Fixture, Outcome, fixtures};

const BODY: &str = "Release: 0.6.0\n\nThe epic.";

/// The fixture whose file name starts with `number-`.
fn fixture(number: &str) -> Fixture {
    fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with(&format!("{number}-")))
        .map(|(_, fixture)| fixture)
        .unwrap_or_else(|| panic!("fixture {number} is in the folder"))
}

fn expected(fixture: &Fixture) -> &ExpectedPlan {
    let Outcome::Plan { plan, .. } = fixture.expect.outcome() else {
        panic!("{} expects a plan", fixture.name);
    };
    plan
}

/// A board with these open epics of release 0.6.0.
fn board(epics: &[u64]) -> FakeBoard {
    let board = FakeBoard::default();
    for number in epics {
        board.add_epic(TicketId::from(*number), "an epic");
        board.set_body(TicketId::from(*number), BODY);
    }
    board
}

/// Files a planned ticket of `node`, with `kept` as its waits, under `epic`.
fn file(board: &FakeBoard, epic: u64, node: &str, plan: &ExpectedPlan, kept: &[KeptWait]) {
    let node = plan
        .nodes
        .iter()
        .find(|candidate| candidate.id == NodeId::from(node))
        .unwrap_or_else(|| panic!("the plan has node {node}"));
    board
        .file_task(&NewTask {
            epic: TicketId::from(epic),
            title: format!("node {}", node.id).into(),
            body: TicketBody::from(ticket_of(node, kept).to_string()),
            state: TicketState::Ready,
            group: None,
            kind: TicketType::from("Task"),
        })
        .expect("the ticket files");
}

fn epics(board: &FakeBoard) -> Vec<Epic> {
    board.epics().expect("the epics read")
}

fn this(all: &[Epic], number: u64) -> Epic {
    all.iter()
        .find(|epic| epic.id == TicketId::from(number))
        .cloned()
        .expect("the epic is open")
}

/// The fixture's plan as a planner would write it: nothing about other epics.
fn bare(fixture: &Fixture) -> PlanInput {
    let mut input = fixture.plan.clone();
    input.outside.clear();
    input.shared.clear();
    input
}

fn names(list: &[&str]) -> BTreeSet<Name> {
    list.iter().map(|name| Name::from(*name)).collect()
}

fn signature_vocabulary() -> Vocabulary {
    Vocabulary {
        signature: names(&["mcp.catalog"]),
        ..Vocabulary::default()
    }
}

#[test]
fn plan_input_outside_resolves_interface_ticket() {
    let fixture = fixture("22");
    let plan = expected(&fixture);
    // Epic 1004's interface ticket, as fixture 29 files it.
    let owner = self::fixture("29");
    let epic_1004 = expected(&owner);
    let board = board(&[1004, 1005]);
    file(&board, 1004, "I@1004.1", epic_1004, &[]);

    let all = epics(&board);
    let mut input = bare(&fixture);
    fill(&mut input, &this(&all, 1005), &all, &signature_vocabulary()).expect("filled");

    assert_eq!(
        input.outside.get(&Name::from("mcp.catalog")),
        Some(&NodeId::from("I@1004.1"))
    );
    assert_eq!(input.outside, fixture.plan.outside);
    let got = optimise(&input, fixture.options).expect("the plan is taken");
    let kept: BTreeSet<_> = plan.kept.iter().cloned().collect();
    assert_eq!(got.kept, kept, "the wait stays on I@1004.1");
    assert_eq!(got.nodes, plan.nodes);
    let orders: BTreeSet<_> = plan.orders.iter().cloned().collect();
    assert_eq!(got.orders, orders);
}

#[test]
fn plan_input_refuses_providing_another_epics_name() {
    let fixture = fixture("23");
    let owner = self::fixture("29");
    let epic_1004 = expected(&owner);
    let board = board(&[1004, 1005]);
    file(&board, 1004, "I@1004.1", epic_1004, &[]);

    let all = epics(&board);
    let mut input = bare(&fixture);
    fill(&mut input, &this(&all, 1005), &all, &signature_vocabulary()).expect("filled");

    assert_eq!(input.outside, fixture.plan.outside);
    let refusals = optimise(&input, fixture.options).expect_err("two providers");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.code == RefusalCode::TwoProviders),
        "{refusals:?}"
    );
}

#[test]
fn plan_input_shared_lifts_a_name_others_need() {
    let fixture = fixture("29");
    let plan = expected(&fixture);
    let waiting = self::fixture("22");
    let waiting_plan = expected(&waiting);
    // Another epic's filed ticket waits on epic 1004 for the name; the vocabulary does not give it.
    let board = board(&[1004, 2000]);
    file(&board, 2000, "D", waiting_plan, &waiting_plan.kept);

    let all = epics(&board);
    let mut input = bare(&fixture);
    fill(&mut input, &this(&all, 1004), &all, &signature_vocabulary()).expect("filled");
    assert_eq!(input.shared, names(&["mcp.catalog"]));
    assert!(input.owed.is_empty(), "the vocabulary does not give it");

    // Given by the vocabulary and waited on, the plan owes it (D13's third guard).
    let mut owing = bare(&fixture);
    let mut given = signature_vocabulary();
    given
        .epic
        .insert(Name::from("mcp.catalog"), TicketId::from(1004));
    fill(&mut owing, &this(&all, 1004), &all, &given).expect("filled");
    assert_eq!(owing.owed, names(&["mcp.catalog"]));

    let got = optimise(&input, fixture.options).expect("the plan is taken");
    assert_eq!(got.nodes, plan.nodes, "the name is lifted into I@1004.1");
    let orders: BTreeSet<_> = plan.orders.iter().cloned().collect();
    assert_eq!(got.orders, orders);

    // The vocabulary alone gives it too, with no ticket elsewhere waiting.
    let quiet = self::board(&[1004, 2000]);
    let all = epics(&quiet);
    let mut input = bare(&fixture);
    let mut vocabulary = signature_vocabulary();
    vocabulary
        .epic
        .insert(Name::from("mcp.catalog"), TicketId::from(1004));
    fill(&mut input, &this(&all, 1004), &all, &vocabulary).expect("filled");
    assert_eq!(input.shared, names(&["mcp.catalog"]));
    assert!(input.owed.is_empty(), "no ticket elsewhere waits for it");

    // A name with no signature is not lifted, so it is not shared.
    let mut input = bare(&fixture);
    let mut vocabulary = Vocabulary {
        no_signature: names(&["mcp.catalog"]),
        ..Vocabulary::default()
    };
    vocabulary
        .epic
        .insert(Name::from("mcp.catalog"), TicketId::from(1004));
    fill(&mut input, &this(&all, 1004), &all, &vocabulary).expect("filled");
    assert!(input.shared.is_empty());
}

#[test]
fn plan_input_refuses_name_of_older_epic() {
    let fixture = fixture("22");
    // Epic 3000 was planned before 0.6.0: it has a task and no plan record.
    let board = board(&[1005, 3000]);
    board.add_ticket(Ticket {
        id: TicketId::from(3001),
        epic: Some(TicketId::from(3000)),
        title: "an old task".into(),
        body: "Goal: an old task.".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    let all = epics(&board);
    let mut vocabulary = signature_vocabulary();
    vocabulary
        .epic
        .insert(Name::from("mcp.catalog"), TicketId::from(3000));

    let mut input = bare(&fixture);
    let error = fill(&mut input, &this(&all, 1005), &all, &vocabulary).expect_err("refused");
    let said = error.to_string();
    assert!(said.contains("mcp.catalog"), "{said}");
    assert!(said.contains("#3000"), "{said}");
    assert!(said.contains("need that epic's task as `#<n>`"), "{said}");
    assert!(input.outside.is_empty(), "nothing is filled on a refusal");

    // The same name, given to an epic that has filed nothing, is a wait on the epic.
    let unfiled = self::board(&[1005, 3000]);
    let all = epics(&unfiled);
    let mut input = bare(&fixture);
    fill(&mut input, &this(&all, 1005), &all, &vocabulary).expect("filled");
    assert_eq!(
        input.outside.get(&Name::from("mcp.catalog")),
        Some(&NodeId::from("I@3000.1"))
    );
}
