use super::*;

fn ticket() -> Ticket {
    Ticket {
        id: TicketId::from(7),
        epic: Some(TicketId::from(2)),
        title: "export the list".into(),
        body: "Goal: export the list".into(),
        state: Some(TicketState::Ready),
        url: "https://github.com/example/tracker/issues/7".into(),
        kind: Some("Task".into()),
    }
}

#[test]
fn a_summary_prints_ids_as_numbers_and_the_state_as_its_word() {
    let printed = serde_json::to_value(ticket().summary()).expect("a summary serializes");
    assert_eq!(
        printed,
        serde_json::json!({
            "number": 7,
            "epic": 2,
            "title": "export the list",
            "state": "ready",
            "url": "https://github.com/example/tracker/issues/7",
            "type": "Task",
        })
    );
}

#[test]
fn the_summary_carries_the_type() {
    let typed = serde_json::to_value(ticket().summary()).expect("a summary serializes");
    assert_eq!(typed["type"], "Task");
    let untyped = Ticket {
        kind: None,
        ..ticket()
    };
    let printed = serde_json::to_value(untyped.summary()).expect("a summary serializes");
    assert_eq!(printed["type"], serde_json::Value::Null);
}

#[test]
fn a_summary_without_epic_or_state_prints_nulls() {
    let bare = Ticket {
        epic: None,
        state: None,
        ..ticket()
    };
    let printed = serde_json::to_value(bare.summary()).expect("a summary serializes");
    assert_eq!(printed["epic"], serde_json::Value::Null);
    assert_eq!(printed["state"], serde_json::Value::Null);
}

#[test]
fn an_issue_stands_by_done_then_open_then_how_it_closed() {
    assert_eq!(Standing::of_issue(true, false, false), Standing::Open);
    assert_eq!(Standing::of_issue(true, false, true), Standing::Done);
    assert_eq!(Standing::of_issue(false, false, false), Standing::Done);
    assert_eq!(
        Standing::of_issue(false, true, false),
        Standing::ClosedNotPlanned
    );
    assert_eq!(
        Standing::of_issue(false, true, true),
        Standing::Done,
        "done wins over a drop"
    );
}
