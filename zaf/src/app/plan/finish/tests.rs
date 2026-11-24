use std::path::Path;
use std::time::UNIX_EPOCH;

use super::*;
use crate::domain::plan::read::PlanGraph;
use crate::domain::plan::record::{FilingMark, PlanRecord};
use crate::domain::{DocumentSignature, TaskBody, Ticket, TicketState};
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

fn project(design: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = temp.path().join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(design, UNIX_EPOCH),
    )
    .expect("design");
    temp
}

fn section(node: &str, writes: &str, provides: &str, needs: &str) -> String {
    format!(
        "Task: Build {node}\nNode: {node}\nGoal: {node} works\nScope: the part\n\
         Out of scope: the rest\nInterfaces: none\nDesign pages: D2\nAcceptance:\n- it works\n\
         Tests: it_works\nSecurity trigger: no\nModel: sonnet\nSize: S\n\
         Writes: {writes}\nProvides: {provides}\nNeeds: {needs}\n\n"
    )
}

/// Fixture 12: four tickets, `A+B`, `C`, `D` and `I@7.1`.
fn plan() -> String {
    [
        section("A", "app/update", "none", "none"),
        section("B", "app/update", "none", "none"),
        section("C", "adapters/mcp", "mcp.catalog", "none"),
        section("D", "app/profile", "none", "mcp.catalog"),
    ]
    .concat()
}

fn finish_of(root: &Path, board: &FakeBoard) -> Result<Finished, FileError> {
    finish(
        root,
        &manifest(),
        board,
        &FakeVcs::default(),
        TicketId::from(EPIC),
    )
}

/// The four tickets of the plan, as a full filing on a scratch board writes them.
fn four_tickets(root: &Path) -> Vec<Ticket> {
    let scratch = board();
    super::super::file::file(
        root,
        &manifest(),
        &scratch,
        &FakeVcs::default(),
        TicketId::from(EPIC),
        plan().as_bytes(),
    )
    .expect("filed");
    scratch
        .epic(EPIC.into())
        .expect("epic")
        .tasks
        .into_iter()
        .map(|task| Ticket {
            id: task.number,
            epic: Some(EPIC.into()),
            title: task.title,
            body: task.body,
            state: Some(TicketState::Ready),
            url: "".into(),
            kind: None,
        })
        .collect()
}

/// A board with the record posted and its first `filed` tickets filed, and no mark.
fn half_filed(root: &Path, filed: usize, dropped: &[u64]) -> FakeBoard {
    let tickets = four_tickets(root);
    let board = board();
    let record = PlanRecord {
        k: 1,
        design: BodyHash::of(&TicketBody::from(
            design_text(root, &manifest(), "0.6.0").expect("the signed design"),
        )),
        dropped: dropped.iter().copied().map(TicketId::from).collect(),
        plan: plan(),
    };
    board
        .comment(EPIC.into(), &record.to_string())
        .expect("record");
    for (index, mut ticket) in tickets.into_iter().take(filed).enumerate() {
        ticket.id = TicketId::from(100 + index as u64);
        board.add_ticket(ticket);
    }
    board.reset_calls();
    board
}

fn nodes(board: &FakeBoard) -> Vec<String> {
    let epic = board.epic(EPIC.into()).expect("the epic");
    epic.tasks
        .iter()
        .map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => ticket.node.to_string(),
            other => panic!("ticket {} is no planned body: {other:?}", task.number),
        })
        .collect()
}

#[test]
fn plan_file_finish_files_the_rest() {
    let temp = project(DESIGN);
    let board = half_filed(temp.path(), 2, &[]);
    let Finished::Filed(filed) = finish_of(temp.path(), &board).expect("finished") else {
        panic!("nothing to finish");
    };

    assert_eq!((filed.filed.len(), filed.skipped), (2, 2));
    assert_eq!(board.calls("file_task"), 2, "only the two it lacked");
    let mut held = nodes(&board);
    held.sort();
    assert_eq!(held, ["A+B", "C", "D", "I@7.1"], "each node once");

    let comments = board.comments(EPIC.into());
    assert_eq!(comments.len(), 2, "the record and the mark: {comments:?}");
    let mark: FilingMark = comments[1].parse().expect("a filing mark");
    assert_eq!(mark.k, 1);
    let epics = board.epics().expect("epics");
    assert_eq!(mark.graph, PlanGraph::read(&epics[0], &epics).hash());
    assert!(
        board.epic(EPIC.into()).expect("epic").records[0]
            .mark
            .is_some()
    );
}

#[test]
fn plan_file_finish_closes_only_the_drops_still_open() {
    let temp = project(DESIGN);
    let board = half_filed(temp.path(), 4, &[8, 9]);
    for number in [8, 9] {
        board.add_ticket(Ticket {
            id: number.into(),
            epic: Some(EPIC.into()),
            title: "old".into(),
            body: "Goal: old.".into(),
            state: Some(TicketState::Ready),
            url: "".into(),
            kind: None,
        });
    }
    board.drop_task(9.into()).expect("closed already");
    board.reset_calls();
    finish_of(temp.path(), &board).expect("finished");

    assert_eq!(board.calls("drop_task"), 1);
    assert_eq!(board.closed_as(8.into()), Some(false), "not planned");
    assert_eq!(board.calls("file_task"), 0);
    assert_eq!(board.comments(EPIC.into()).len(), 2);
}

#[test]
fn plan_file_finish_raises_on_changed_design() {
    let temp = project(DESIGN);
    let board = half_filed(temp.path(), 2, &[]);
    let changed = format!("{DESIGN}\nA later edit.\n");
    let temp = project(&changed);
    let error = finish_of(temp.path(), &board).expect_err("refused");

    let FileError::DesignChanged { recorded, now, .. } = &error else {
        panic!("not a changed design: {error}");
    };
    assert_ne!(recorded, now);
    assert_eq!(board.calls("file_task"), 0);
    assert_eq!(board.comments(EPIC.into()).len(), 1, "no mark posted");
    let questions = board.questions().expect("questions");
    assert_eq!(questions.len(), 1);
    let text = NewQuestion::text_of(&questions[0]);
    assert!(text.contains(recorded) && text.contains(now), "{text}");

    // Asked again, the same question is not raised twice.
    finish_of(temp.path(), &board).expect_err("refused again");
    assert_eq!(board.questions().expect("questions").len(), 1);
}

#[test]
fn plan_file_finish_nothing_to_do() {
    let temp = project(DESIGN);
    let board = half_filed(temp.path(), 4, &[]);
    finish_of(temp.path(), &board).expect("finished");
    board.reset_calls();
    let before = board.comments(EPIC.into());

    let again = finish_of(temp.path(), &board).expect("nothing");
    assert_eq!(again, Finished::Nothing);
    assert_eq!(board.comments(EPIC.into()), before);
    assert_eq!(board.calls("comment"), 0);
    assert_eq!(board.calls("file_task"), 0);
    assert_eq!(board.calls("drop_task"), 0);

    // An epic with no record has nothing to finish either.
    let empty = self::board();
    assert_eq!(
        finish_of(temp.path(), &empty).expect("nothing"),
        Finished::Nothing
    );
}
