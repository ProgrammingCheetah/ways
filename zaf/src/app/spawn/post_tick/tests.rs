use serde_json::{Value, json};

use super::*;
use crate::app::actions::ActionName;
use crate::app::spawn::outcome::{Launched, Ran, Started};
use crate::domain::TicketId;

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

/// What the hook is given on stdin for `outcome`.
fn stdin_of(outcome: &TickOutcome) -> Value {
    let mut given = Vec::new();
    run(
        outcome,
        &mut |text| {
            given.push(text.to_owned());
            Ok(())
        },
        &mut |_| {},
    );
    assert_eq!(given.len(), 1, "the hook ran once");
    assert!(given[0].ends_with('\n'), "one line: {:?}", given[0]);
    serde_json::from_str(&given[0]).expect("JSON")
}

#[test]
fn post_tick_gets_the_started_phase() {
    let outcome = TickOutcome {
        started: Some(Started {
            task: TicketId::from(445),
            action: name("implement"),
            units: 2,
        }),
        ..TickOutcome::default()
    };

    let given = stdin_of(&outcome);

    assert_eq!(
        given,
        json!({
            "started": {"task": 445, "action": "implement", "units": 2},
            "waits": [],
            "ran": [],
            "questions": [],
            "idle": false,
        })
    );
}

#[test]
fn post_tick_lists_the_waits() {
    let outcome = TickOutcome {
        waits: vec![
            Launched {
                task: TicketId::from(7),
                action: name("review"),
            },
            Launched {
                task: TicketId::from(9),
                action: name("ci"),
            },
        ],
        ran: vec![Ran {
            task: TicketId::from(3),
            action: name("merge"),
            exit: 1,
        }],
        questions: vec![TicketId::from(11)],
        ..TickOutcome::default()
    };

    let given = stdin_of(&outcome);

    assert_eq!(
        given["waits"],
        json!([
            {"task": 7, "action": "review"},
            {"task": 9, "action": "ci"},
        ])
    );
    assert_eq!(
        given["ran"],
        json!([{"task": 3, "action": "merge", "exit": 1}])
    );
    assert_eq!(given["questions"], json!([11]));
    assert_eq!(given["started"], Value::Null);
    assert_eq!(given["idle"], json!(false));
}

#[test]
fn an_idle_tick_says_idle() {
    let given = stdin_of(&TickOutcome::default());

    assert_eq!(
        given,
        json!({
            "started": null,
            "waits": [],
            "ran": [],
            "questions": [],
            "idle": true,
        })
    );
}

#[test]
fn a_failing_post_tick_changes_nothing() {
    let outcome = TickOutcome {
        started: Some(Started {
            task: TicketId::from(445),
            action: name("implement"),
            units: 2,
        }),
        ..TickOutcome::default()
    };
    let before = outcome.clone();
    let mut logged = Vec::new();

    // `run` gives nothing back for a tick to act on: a failure can only be logged.
    let () = run(&outcome, &mut |_| Err("exit 1".to_owned()), &mut |line| {
        logged.push(line.to_owned())
    });

    assert_eq!(outcome, before, "the outcome is as it was");
    assert_eq!(logged, ["the post-tick hook failed: exit 1"]);
}
