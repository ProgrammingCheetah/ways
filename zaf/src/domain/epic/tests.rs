use std::time::UNIX_EPOCH;

use super::*;

const BODY: &str = "Goal: one ticket shape over every backend.\n";

fn task(n: u64, state: TicketState, standing: Standing) -> EpicTask {
    EpicTask {
        number: TicketId::from(n),
        title: format!("task {n}").into(),
        body: "".into(),
        url: "".into(),
        state: Some(state),
        standing,
        group: None,
        kind: None,
        closed_at: None,
    }
}

fn epic() -> Epic {
    Epic {
        id: TicketId::from(1),
        title: "tickets".into(),
        body: BODY.into(),
        tasks: Vec::new(),
        signature: None,
        planned_at: None,
        records: Vec::new(),
        challengers: Vec::new(),
    }
}

fn signed(epic: Epic) -> Epic {
    Epic {
        signature: Some(Signature::of(&epic.body, UNIX_EPOCH)),
        ..epic
    }
}

#[test]
fn a_body_hash_is_sha256_in_hex() {
    let hash = BodyHash::of(&"".into());
    assert_eq!(
        hash.as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn line_endings_and_trailing_space_do_not_move_the_hash() {
    let plain = BodyHash::of(&"Goal: g\nSize: s".into());
    assert_eq!(BodyHash::of(&"Goal: g\r\nSize: s\r\n\n  ".into()), plain);
    assert_ne!(BodyHash::of(&"Goal: g\nSize: m".into()), plain);
}

#[test]
fn a_hash_reads_back_from_its_hex_only() {
    let hash = BodyHash::of(&BODY.into());
    assert_eq!(hash.as_str().parse::<BodyHash>(), Ok(hash.clone()));
    assert_eq!(hash.as_str().to_uppercase().parse::<BodyHash>(), Ok(hash));
    assert!("abc".parse::<BodyHash>().is_err());
    assert!("z".repeat(64).parse::<BodyHash>().is_err());
}

#[test]
fn an_epic_walks_its_stages_in_order() {
    let mut epic = epic();
    assert_eq!(epic.stage(), EpicStage::Unsigned);
    epic = signed(epic);
    assert_eq!(epic.stage(), EpicStage::Signed);
    epic.tasks = vec![
        task(2, TicketState::Ready, Standing::Open),
        task(3, TicketState::Blocked, Standing::Open),
    ];
    assert_eq!(epic.stage(), EpicStage::Planned);
    epic.challengers = vec![None];
    assert_eq!(epic.stage(), EpicStage::Challenged);
    epic.tasks = vec![
        task(2, TicketState::Done, Standing::Done),
        task(3, TicketState::Ready, Standing::ClosedNotPlanned),
    ];
    assert_eq!(epic.stage(), EpicStage::Done);
    assert_eq!(epic.tally(), (1, 1), "a dropped task counts for nothing");
}

fn challenged_with(tasks: Vec<EpicTask>) -> Epic {
    Epic {
        tasks,
        challengers: vec![None],
        ..signed(epic())
    }
}

#[test]
fn epic_all_done_is_done() {
    let epic = challenged_with(vec![
        task(2, TicketState::Done, Standing::Done),
        task(3, TicketState::Done, Standing::Done),
    ]);
    assert_eq!(epic.stage(), EpicStage::Done);
    assert!(epic.finished_done());
}

#[test]
fn epic_one_done_rest_dropped_is_done() {
    let epic = challenged_with(vec![
        task(2, TicketState::Done, Standing::Done),
        task(3, TicketState::Ready, Standing::ClosedNotPlanned),
    ]);
    assert_eq!(epic.stage(), EpicStage::Done);
    assert!(epic.finished_done());
}

#[test]
fn epic_all_dropped_is_not_done() {
    let epic = challenged_with(vec![
        task(2, TicketState::Ready, Standing::ClosedNotPlanned),
        task(3, TicketState::Ready, Standing::ClosedNotPlanned),
    ]);
    assert_eq!(epic.stage(), EpicStage::Challenged);
    assert!(!epic.finished_done());
}

#[test]
fn epic_one_open_is_not_done() {
    let epic = challenged_with(vec![
        task(2, TicketState::Done, Standing::Done),
        task(3, TicketState::Ready, Standing::Open),
    ]);
    assert_eq!(epic.stage(), EpicStage::Challenged);
    assert!(!epic.finished_done());
}

#[test]
fn an_edit_voids_the_signature() {
    let mut epic = signed(epic());
    assert!(epic.signed());
    epic.body = format!("{BODY}Also: Projects.").into();
    assert!(!epic.signed());
    assert!(epic.signature_void());
    assert_eq!(epic.stage(), EpicStage::Unsigned);
}

#[test]
fn release_line_with_escape_is_cleaned_or_refused() {
    assert_eq!(release_of("Release: 0.6.\x1b[2J0\n\nGoal."), None);
    assert_eq!(release_of("Release: 0.6.0\u{9b}\n"), None);
    assert_eq!(release_of("Release: 0.6.0\x07"), None);
    assert_eq!(release_of("Release: 0.6.0\r\nGoal."), Some("0.6.0"));
}

#[test]
fn epic_of_a_version_reads_the_first_line_only() {
    let of = |body: &str| Epic {
        body: body.into(),
        ..epic()
    };
    let first = of("Release: 0.6.0\n\nGoal: one.");
    let crlf = of("Release: 0.6.0\r\nGoal: one.");
    let second = of("Goal: one.\nRelease: 0.6.0");
    let none = of(BODY);
    assert_eq!(first.release(), Some("0.6.0"));
    assert_eq!(crlf.release(), Some("0.6.0"));
    assert_eq!(second.release(), None);
    assert_eq!(none.release(), None);
    assert_eq!(release_of("Release: "), None);
    assert_eq!(release_of(""), None);

    let epics = [first, second, none, crlf];
    assert_eq!(of_version(&epics, "0.6.0").count(), 2);
    assert_eq!(of_version(&epics, "0.5.0").count(), 0);
    assert_eq!(release_of(&with_release("0.6.0", "Goal.")), Some("0.6.0"));
}

#[test]
fn a_stage_prints_as_its_word() {
    assert_eq!(EpicStage::Challenged.to_string(), "challenged");
    assert_eq!(
        serde_json::to_value(EpicStage::Unsigned).expect("serializes"),
        "unsigned"
    );
}

fn record_of(k: u64, marked: bool) -> EpicRecord {
    use crate::domain::plan::record::{FilingMark, GraphHash, PlanRecord};
    EpicRecord {
        record: PlanRecord {
            k,
            design: BodyHash::of(&BODY.into()),
            dropped: Vec::new(),
            plan: format!("plan {k}\n"),
        },
        posted_at: None,
        mark: marked.then(|| FilingMark {
            k,
            graph: "ab".repeat(32).parse::<GraphHash>().expect("a digest"),
        }),
    }
}

fn with_records(records: Vec<EpicRecord>) -> Epic {
    Epic {
        tasks: vec![task(2, TicketState::Ready, Standing::Open)],
        records,
        ..signed(epic())
    }
}

#[test]
fn epic_last_record_without_mark_not_planned() {
    let epic = with_records(vec![record_of(1, true), record_of(2, false)]);
    assert_eq!(epic.stage(), EpicStage::Signed);
    assert!(epic.half_filed());
}

/// A graph ticket: node `node`, one section with `goal`.
fn graph_task(number: u64, node: &str, goal: &str) -> EpicTask {
    use crate::domain::plan::body::{PlannedTicket, Section};
    use crate::domain::plan::{NodeId, NodeKind};
    use crate::domain::{SecurityTrigger, TaskModel, TaskSize, TaskTemplate};
    use std::collections::BTreeSet;
    let body = PlannedTicket {
        node: NodeId::from(node),
        kind: NodeKind::Body,
        signatures: Vec::new(),
        turns: Vec::new(),
        waits: Vec::new(),
        sections: vec![Section {
            key: None,
            template: TaskTemplate {
                goal: goal.to_owned(),
                owned_paths: "a path".to_owned(),
                scoped: false,
                out_of_scope: None,
                interfaces: "none".to_owned(),
                design_pages: "D1".to_owned(),
                architecture: String::new(),
                acceptance: vec!["a line".to_owned()],
                tests: "a test".to_owned(),
                security_trigger: SecurityTrigger::No,
                model: TaskModel::Sonnet,
                size: TaskSize::S,
                blocked_by: Vec::new(),
                style: None,
            },
            writes: BTreeSet::new(),
            provides: BTreeSet::new(),
            implements: BTreeSet::new(),
            needs: BTreeSet::new(),
        }],
    }
    .to_string();
    EpicTask {
        body: body.into(),
        ..task(number, TicketState::Ready, Standing::Open)
    }
}

/// A graph epic: two graph tickets and a filed plan record, with no challenger yet.
fn graph_epic() -> Epic {
    Epic {
        tasks: vec![
            graph_task(2, "a", "the first goal"),
            graph_task(3, "b", "the second goal"),
        ],
        records: vec![record_of(1, true)],
        ..signed(epic())
    }
}

/// `epic` with a challenger report ending in the hash its graph has now.
fn challenged_now(epic: Epic) -> Epic {
    let hash = PlanGraph::read(&epic, &[]).hash();
    Epic {
        challengers: vec![Some(hash)],
        ..epic
    }
}

#[test]
fn graph_epic_challenged_while_hash_matches() {
    assert!(!graph_epic().challenged());
    let epic = challenged_now(graph_epic());
    assert!(epic.challenged());
    assert_eq!(epic.stage(), EpicStage::Challenged);
}

#[test]
fn graph_epic_unchallenged_after_body_edit() {
    let mut epic = challenged_now(graph_epic());
    epic.tasks[0] = graph_task(2, "a", "an edited goal");
    assert!(!epic.challenged());
    assert_eq!(epic.stage(), EpicStage::Planned);
}

#[test]
fn graph_epic_still_challenged_after_ticket_closes() {
    let mut epic = challenged_now(graph_epic());
    epic.tasks[0].state = Some(TicketState::Done);
    epic.tasks[0].standing = Standing::Done;
    epic.tasks[0].closed_at = Some(UNIX_EPOCH);
    assert!(epic.challenged());
}

#[test]
fn graph_epic_challenged_by_any_one_of_its_reports() {
    let current = PlanGraph::read(&graph_epic(), &[]).hash();
    let stale = "cd".repeat(32).parse::<GraphHash>().expect("a digest");
    let epic = Epic {
        challengers: vec![Some(stale), None, Some(current)],
        ..graph_epic()
    };
    assert!(epic.challenged());
    let stale_only = Epic {
        challengers: vec![Some("cd".repeat(32).parse().expect("a digest")), None],
        ..graph_epic()
    };
    assert!(
        !stale_only.challenged(),
        "a report with no hash line does not hold a graph epic"
    );
}

#[test]
fn older_epic_challenged_by_any_report() {
    let epic = Epic {
        tasks: vec![task(2, TicketState::Ready, Standing::Open)],
        challengers: vec![None],
        ..signed(epic())
    };
    assert!(epic.records.is_empty());
    assert!(epic.challenged());
    assert_eq!(epic.stage(), EpicStage::Challenged);
    let unread = Epic {
        challengers: Vec::new(),
        ..epic
    };
    assert!(!unread.challenged());
}

#[test]
fn a_report_holds_the_hash_on_its_last_line() {
    let hash = "ab".repeat(32);
    let report = format!("All exact.\ngraph sha256:{hash}\n");
    assert_eq!(report_graph(&report), hash.parse().ok());
    assert_eq!(report_graph("All exact."), None);
    assert_eq!(
        report_graph(&format!("graph sha256:{hash}\nand more")),
        None
    );
    assert_eq!(report_graph("graph sha256:nothex"), None);
}

#[test]
fn epic_last_record_with_mark_planned() {
    let epic = with_records(vec![record_of(1, false), record_of(2, true)]);
    assert_eq!(epic.stage(), EpicStage::Planned);
    assert!(!epic.half_filed());
}
