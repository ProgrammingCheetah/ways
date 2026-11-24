//! A challenger's report on its epic (M12): accepted only while the graph hashes as it did at
//! the launch.

use super::*;
use crate::domain::CHALLENGER_ROLE;
use crate::domain::plan::read::PlanGraph;

fn challenger() -> Instance {
    Instance {
        role: RoleName::from(CHALLENGER_ROLE),
        epic: TicketId::from(2),
        number: InstanceNumber::from(1),
    }
}

/// A board with epic 2 holding one task, and the hash its graph has now.
fn challenged_board() -> (FakeBoard, String) {
    let board = FakeBoard::default();
    board.add_signed_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(Ticket {
        epic: Some(TicketId::from(2)),
        body: body("no", "sonnet", "S").into(),
        ..ticket(3, TicketState::Ready)
    });
    let hash = graph_hash(&board);
    (board, hash)
}

fn graph_hash(board: &FakeBoard) -> String {
    let epics = board.epics().expect("epics");
    let epic = epics.iter().find(|epic| epic.id == TicketId::from(2));
    format!(
        "sha256:{}",
        PlanGraph::read(epic.expect("the epic"), &epics).hash()
    )
}

/// A state dir whose run folder for `instance` holds a launch record with `graph`.
fn launched(instance: &Instance, graph: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = dir
        .path()
        .join("runs")
        .join(crate::app::spawn::slug(instance));
    std::fs::create_dir_all(&run).expect("run folder");
    let record = runs::LaunchRecord {
        instance: instance.to_string(),
        tasks: Vec::new(),
        reserved_at: "2026-11-24T00:00:00Z".to_owned(),
        started_at: None,
        graph: graph.map(str::to_owned),
    };
    let text = serde_json::to_string(&record).expect("record");
    std::fs::write(run.join(runs::LAUNCH_FILE), text).expect("launch record");
    dir
}

fn challenger_report(body: &str) -> ReportCommand {
    ReportCommand {
        task: TicketId::from(2),
        instance: challenger(),
        body: body.to_owned(),
    }
}

#[test]
fn challenger_report_appends_graph_hash() {
    let (board, hash) = challenged_board();
    let state = launched(&challenger(), Some(&hash));
    let outcome = report_checked(&board, state.path(), &challenger_report("pass\n"))
        .expect("the graph is as launched");
    assert_eq!(outcome, ReportOutcome { heartbeat: false });
    let reports = board.reports(TicketId::from(2));
    assert_eq!(reports.len(), 1);
    let last = reports[0].1.lines().last().expect("a last line");
    assert_eq!(last, format!("graph {hash}"));
    assert_eq!(reports[0].1, format!("pass\ngraph {hash}"));
}

#[test]
fn challenger_report_refused_after_mid_check_filing() {
    let (board, launch) = challenged_board();
    let state = launched(&challenger(), Some(&launch));
    // The challenger files a ticket in the middle of its check.
    board
        .file_task(&NewTask {
            epic: TicketId::from(2),
            title: "Sneaked in".into(),
            body: body("no", "sonnet", "S").into(),
            state: TicketState::Ready,
            group: None,
            kind: "Task".into(),
        })
        .expect("files");
    let now = graph_hash(&board);
    assert_ne!(now, launch);

    let refused = report_checked(&board, state.path(), &challenger_report("pass"));
    let Err(BoardError::GraphChanged {
        launch: named_launch,
        now: named_now,
        question,
    }) = refused
    else {
        panic!("expected a refusal, got {refused:?}");
    };
    assert_eq!(named_launch, launch);
    assert_eq!(named_now, now);
    assert!(
        board.reports(TicketId::from(2)).is_empty(),
        "nothing written"
    );

    let asked = board.questions_on(TicketId::from(2)).expect("questions");
    assert_eq!(asked.len(), 1, "one question on the epic");
    assert_eq!(asked[0].0.id, question);
    let text = NewQuestion::text_of(&asked[0].0);
    assert!(text.contains(&launch) && text.contains(&now), "{text}");

    // The same pair again is the question already open, not a second one.
    let again = report_checked(&board, state.path(), &challenger_report("pass"));
    assert!(
        matches!(again, Err(BoardError::GraphChanged { question: same, .. }) if same == question),
        "{again:?}"
    );
    let asked = board.questions_on(TicketId::from(2)).expect("questions");
    assert_eq!(asked.len(), 1);
}

#[test]
fn challenger_report_without_launch_graph_written_as_today() {
    let (board, _) = challenged_board();
    let state = launched(&challenger(), None);
    report_checked(&board, state.path(), &challenger_report("pass")).expect("written");
    assert_eq!(
        board.reports(TicketId::from(2)),
        [(challenger(), "pass".to_owned())]
    );
    // A run folder with no launch record at all reads the same.
    let bare = tempfile::tempdir().expect("temp dir");
    report_checked(&board, bare.path(), &challenger_report("pass again")).expect("written");
    assert_eq!(board.reports(TicketId::from(2))[0].1, "pass again");
}

#[test]
fn team_report_never_hashed() {
    let (board, hash) = challenged_board();
    let team = Instance {
        epic: TicketId::from(2),
        ..instance(1)
    };
    // Even a record with a graph under the team's slug changes nothing for a team.
    let state = launched(&team, Some(&hash));
    let command = ReportCommand {
        task: TicketId::from(2),
        instance: team.clone(),
        body: "epic done".to_owned(),
    };
    report_checked(&board, state.path(), &command).expect("written");
    assert_eq!(
        board.reports(TicketId::from(2)),
        [(team, "epic done".to_owned())]
    );
    // A challenger's report on another issue is not hashed either.
    let state = launched(&challenger(), Some("sha256:stale"));
    let on_task = ReportCommand {
        task: TicketId::from(3),
        ..challenger_report("notes")
    };
    report_checked(&board, state.path(), &on_task).expect("written");
    assert_eq!(board.reports(TicketId::from(3))[0].1, "notes");
}
