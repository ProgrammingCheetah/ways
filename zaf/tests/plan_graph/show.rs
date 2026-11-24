//! `plan show` (D24): fixture 20's output is filed as an epic on the fake board, and the view
//! `show` gives and the text `plan show` prints are read against what the fixture expects.

use zaf::app::plan::show::{PlanView, ShowError, ViewKind, show, show_before};
use zaf::cli::plan::{render, render_json};
use zaf::domain::plan::Measures;
use zaf::domain::plan::record::PlanRecord;
use zaf::domain::{BodyHash, NewTask, TicketBody, TicketId, TicketState, TicketType};
use zaf::fakes::FakeBoard;
use zaf::ports::Board;

use super::read::{board, file, file_older, output};
use super::{ExpectedPlan, Outcome, fixtures};

/// Fixture 20's output filed as an epic, the view of it, and the measures the fixture expects.
fn fixture_20() -> (PlanView, Measures) {
    let (board, epic) = board();
    file(&board, epic, &output("20-"));
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with("20-"))
        .expect("fixture 20 is in the folder");
    let Outcome::Plan { after, .. } = fixture.expect.outcome() else {
        panic!("fixture 20 is refused");
    };
    (show(&board, epic).expect("the epic shows"), after.clone())
}

#[test]
fn plan_show_prints_fixture_20_critical_path() {
    let (view, _) = fixture_20();
    let text = render(&view);
    assert!(text.contains("critical 4: A → C → E\n"), "{text}");
    let layers: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("layer "))
        .collect();
    assert_eq!(
        layers,
        ["layer 1: A B D F", "layer 2: C G", "layer 3: E"],
        "{text}"
    );
}

#[test]
fn plan_show_prints_layers_and_measures() {
    let (view, after) = fixture_20();
    assert_eq!(
        view.measures, after,
        "the measures are the fixture's `after`"
    );
    let text = render(&view);
    let width: Vec<String> = after.width.iter().map(ToString::to_string).collect();
    assert!(text.contains(&format!("chain {}\n", after.chain)), "{text}");
    assert!(
        text.contains(&format!("width {}\n", width.join(" "))),
        "{text}"
    );
    assert!(text.contains(&format!("work {}\n", after.work)), "{text}");
    let sizes: Vec<usize> = view.layers.iter().map(Vec::len).collect();
    assert_eq!(sizes, after.width, "each layer is as wide as it says");
    assert_eq!(view.nodes.len(), after.nodes);
    assert!(
        view.nodes
            .iter()
            .all(|node| node.title.starts_with("node ")),
        "each node carries its ticket's title"
    );
}

#[test]
fn plan_show_refuses_an_epic_with_no_tickets() {
    let (board, epic) = board();
    let error = show(&board, epic).expect_err("an epic with no tickets");
    assert!(matches!(error, ShowError::NoTickets(_)), "{error}");
    let line = error.to_string();
    assert!(!line.contains('\n'), "{line}");
    assert!(line.contains(&epic.to_string()), "{line}");
    assert!(line.contains("zaf plan file"), "{line}");

    // A ticket with no `Node:` line is no planned ticket either.
    board
        .file_task(&NewTask {
            epic,
            title: "old".into(),
            body: TicketBody::from("Goal: older template".to_owned()),
            state: TicketState::Ready,
            group: None,
            kind: TicketType::from("Task"),
        })
        .expect("the ticket files");
    let error = show(&board, epic).expect_err("still no planned ticket");
    assert!(matches!(error, ShowError::NoTickets(_)), "{error}");

    let missing = show(&board, TicketId::from(9999));
    assert!(matches!(missing, Err(ShowError::NotAnEpic(_))));
}

/// Fixture 12's output filed as an epic: what `plan show --json` prints of it, and the fixture.
fn json_12() -> (serde_json::Value, ExpectedPlan, Measures) {
    let (board, epic) = board();
    let plan = output("12-");
    file(&board, epic, &plan);
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with("12-"))
        .expect("fixture 12 is in the folder");
    let Outcome::Plan { after, .. } = fixture.expect.outcome() else {
        panic!("fixture 12 is refused");
    };
    let view = show(&board, epic).expect("the epic shows");
    let text = render_json(&view).to_string();
    (
        serde_json::from_str(&text).expect("the output parses as JSON"),
        plan,
        after.clone(),
    )
}

#[test]
fn plan_show_json_matches_fixture_12() {
    let (json, plan, after) = json_12();

    let mut nodes: Vec<(String, String)> = json["nodes"]
        .as_array()
        .expect("nodes is a list")
        .iter()
        .map(|node| {
            (
                node["id"].as_str().expect("an id").to_owned(),
                node["kind"].as_str().expect("a kind").to_owned(),
            )
        })
        .collect();
    nodes.sort();
    let mut expected: Vec<(String, String)> = plan
        .nodes
        .iter()
        .map(|node| {
            let kind = serde_json::to_value(node.kind).expect("a kind");
            (
                node.id.to_string(),
                kind.as_str().expect("a kind name").to_owned(),
            )
        })
        .collect();
    expected.sort();
    assert_eq!(nodes, expected, "the nodes are the fixture's");

    let mut orders = json["orders"].as_array().expect("orders is a list").clone();
    let mut want: Vec<serde_json::Value> = plan
        .orders
        .iter()
        .map(|order| serde_json::to_value(order).expect("an order"))
        .collect();
    orders.sort_by_key(ToString::to_string);
    want.sort_by_key(ToString::to_string);
    assert_eq!(orders, want, "the waits are the fixture's");

    assert_eq!(
        json["measures"],
        serde_json::to_value(&after).expect("measures"),
        "the measures are the fixture's `after`"
    );
    assert_eq!(json["kept"], serde_json::json!([]));
    assert!(json["layers"].is_array());
}

#[test]
fn plan_show_json_is_deterministic() {
    let (board, epic) = board();
    file(&board, epic, &output("12-"));
    let first = render_json(&show(&board, epic).expect("the epic shows")).to_string();
    let second = render_json(&show(&board, epic).expect("the epic shows")).to_string();
    assert_eq!(first, second, "the same epic prints the same bytes");
}

/// Fixture 20's input as a plan a planner writes: each node a section, its size `S` for 1 and
/// `M` for 2.
fn plan_20() -> (String, Measures) {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with("20-"))
        .expect("fixture 20 is in the folder");
    let Outcome::Plan { before, .. } = fixture.expect.outcome() else {
        panic!("fixture 20 is refused");
    };
    let list = |names: &std::collections::BTreeSet<zaf::domain::plan::Name>| {
        names
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let text = fixture
        .plan
        .nodes
        .iter()
        .map(|node| {
            let behaviour = &node.behaviours[0];
            let size = match behaviour.size.weight().expect("a weight").0 {
                1 => "S",
                2 => "M",
                other => panic!("size {other} has no letter"),
            };
            format!(
                "Task: node {id}\nNode: {id}\nGoal: g\nScope: s\nOut of scope: nothing\n\
                 Interfaces: none\nDesign pages: D2\nAcceptance:\n- `g` works\nTests: t\n\
                 Security trigger: no\nModel: sonnet\nSize: {size}\nWrites:\n\
                 Provides: {provides}\nNeeds: {needs}\n\n",
                id = node.id,
                provides = list(&behaviour.provides),
                needs = list(&behaviour.needs),
            )
        })
        .collect();
    (text, before.clone())
}

/// A plan record of `plan`, posted as a comment on `epic`.
fn post_record(board: &FakeBoard, epic: TicketId, k: u64, plan: String) {
    let record = PlanRecord {
        k,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: Vec::new(),
        plan,
    };
    board.comment(epic, &record.to_string()).expect("comment");
}

#[test]
fn plan_show_before_prints_the_records_nodes() {
    let (board, epic) = board();
    let (plan, before) = plan_20();
    post_record(&board, epic, 1, plan);
    file(&board, epic, &output("20-"));

    let view = show_before(&board, epic).expect("the epic shows as planned");
    assert_eq!(view.kind, ViewKind::Before);
    assert_eq!(
        view.measures, before,
        "the measures are the fixture's `before`"
    );
    let ids: Vec<&str> = view.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(ids, ["A", "B", "C", "D", "E", "F", "G"]);
    assert!(view.nodes.iter().all(|node| node.ticket.is_none()));

    let text = render(&view);
    assert!(text.starts_with("as planned\n"), "{text}");
    assert!(text.contains("critical 4: A → C → E\n"), "{text}");
    assert!(text.contains("layer 1: A B D F\n"), "{text}");
}

#[test]
fn plan_show_before_names_a_record_that_does_not_read() {
    let (board, epic) = board();
    post_record(&board, epic, 1, "not a plan\n".to_owned());
    let error = show_before(&board, epic).expect_err("the record does not read");
    assert!(matches!(error, ShowError::BadRecord { .. }), "{error}");
}

#[test]
fn plan_show_older_epic_prints_one_view() {
    let (board, epic) = board();
    let first = file_older(&board, epic, &[]);
    file_older(&board, epic, &[first]);
    for view in [
        show(&board, epic).expect("the epic shows"),
        show_before(&board, epic).expect("the epic shows with --before"),
    ] {
        assert_eq!(view.kind, ViewKind::Older);
        let text = render(&view);
        assert!(text.starts_with("planned before 0.6.0\n"), "{text}");
        assert!(text.contains("chain 2\n"), "{text}");
    }
}
