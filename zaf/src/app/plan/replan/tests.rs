//! A re-plan on a fake board: fixtures 27 and 28 of the plan-graph set, filed for real and then
//! planned again, and the `Drop:` rule.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::*;
use crate::app::plan::check::CheckError;
use crate::app::plan::file::{FileError, FiledPlan, file};
use crate::domain::plan::{RefusalCode, Turn};
use crate::domain::{DocumentSignature, TaskBody};
use crate::fakes::{FakeBoard, FakeVcs};
use crate::manifest::Manifest;
use crate::ports::Board;

const EPIC: u64 = 7;

const DESIGN: &str = "```toml vocabulary\n[areas]\n\"store\" = \"the store\"\n\
    \"cli\" = \"the cli\"\n\"hub\" = \"the hub\"\n\"docs\" = \"the docs\"\n\"dstore\" = \"d\"\n\
    [names.\"s\"]\nsignature = \"fn s()\"\n[names.\"d\"]\nsignature = \"fn d()\"\n\
    [names.\"q\"]\nsignature = \"fn q()\"\n[names.\"t\"]\nsignature = \"fn t()\"\n\
    [names.\"y\"]\nsignature = \"fn y()\"\n```\n";

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

fn project() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = temp.path().join(".teams/releases/0.6.0");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign(DESIGN, UNIX_EPOCH),
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

/// The epic's tickets by node: number and body as filed.
fn filed(board: &FakeBoard) -> BTreeMap<String, (TicketId, String)> {
    let epic = board.epic(EPIC.into()).expect("the epic");
    epic.tasks
        .iter()
        .map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => (
                ticket.node.to_string(),
                (task.number, task.body.as_str().to_owned()),
            ),
            other => panic!("ticket {} is no planned body: {other:?}", task.number),
        })
        .collect()
}

fn turns_of(board: &FakeBoard, node: &str) -> Vec<Turn> {
    let (_, body) = filed(board).remove(node).expect("the node is filed");
    match TaskBody::parse(&body) {
        Ok(TaskBody::Planned(ticket)) => ticket.turns,
        other => panic!("{other:?}"),
    }
}

/// Fixture 27's board: P gives s to A, so s is lifted into I@7.1; B gives d; X writes hub. Then
/// B is done.
fn first_plan_filed(root: &Path, board: &FakeBoard) {
    let plan = [
        section("P", "store", "s", "none"),
        section("A", "cli", "none", "s"),
        section("B", "dstore", "d", "none"),
        section("X", "hub", "none", "none"),
    ]
    .concat();
    run(root, board, &plan).expect("the first plan is filed");
    let (b, _) = filed(board).remove("B").expect("B is filed");
    board.close(b);
}

/// Fixture 27's second plan: C uses s and d and gives t, E uses t, Y2 writes hub.
fn second_plan() -> String {
    [
        section("C", "cli", "t", "s, d"),
        section("E", "docs", "none", "t"),
        section("Y2", "hub", "y", "none"),
    ]
    .concat()
}

#[test]
fn replan_files_beside_filed_tickets() {
    let temp = project();
    let board = board();
    first_plan_filed(temp.path(), &board);
    let before = filed(&board);
    assert_eq!(
        before.keys().map(String::as_str).collect::<Vec<_>>(),
        ["A", "B", "I@7.1", "P", "X"]
    );

    let second = run(temp.path(), &board, &second_plan()).expect("the re-plan is filed");

    assert_eq!(second.record, 2);
    let nodes: Vec<&str> = second.filed.iter().map(|(node, _)| node.as_str()).collect();
    assert_eq!(nodes, ["C", "E", "I@7.2", "Y2"], "only the new nodes");
    assert_eq!(second.skipped, 0);

    // Every ticket filed before is as it was.
    let after = filed(&board);
    for (node, held) in &before {
        assert_eq!(after.get(node), Some(held), "{node} is unchanged");
    }
    // The new signatures went into the new interface ticket, never into I@7.1.
    let interface = &after["I@7.2"].1;
    assert!(interface.contains("t: fn t()"), "{interface}");
    assert!(!after["I@7.1"].1.contains("fn t()"));

    // The new tasks take turns after the filed tickets that write their area.
    let area = |text: &str| crate::domain::plan::Area::from(text);
    assert_eq!(
        turns_of(&board, "C"),
        [Turn(NodeId::from("A"), area("cli"))]
    );
    assert_eq!(
        turns_of(&board, "Y2"),
        [Turn(NodeId::from("X"), area("hub"))]
    );
    assert!(turns_of(&board, "E").is_empty());
    // Nothing is merged with a filed ticket, and the first plan's interface kept its own.
    assert!(!after.keys().any(|node| node.contains('+')));
    assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 2);
}

#[test]
fn replan_refuses_fixed_provider() {
    let temp = project();
    let board = board();
    // P2 gives q and nothing needs it yet, so no interface holds q.
    run(temp.path(), &board, &section("P2", "store", "q", "none")).expect("filed");
    let error = run(temp.path(), &board, &section("N", "cli", "none", "q")).expect_err("refused");
    match error {
        FileError::Check(CheckError::Refused(refusals)) => {
            assert_eq!(refusals.len(), 1, "{refusals:?}");
            assert_eq!(refusals[0].code, RefusalCode::FixedProvider);
        }
        other => panic!("not refused as fixed-provider: {other}"),
    }
    assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 1);
}

fn phase_run(root: &Path, task: TicketId) {
    let folder = root.join(".zaf/phases/run-1");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("action"), "implement").expect("action");
    std::fs::write(folder.join("task"), format!("{}\n", task.get())).expect("task");
}

#[test]
fn replan_refuses_drop_of_started_ticket() {
    let temp = project();
    let board = board();
    run(temp.path(), &board, &section("A", "cli", "none", "none")).expect("filed");
    let (a, _) = filed(&board).remove("A").expect("A");
    phase_run(temp.path(), a);
    let plan = format!(
        "Drop: #{}\n\n{}",
        a.get(),
        section("C", "docs", "none", "none")
    );
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(
        matches!(&error, FileError::Check(CheckError::Drop(barred))
            if barred.0 == [(a, DropBar::PhaseRun)]),
        "{error}"
    );
    assert!(
        error.to_string().contains(&a.to_string()),
        "it names the ticket: {error}"
    );
    assert_eq!(board.closed_as(a), None);
    assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 1);
    assert_eq!(board.calls("drop_task"), 0);
}

#[test]
fn replan_refuses_drop_of_ticket_with_pull_request() {
    let temp = project();
    let board = board();
    run(temp.path(), &board, &section("A", "cli", "none", "none")).expect("filed");
    let (a, _) = filed(&board).remove("A").expect("A");
    let slices = temp.path().join(".zaf/slices");
    std::fs::create_dir_all(&slices).expect("slices");
    std::fs::write(slices.join(format!("{}.pr", a.get())), "12").expect("record");
    let plan = format!(
        "Drop: #{}\n\n{}",
        a.get(),
        section("C", "docs", "none", "none")
    );
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(
        matches!(&error, FileError::Check(CheckError::Drop(barred))
            if barred.0 == [(a, DropBar::PullRequest)]),
        "{error}"
    );
}

#[test]
fn replan_drops_unstarted_ticket() {
    let temp = project();
    let board = board();
    run(temp.path(), &board, &section("A", "cli", "none", "none")).expect("filed");
    let (a, _) = filed(&board).remove("A").expect("A");
    let plan = format!(
        "Drop: #{}\n\n{}",
        a.get(),
        section("C", "cli", "none", "none")
    );
    let second = run(temp.path(), &board, &plan).expect("filed");

    assert_eq!(second.dropped, [a]);
    assert_eq!(board.closed_as(a), Some(false), "closed as not planned");
    // The dropped ticket is no fixed node, so C takes no turn after it.
    assert!(turns_of(&board, "C").is_empty());
}

#[test]
fn replan_refuses_a_drop_of_a_done_ticket() {
    let temp = project();
    let board = board();
    run(temp.path(), &board, &section("A", "cli", "none", "none")).expect("filed");
    let (a, _) = filed(&board).remove("A").expect("A");
    board.close(a);
    let plan = format!(
        "Drop: #{}\n\n{}",
        a.get(),
        section("C", "docs", "none", "none")
    );
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    assert!(
        matches!(&error, FileError::Check(CheckError::Drop(barred))
            if barred.0 == [(a, DropBar::NotOpen)]),
        "{error}"
    );
}

#[test]
fn replan_refuses_a_node_a_closed_ticket_holds() {
    let temp = project();
    let board = board();
    run(temp.path(), &board, &section("A", "cli", "none", "none")).expect("filed");
    let (a, _) = filed(&board).remove("A").expect("A");
    board.close(a);
    let error =
        run(temp.path(), &board, &section("A", "docs", "none", "none")).expect_err("refused");
    assert!(
        matches!(&error, FileError::Check(CheckError::NodeFiled(taken))
            if taken.0 == [(NodeId::from("A"), a)]),
        "{error}"
    );
}

#[test]
fn a_dropped_name_must_be_provided_again() {
    let temp = project();
    let board = board();
    // Y gives y, no one needs it yet: it is no interface's. X will need y, so the second plan
    // drops Y and must give y again.
    run(temp.path(), &board, &section("Y", "hub", "y", "none")).expect("filed");
    let (y, _) = filed(&board).remove("Y").expect("Y");
    let plan = format!("Drop: #{}\n\n{}", y.get(), section("X", "cli", "none", "y"));
    let error = run(temp.path(), &board, &plan).expect_err("refused");
    match error {
        FileError::Check(CheckError::Refused(refusals)) => {
            assert!(
                refusals
                    .iter()
                    .all(|refusal| refusal.code == RefusalCode::Unprovided),
                "{refusals:?}"
            );
        }
        other => panic!("{other}"),
    }
    let again = [
        section("X", "cli", "none", "y"),
        section("Y2", "hub", "y", "none"),
    ]
    .concat();
    let ok = run(
        temp.path(),
        &board,
        &format!("Drop: #{}\n\n{again}", y.get()),
    )
    .expect("filed once y is given again");
    assert_eq!(ok.dropped, [y]);
}

#[test]
fn an_epic_with_no_record_has_no_fixed_nodes() {
    let epic = Epic {
        id: TicketId::from(EPIC),
        title: "e".into(),
        body: "Release: 0.6.0".into(),
        tasks: Vec::new(),
        signature: None,
        planned_at: None,
        challengers: Vec::new(),
        records: Vec::new(),
    };
    let key = EpicKey::from("7".to_owned());
    assert!(fixed_nodes(&epic, &key, &[], &|_| false).is_empty());
    assert_eq!(next_record(&epic), 1);
}

/// A and B write one area and fit the limit, so the first plan files them as one ticket `A+B`.
fn merged_ticket_filed(root: &Path, board: &FakeBoard) -> TicketId {
    let plan = [
        section("A", "cli", "none", "none"),
        section("B", "cli", "none", "none"),
    ]
    .concat();
    run(root, board, &plan).expect("the first plan is filed");
    filed(board).remove("A+B").expect("A+B is filed").0
}

/// The keys A and B again, writing `area`: they would merge into `A+B` once more.
fn same_keys(area: &str) -> String {
    [
        section("A", area, "none", "none"),
        section("B", area, "none", "none"),
    ]
    .concat()
}

fn assert_merged_id_refused(error: &FileError, held: TicketId) {
    assert!(
        matches!(error, FileError::Check(CheckError::NodeFiled(taken))
            if taken.0 == [(NodeId::from("A+B"), held)]),
        "{error}"
    );
    let said = error.to_string();
    assert!(
        said.contains("A+B") && said.contains(&held.to_string()) && said.contains("other keys"),
        "it names the id, the ticket and the way on: {said}"
    );
}

#[test]
fn replan_refuses_keys_that_merge_into_the_id_of_a_ticket_it_drops() {
    let temp = project();
    let board = board();
    let held = merged_ticket_filed(temp.path(), &board);
    let plan = format!("Drop: #{}\n\n{}", held.get(), same_keys("cli"));

    let error = run(temp.path(), &board, &plan).expect_err("refused");

    assert_merged_id_refused(&error, held);
    // Nothing was posted: the ticket stands and the epic has its one record.
    assert_eq!(board.closed_as(held), None);
    assert_eq!(board.calls("drop_task"), 0);
    assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 1);
    assert_eq!(filed(&board).len(), 1);
}

#[test]
fn replan_refuses_keys_that_merge_into_the_id_of_a_done_or_closed_ticket() {
    for done in [true, false] {
        let temp = project();
        let board = board();
        let held = merged_ticket_filed(temp.path(), &board);
        if done {
            board.close(held);
        } else {
            board.close_not_planned(held);
        }

        let error = run(temp.path(), &board, &same_keys("docs")).expect_err("refused");

        assert_merged_id_refused(&error, held);
        assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 1);
        assert_eq!(filed(&board).len(), 1);
    }
}

#[test]
fn replan_refuses_keys_that_merge_into_the_id_of_an_open_ticket() {
    let temp = project();
    let board = board();
    let held = merged_ticket_filed(temp.path(), &board);

    // All three write one area: it was two nodes `A+B` and a panic in the measure.
    let error = run(temp.path(), &board, &same_keys("cli")).expect_err("refused");

    assert_merged_id_refused(&error, held);
    assert_eq!(board.epic(EPIC.into()).expect("epic").records.len(), 1);
    assert_eq!(filed(&board).len(), 1);
}

#[test]
fn replan_takes_new_keys_beside_a_merged_ticket() {
    let temp = project();
    let board = board();
    let held = merged_ticket_filed(temp.path(), &board);
    let plan = [
        section("A2", "cli", "none", "none"),
        section("B2", "cli", "none", "none"),
    ]
    .concat();

    let second = run(temp.path(), &board, &plan).expect("filed");

    let nodes: Vec<&str> = second.filed.iter().map(|(node, _)| node.as_str()).collect();
    assert_eq!(nodes, ["A2+B2"]);
    assert_eq!(board.closed_as(held), None);
}

/// D6: what a record's `New names:` added lasts, as `plan verify` holds it to. A later plan
/// writes the area, needs the name with no signature and lifts the one with a signature, and
/// declares none of them again.
#[test]
fn replan_knows_the_names_an_earlier_record_added() {
    let temp = project();
    let board = board();
    let first = format!(
        "New names:\n- area extra\n- no signature m\n- signature z: fn z()\n\n{}",
        section("X", "hub", "m", "none")
    );
    run(temp.path(), &board, &first).expect("the first plan is filed");

    let second = [
        // `extra` is record 1's area, and `m` its name with no signature, which the open X gives.
        section("N", "extra", "none", "m"),
        // `z` is record 1's name with a signature: lifted, with the shape record 1 gave it.
        section("Z", "store", "z", "none"),
        section("R", "docs", "none", "z"),
    ]
    .concat();
    let filed_now = run(temp.path(), &board, &second).expect("the re-plan is filed");

    let nodes: Vec<&str> = filed_now
        .filed
        .iter()
        .map(|(node, _)| node.as_str())
        .collect();
    assert_eq!(nodes, ["I@7.2", "N", "R", "Z"]);
    let after = filed(&board);
    assert!(
        after["I@7.2"].1.contains("z: fn z()"),
        "{}",
        after["I@7.2"].1
    );
    // `m` has no signature, so N waits on X itself and no interface holds `m`.
    assert!(!after["I@7.2"].1.contains("m:"), "{}", after["I@7.2"].1);

    let failures = crate::app::plan::verify::verify(
        temp.path(),
        &manifest(),
        &board,
        &FakeVcs::default(),
        TicketId::from(EPIC),
    )
    .expect("verified");
    assert!(failures.is_empty(), "{failures:?}");
}
