//! `plan verify` (D24, D27, I13): an epic's graph filed on the fake board is held to O3's checks;
//! a good one gives nothing, one built to break a check gives that one failure, and an epic
//! filed before 0.6.0 is refused with one line.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::UNIX_EPOCH;

use zaf::app::plan::verify::{VerifyError, verify};
use zaf::domain::plan::read::PlanGraph;
use zaf::domain::plan::record::{FilingMark, PlanRecord};
use zaf::domain::plan::shape::check_shape;
use zaf::domain::plan::{
    Area, Behaviour, KeptWait, Name, Node, NodeId, NodeKind, Size, Vocabulary, Weight,
};
use zaf::domain::{BodyHash, DocumentSignature, NewEpic, TicketBody, TicketId};
use zaf::fakes::{FakeBoard, FakeVcs};
use zaf::manifest::Manifest;
use zaf::ports::Board;

use super::ExpectedPlan;
use super::read::{board, file, file_older, output};

const DESIGN: &str = "```toml vocabulary\n\
    [names.\"mcp.catalog\"]\nsignature = \"fn catalog()\"\n\
    [names.\"sig.api\"]\nsignature = \"fn api()\"\n\
    [names.\"cli\"]\nno_signature = true\n\
    [names.\"plain.a\"]\nno_signature = true\n\
    [names.\"plain.b\"]\nno_signature = true\n```\n";

fn manifest() -> Manifest {
    "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n"
        .parse()
        .expect("manifest")
}

/// A project root whose release 0.6.0 has a signed design.
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

/// The failure lines `plan verify` prints for `epic`.
fn lines(root: &Path, board: &FakeBoard, epic: TicketId) -> Result<Vec<String>, VerifyError> {
    verify(root, &manifest(), board, &FakeVcs::default(), epic)
        .map(|failures| failures.iter().map(ToString::to_string).collect())
}

fn record() -> PlanRecord {
    PlanRecord {
        k: 1,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: Vec::new(),
        plan: String::new(),
    }
}

/// Plan record 1 with its filing mark, which holds the hash of the graph as the board reads now.
fn mark(board: &FakeBoard, epic: TicketId) {
    board
        .comment(epic, &record().to_string())
        .expect("the record posts");
    let epics = board.epics().expect("epics");
    let found = epics.iter().find(|e| e.id == epic).expect("the epic");
    let mark = FilingMark {
        k: 1,
        graph: PlanGraph::read(found, &epics).hash(),
    };
    board
        .comment(epic, &mark.to_string())
        .expect("the mark posts");
}

fn plan(nodes: Vec<Node>, kept: Vec<KeptWait>) -> ExpectedPlan {
    ExpectedPlan {
        nodes,
        orders: Vec::new(),
        kept,
        limit: None,
        lanes: None,
        record: None,
        shared: None,
        no_signature: None,
        existing: None,
        outside: None,
    }
}

fn names(list: &[&str]) -> BTreeSet<Name> {
    list.iter().map(|name| Name::from(*name)).collect()
}

fn behaviour(writes: &[&str], provides: &[&str], needs: &[&str]) -> Behaviour {
    Behaviour {
        text: "does a thing".to_owned(),
        size: Size::Weight(Weight(1)),
        writes: writes.iter().map(|area| Area::from(*area)).collect(),
        provides: names(provides),
        needs: names(needs),
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

/// Files `plan` under a fresh epic, marks it as filed, and verifies it.
fn broken(plan: &ExpectedPlan) -> Vec<String> {
    let root = project();
    let (board, epic) = board();
    file(&board, epic, plan);
    mark(&board, epic);
    lines(root.path(), &board, epic).expect("the epic verifies")
}

/// One failure line, for the rule `rule`, whose text holds each of `words`.
fn one_line(lines: &[String], rule: &str, words: &[&str]) {
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with(rule), "{lines:?}");
    for word in words {
        assert!(lines[0].contains(word), "{lines:?} lacks {word}");
    }
}

#[test]
fn plan_verify_passes_a_good_epic() {
    let root = project();
    let (board, epic) = board();
    file(&board, epic, &output("12-"));
    mark(&board, epic);

    let found = lines(root.path(), &board, epic).expect("verifies");

    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn plan_verify_fails_each_check_with_one_line() {
    // A wait on a name with a signature, through a body.
    let through = plan(
        vec![
            node("P", vec![behaviour(&["a"], &["sig.api"], &[])]),
            node("Q", vec![behaviour(&["b"], &[], &["sig.api"])]),
        ],
        vec![],
    );
    one_line(
        &broken(&through),
        "signature-through-interface",
        &["sig.api", "Q", "P"],
    );

    // A wait on a name the vocabulary does not know.
    let unsigned = plan(
        vec![
            node("P", vec![behaviour(&["a"], &["mystery"], &[])]),
            node("Q", vec![behaviour(&["b"], &[], &["mystery"])]),
        ],
        vec![],
    );
    one_line(&broken(&unsigned), "unsigned-wait", &["mystery"]);

    // A body over the limit.
    let heavy = plan(
        vec![node(
            "P",
            vec![
                behaviour(&["a"], &[], &[]),
                behaviour(&["b"], &[], &[]),
                behaviour(&["c"], &[], &[]),
            ],
        )],
        vec![],
    );
    one_line(&broken(&heavy), "body-limit", &["P"]);

    // Waits that go round.
    let cycle = plan(
        vec![
            node("P", vec![behaviour(&["a"], &["plain.a"], &["plain.b"])]),
            node("Q", vec![behaviour(&["b"], &["plain.b"], &["plain.a"])]),
        ],
        vec![],
    );
    one_line(&broken(&cycle), "cycle", &["P", "Q"]);

    // Two writers of one area with no order.
    let writers = plan(
        vec![
            node("P", vec![behaviour(&["a"], &[], &[])]),
            node("Q", vec![behaviour(&["a"], &[], &[])]),
        ],
        vec![],
    );
    one_line(&broken(&writers), "unordered-writers", &["P", "Q", "a"]);

    one_line(&edited_after_filing(), "graph-hash", &[]);
    one_line(&half_filed(), "half-filed", &["1"]);
}

/// A good epic that was edited after zaf filed it.
fn edited_after_filing() -> Vec<String> {
    let root = project();
    let (fake, epic) = board();
    let filed = file(&fake, epic, &output("12-"));
    mark(&fake, epic);
    let ticket = filed[0].1;
    let body = fake
        .epic(epic)
        .expect("the epic")
        .tasks
        .iter()
        .find(|task| task.number == ticket)
        .expect("the ticket")
        .body
        .to_string();
    fake.set_body(ticket, &body.replace("Goal: a goal", "Goal: another goal"));
    lines(root.path(), &fake, epic).expect("verifies")
}

/// A good epic whose last record has no mark.
fn half_filed() -> Vec<String> {
    let root = project();
    let (fake, epic) = board();
    file(&fake, epic, &output("12-"));
    fake.comment(epic, &record().to_string())
        .expect("the record posts");
    lines(root.path(), &fake, epic).expect("verifies")
}

#[test]
fn plan_verify_fails_a_kept_wait_on_a_body_for_a_signature() {
    let root = project();
    let (board, epic) = board();
    // Another epic's body ticket, which the first epic's node D waits on for a signature.
    let other = board
        .file_epic(&NewEpic {
            title: "other".into(),
            body: "Release: 0.6.0\n\nGoal.".into(),
        })
        .expect("the epic files");
    let theirs = file(
        &board,
        other,
        &plan(vec![node("B", vec![behaviour(&["z"], &[], &[])])], vec![]),
    );
    let on = theirs[0].1;
    let kept = KeptWait {
        from: NodeId::from(on.get().to_string().as_str()),
        to: NodeId::from("D"),
        needs: Name::from("sig.api"),
    };
    file(
        &board,
        epic,
        &plan(
            vec![node("D", vec![behaviour(&["d"], &[], &[])])],
            vec![kept],
        ),
    );
    mark(&board, epic);

    let found = lines(root.path(), &board, epic).expect("verifies");

    one_line(&found, "signature-through-interface", &["sig.api", "D"]);
}

#[test]
fn plan_verify_refuses_an_older_epic() {
    let root = project();
    let (board, epic) = board();
    let first = file_older(&board, epic, &[]);
    file_older(&board, epic, &[first]);

    let error = lines(root.path(), &board, epic).expect_err("an older epic is refused");

    assert!(matches!(error, VerifyError::Older), "{error}");
    assert_eq!(
        error.to_string(),
        "planned before 0.6.0: plan verify checks graph epics"
    );
}

#[test]
fn plan_verify_agrees_with_check_shape() {
    let root = project();
    let (board, epic) = board();
    file(&board, epic, &output("12-"));
    mark(&board, epic);
    let epics = board.epics().expect("epics");
    let found = epics.iter().find(|e| e.id == epic).expect("the epic");
    let graph = PlanGraph::read(found, &epics);
    let vocabulary = Vocabulary {
        signature: names(&["mcp.catalog", "sig.api"]),
        no_signature: names(&["cli", "plain.a", "plain.b"]),
        ..Vocabulary::default()
    };

    let direct = check_shape(&graph, &vocabulary, &found.records);
    let verified =
        verify(root.path(), &manifest(), &board, &FakeVcs::default(), epic).expect("verifies");

    assert_eq!(verified, direct);
}
