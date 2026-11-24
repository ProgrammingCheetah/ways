use std::path::Path;
use std::time::UNIX_EPOCH;

use super::*;
use crate::domain::plan::record::{FilingMark, PlanRecord};
use crate::domain::{DocumentSignature, Ticket};
use crate::fakes::{FakeBoard, FakeVcs};

const EPIC: u64 = 7;

const DESIGN: &str = "```toml vocabulary\n[areas]\n\"a\" = \"a\"\n\"b\" = \"b\"\n\
    \"app/update\" = \"update\"\n\"adapters/mcp\" = \"mcp\"\n\"app/profile\" = \"profile\"\n\
    [names.\"a.api\"]\nsignature = \"fn a()\"\n[names.\"b.api\"]\nsignature = \"fn b()\"\n\
    [names.\"mcp.catalog\"]\nsignature = \"fn catalog() -> Vec<Verb>\"\n```\n";

fn manifest() -> Manifest {
    "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n"
        .parse()
        .expect("manifest")
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_epic(EPIC.into(), "release");
    board.set_body(EPIC.into(), "Release: 0.6.0\n\nThe epic.");
    board
}

fn signed_design(root: &Path) {
    let folder = root.join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(DESIGN, UNIX_EPOCH),
    )
    .expect("design");
}

/// One section of a plan, as a planner writes it.
fn section(node: &str, writes: &str, provides: &str, needs: &str) -> String {
    format!(
        "Task: Build {node}\nNode: {node}\nGoal: {node} works\nScope: the part\n\
         Out of scope: the rest\nInterfaces: none\nDesign pages: D2\nAcceptance:\n- it works\n\
         Tests: it_works\nSecurity trigger: no\nModel: sonnet\nSize: S\n\
         Writes: {writes}\nProvides: {provides}\nNeeds: {needs}\n\n"
    )
}

/// The plan of fixture 12: A and B merge, D waits on the catalog's interface and not on C.
fn fixture_12() -> String {
    [
        section("A", "app/update", "none", "none"),
        section("B", "app/update", "none", "none"),
        section("C", "adapters/mcp", "mcp.catalog", "none"),
        section("D", "app/profile", "none", "mcp.catalog"),
    ]
    .concat()
}

fn run(root: &Path, board: &FakeBoard, plan: &str) -> Result<FiledPlan, FileError> {
    file(
        root,
        &manifest(),
        board,
        &FakeVcs::default(),
        TicketId::from(EPIC),
        plan.as_bytes(),
    )
}

fn project() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    signed_design(temp.path());
    temp
}

/// The nodes of the epic's tickets, by ticket number.
fn nodes(board: &FakeBoard) -> Vec<(TicketId, String)> {
    let epic = board.epic(EPIC.into()).expect("the epic");
    epic.tasks
        .iter()
        .map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => (task.number, ticket.node.to_string()),
            other => panic!("ticket {} is no planned body: {other:?}", task.number),
        })
        .collect()
}

#[test]
fn plan_file_posts_record_tickets_mark() {
    let temp = project();
    let board = board();
    let plan = fixture_12();
    let filed = run(temp.path(), &board, &plan).expect("filed");

    // One record, then one mark, and nothing else on the epic.
    let comments = board.comments(EPIC.into());
    assert_eq!(comments.len(), 2, "{comments:?}");
    let record: PlanRecord = comments[0].parse().expect("a plan record");
    assert_eq!((record.k, record.plan.as_str()), (1, plan.as_str()));
    assert!(record.dropped.is_empty());
    let mark: FilingMark = comments[1].parse().expect("a filing mark");
    assert_eq!(mark.k, 1);

    // Each ticket once, in natural id order, so the numbers rise with the ids.
    let ids: Vec<String> = nodes(&board).into_iter().map(|(_, node)| node).collect();
    assert_eq!(ids, ["A+B", "C", "D", "I@7.1"]);
    assert_eq!(filed.filed.len(), 4);
    assert_eq!(board.calls("file_task"), 4);

    // The mark holds the graph as the board now reads it.
    let epics = board.epics().expect("epics");
    let graph = PlanGraph::read(&epics[0], &epics);
    assert_eq!(mark.graph, graph.hash());

    assert_eq!(
        filed.summary(),
        "filed 4 tickets for 4 planned tasks (1 interface, 1 merge): chain 2 → 2, critical 2 → 2"
    );
}

#[test]
fn plan_file_files_each_ticket_once() {
    let temp = project();
    let board = board();
    let plan = fixture_12();
    run(temp.path(), &board, &plan).expect("filed");
    // The tickets are fixed nodes now (D29): the same plan again would make their ids again (the
    // merged `A+B` is named first, before the optimiser runs), and nothing is posted or filed.
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(
        matches!(error, FileError::Check(CheckError::NodeFiled(_))),
        "{error}"
    );
    assert_eq!(board.calls("file_task"), 4, "no ticket is filed twice");
    assert_eq!(nodes(&board).len(), 4);
    // One record with its mark.
    assert_eq!(board.comments(EPIC.into()).len(), 2);
    let epic = board.epic(EPIC.into()).expect("the epic");
    assert_eq!(epic.records.len(), 1);
    assert!(epic.records.iter().all(|filed| filed.mark.is_some()));
}

#[test]
fn plan_file_drops_as_not_planned() {
    let temp = project();
    let board = board();
    board.add_ticket(Ticket {
        id: 8.into(),
        epic: Some(EPIC.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    let plan = format!("Drop: #8\n\n{}", fixture_12());
    let filed = run(temp.path(), &board, &plan).expect("filed");

    assert_eq!(board.closed_as(8.into()), Some(false), "not planned");
    assert_eq!(filed.dropped, [TicketId::from(8)]);
    let comments = board.comments(EPIC.into());
    let record: PlanRecord = comments[0].parse().expect("a plan record");
    assert_eq!(record.dropped, [TicketId::from(8)]);
    assert_eq!(comments.len(), 2);
}

#[test]
fn plan_file_refuses_a_drop_that_is_no_task_of_the_epic() {
    let temp = project();
    let board = board();
    let plan = format!("Drop: #99\n\n{}", fixture_12());
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(matches!(error, FileError::NotItsTask { .. }), "{error}");
    assert!(board.comments(EPIC.into()).is_empty());
}

#[test]
fn plan_file_refused_posts_nothing() {
    let temp = project();
    let board = board();
    let cycle = [
        section("A", "a", "a.api", "b.api"),
        section("B", "b", "b.api", "a.api"),
    ]
    .concat();
    let error = run(temp.path(), &board, &cycle).expect_err("refused");
    assert!(
        matches!(error, FileError::Check(CheckError::Refused(_))),
        "{error}"
    );
    let unread = run(temp.path(), &board, "not a plan").expect_err("refused");
    assert!(
        matches!(unread, FileError::Check(CheckError::Unreadable(_))),
        "{unread}"
    );
    assert!(board.comments(EPIC.into()).is_empty());
    assert_eq!(board.calls("file_task"), 0);
    assert_eq!(board.calls("comment"), 0);
    assert_eq!(board.calls("drop_task"), 0);
}

#[test]
fn plan_file_refuses_record_over_limit() {
    let temp = project();
    let board = board();
    // Under the plan's own limits (lines of 2,000 bytes at most) and over a comment's.
    let long = "x".repeat(1_900);
    let padding: Vec<String> = (0..40).map(|_| format!("  {long}")).collect();
    let plan = fixture_12().replacen(
        "Scope: the part\n",
        &format!("Scope: the part\n{}\n", padding.join("\n")),
        1,
    );
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(matches!(error, FileError::TooLong { .. }), "{error}");
    assert!(board.comments(EPIC.into()).is_empty());
    assert_eq!(board.calls("file_task"), 0);
}

#[test]
fn plan_file_refuses_while_record_unmarked() {
    let temp = project();
    let board = board();
    let record = PlanRecord {
        k: 1,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: Vec::new(),
        plan: fixture_12(),
    };
    board
        .comment(EPIC.into(), &record.to_string())
        .expect("a record posted");
    board.reset_calls();
    let error = run(temp.path(), &board, &fixture_12()).expect_err("refused");
    assert!(
        matches!(error, FileError::Unmarked { record: 1, .. }),
        "{error}"
    );
    assert!(error.to_string().contains("zaf plan file 7"), "{error}");
    assert_eq!(board.comments(EPIC.into()).len(), 1, "nothing was posted");
    assert_eq!(board.calls("file_task"), 0);
}

/// D4, D13: a plan needs a filed task of another epic as `#<n>`. It files as a kept wait on that
/// task by its number, and the epic then passes `plan verify`.
#[test]
fn plan_file_keeps_a_need_on_a_filed_task_by_number() {
    let temp = project();
    let board = board();
    board.add_epic(8.into(), "older");
    board.set_body(8.into(), "Release: 0.6.0\n\nThe older epic.");
    board.add_ticket(Ticket {
        id: 9.into(),
        epic: Some(8.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: None,
        url: "".into(),
        kind: None,
    });
    let plan = section("D", "app/profile", "none", "#9");

    run(temp.path(), &board, &plan).expect("filed");

    let epic = board.epic(EPIC.into()).expect("the epic");
    let [task] = epic.tasks.as_slice() else {
        panic!("one ticket: {:?}", epic.tasks);
    };
    let Ok(TaskBody::Planned(ticket)) = TaskBody::parse(task.body.as_str()) else {
        panic!("a planned body: {}", task.body);
    };
    assert_eq!(
        ticket.waits,
        [crate::domain::plan::Wait {
            on: crate::domain::plan::WaitOn::Task(9.into()),
            name: crate::domain::plan::Name::of_task(9.into()),
        }]
    );
    let failures = crate::app::plan::verify::verify(
        temp.path(),
        &manifest(),
        &board,
        &FakeVcs::default(),
        TicketId::from(EPIC),
    )
    .expect("verifies");
    assert!(failures.is_empty(), "{failures:?}");
}

/// D13's third guard, Q10: the vocabulary gives epic #7 `mcp.catalog`, a filed ticket of epic #5
/// waits on epic #7 for it, and #7's plan leaves the name out. The plan is `unprovided` and
/// nothing is filed; with the name it files.
#[test]
fn plan_file_refuses_a_plan_without_a_name_another_epic_waits_for() {
    use crate::domain::plan::{Name, RefusalCode, Wait, WaitOn};
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = temp.path().join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    let design = DESIGN.replace(
        "signature = \"fn catalog() -> Vec<Verb>\"",
        "signature = \"fn catalog() -> Vec<Verb>\"\nepic = 7",
    );
    assert_ne!(design, DESIGN);
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(&design, UNIX_EPOCH),
    )
    .expect("design");
    let board = board();
    board.add_epic(5.into(), "the waiting epic");
    board.set_body(5.into(), "Release: 0.6.0\n\nThe waiting epic.");
    file(
        temp.path(),
        &manifest(),
        &board,
        &FakeVcs::default(),
        TicketId::from(5),
        section("D", "app/profile", "none", "mcp.catalog").as_bytes(),
    )
    .expect("epic 5 files its plan");
    let waiting = board.epic(5.into()).expect("epic 5");
    let Ok(TaskBody::Planned(ticket)) = TaskBody::parse(waiting.tasks[0].body.as_str()) else {
        panic!("a planned body: {}", waiting.tasks[0].body);
    };
    assert_eq!(
        ticket.waits,
        [Wait {
            on: WaitOn::Epic(EPIC.into()),
            name: Name::from("mcp.catalog"),
        }]
    );

    let without = section("A", "app/update", "none", "none");
    let error = run(temp.path(), &board, &without).expect_err("refused");
    let FileError::Check(CheckError::Refused(refusals)) = &error else {
        panic!("a refusal: {error}");
    };
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, RefusalCode::Unprovided);
    assert!(refusals[0].detail.contains("mcp.catalog"), "{refusals:?}");
    assert!(board.comments(EPIC.into()).is_empty());
    assert!(board.epic(EPIC.into()).expect("epic 7").tasks.is_empty());

    let with = [without, section("C", "adapters/mcp", "mcp.catalog", "none")].concat();
    run(temp.path(), &board, &with).expect("filed");
    let ids: Vec<String> = nodes(&board).into_iter().map(|(_, node)| node).collect();
    assert_eq!(
        ids,
        ["A", "C", "I@7.1"],
        "the name is lifted for the waiter"
    );
}

/// A crash after the `Drop:` tickets were closed and before the mark: the same plan run again is
/// told to finish the record, not that its dropped ticket is not open.
#[test]
fn plan_file_on_a_half_filed_epic_says_finish_it_before_any_other_refusal() {
    let temp = project();
    let board = board();
    board.add_ticket(Ticket {
        id: 8.into(),
        epic: Some(EPIC.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    let plan = format!("Drop: #8\n\n{}", fixture_12());
    run(temp.path(), &board, &plan).expect("filed");
    // The mark is lost: the record stands, its tickets are filed and #8 is closed.
    let scratch = self::board();
    scratch.add_ticket(Ticket {
        id: 8.into(),
        epic: Some(EPIC.into()),
        title: "an old task".into(),
        body: "Goal: old.".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    scratch.drop_task(8.into()).expect("closed");
    scratch
        .comment(EPIC.into(), &board.comments(EPIC.into())[0])
        .expect("the record");
    for task in board.epic(EPIC.into()).expect("epic").tasks {
        if task.number != TicketId::from(8) {
            scratch.add_ticket(Ticket {
                id: task.number,
                epic: Some(EPIC.into()),
                title: task.title,
                body: task.body,
                state: Some(TicketState::Ready),
                url: "".into(),
                kind: None,
            });
        }
    }
    scratch.reset_calls();

    let error = run(temp.path(), &scratch, &plan).expect_err("refused");

    assert!(
        matches!(error, FileError::Unmarked { record: 1, .. }),
        "{error}"
    );
    assert_eq!(scratch.comments(EPIC.into()).len(), 1, "nothing was posted");
    assert_eq!(scratch.calls("file_task"), 0);
}
