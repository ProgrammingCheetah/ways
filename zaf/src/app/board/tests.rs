use std::path::{Path, PathBuf};

use super::*;
use crate::app::check::{CheckRecord, Verdict};
use crate::domain::{InstanceNumber, RoleName};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
use crate::ports::{BaseName, PrNumber, SliceName};

fn ticket(number: u64, label: TicketState) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: Some(TicketId::from(1)),
        title: format!("Task {number}").into(),
        body: "".into(),
        state: Some(label),
        url: "".into(),
    }
}

fn instance(number: u32) -> Instance {
    Instance {
        role: RoleName::from("team"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(number),
    }
}

fn board_with(tickets: &[(u64, TicketState)]) -> FakeBoard {
    let board = FakeBoard::default();
    for (number, label) in tickets {
        board.add_ticket(ticket(*number, *label));
    }
    board
}

fn claim_command(task: u64, by: u32, batch: u32) -> ClaimCommand {
    ClaimCommand {
        task: TicketId::from(task),
        instance: instance(by),
        batch,
    }
}

#[test]
fn ready_lists_only_ready_tasks_in_issue_order() {
    let board = board_with(&[
        (4, TicketState::Ready),
        (2, TicketState::Ready),
        (3, TicketState::Claimed),
        (5, TicketState::Done),
    ]);
    let numbers: Vec<TicketId> = ready(&board)
        .expect("ready")
        .into_iter()
        .map(|ticket| ticket.id)
        .collect();
    assert_eq!(numbers, [TicketId::from(2), TicketId::from(4)]);
}

#[test]
fn a_claim_takes_a_ready_task_and_a_repeat_is_quiet() {
    let board = board_with(&[(2, TicketState::Ready)]);
    let command = claim_command(2, 1, 1);
    assert_eq!(claim(&board, &command), Ok(ClaimOutcome::Claimed));
    assert_eq!(claim(&board, &command), Ok(ClaimOutcome::AlreadyHeld));
    let task = board.task(TicketId::from(2)).expect("task");
    assert_eq!(task.state, Some(TicketState::Claimed));
}

#[test]
fn claims_stop_at_the_batch_size() {
    let board = board_with(&[
        (2, TicketState::Ready),
        (3, TicketState::Ready),
        (4, TicketState::Ready),
    ]);
    claim(&board, &claim_command(2, 1, 2)).expect("first");
    claim(&board, &claim_command(3, 1, 2)).expect("second");
    let refused = claim(&board, &claim_command(4, 1, 2));
    let Err(BoardError::BatchFull { batch, held, .. }) = refused else {
        panic!("expected a full batch, got {refused:?}");
    };
    assert_eq!(batch, 2);
    assert_eq!(held, [TicketId::from(2), TicketId::from(3)]);
    let untouched = board.task(TicketId::from(4)).expect("task");
    assert_eq!(untouched.state, Some(TicketState::Ready));
    // A repeat of a held claim never counts against the batch.
    assert_eq!(
        claim(&board, &claim_command(3, 1, 2)),
        Ok(ClaimOutcome::AlreadyHeld)
    );
    // Another instance's claims are not this one's batch.
    assert_eq!(
        claim(&board, &claim_command(4, 2, 1)),
        Ok(ClaimOutcome::Claimed)
    );
}

#[test]
fn a_claim_refuses_a_task_held_by_another_or_not_ready() {
    let board = board_with(&[(2, TicketState::Ready), (3, TicketState::Blocked)]);
    claim(&board, &claim_command(2, 1, 4)).expect("claim");
    assert!(matches!(
        claim(&board, &claim_command(2, 2, 4)),
        Err(BoardError::Port(PortError::Conflict(_)))
    ));
    assert!(matches!(
        claim(&board, &claim_command(3, 2, 4)),
        Err(BoardError::Port(PortError::Rejected(_)))
    ));
    assert!(matches!(
        claim(&board, &claim_command(9, 2, 4)),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn a_report_replaces_the_last_and_touches_a_held_claim() {
    let board = board_with(&[(1, TicketState::Claimed), (2, TicketState::Ready)]);
    claim(&board, &claim_command(2, 1, 4)).expect("claim");
    let before = board.claims().expect("claims")[0].heartbeat;
    let command = ReportCommand {
        task: TicketId::from(2),
        instance: instance(1),
        body: "working".to_owned(),
    };
    let outcome = report(&board, &command).expect("report");
    assert_eq!(outcome, ReportOutcome { heartbeat: true });
    assert!(board.claims().expect("claims")[0].heartbeat >= before);
    let again = ReportCommand {
        body: "tests green".to_owned(),
        ..command
    };
    report(&board, &again).expect("a second report");
    report(&board, &again).expect("the same report again");
    let reports = board.reports(TicketId::from(2));
    assert_eq!(reports, [(instance(1), "tests green".to_owned())]);
}

#[test]
fn a_report_on_an_unclaimed_issue_writes_the_report_only() {
    let board = board_with(&[(1, TicketState::Ready)]);
    let command = ReportCommand {
        task: TicketId::from(1),
        instance: instance(1),
        body: "epic done".to_owned(),
    };
    let outcome = report(&board, &command).expect("report");
    assert_eq!(outcome, ReportOutcome { heartbeat: false });
    assert_eq!(board.reports(TicketId::from(1)).len(), 1);
    let missing = ReportCommand {
        task: TicketId::from(9),
        ..command
    };
    assert!(matches!(
        report(&board, &missing),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn a_question_opens_a_ticket_and_the_issue_waits() {
    let board = board_with(&[(2, TicketState::Claimed)]);
    let command = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns?".to_owned(),
        door: true,
    };
    let number = question(&board, &command).expect("question");
    assert_eq!(board.question(number), Some((true, None)));
    let waiting = board.task(TicketId::from(2)).expect("task");
    assert_eq!(waiting.state, Some(TicketState::Question));
    let nowhere = NewQuestion {
        on: TicketId::from(9),
        ..command
    };
    assert!(matches!(
        question(&board, &nowhere),
        Err(BoardError::Port(PortError::NotFound(_)))
    ));
}

#[test]
fn asking_the_same_question_again_reuses_the_open_one() {
    let board = board_with(&[(2, TicketState::Claimed), (3, TicketState::Ready)]);
    let asked = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let first = question(&board, &asked).expect("question");
    let again = question(&board, &asked).expect("the same question");
    assert_eq!(again, first, "no second ticket");
    assert_eq!(board.questions().expect("questions").len(), 1);
    let waiting = board.task(TicketId::from(2)).expect("task");
    assert_eq!(waiting.state, Some(TicketState::Question));

    let elsewhere = NewQuestion {
        on: TicketId::from(3),
        ..asked.clone()
    };
    let other = question(&board, &elsewhere).expect("same text, other issue");
    assert_ne!(other, first, "another issue is another question");
    let reworded = NewQuestion {
        text: "Which rows?".to_owned(),
        ..asked.clone()
    };
    let third = question(&board, &reworded).expect("other text");
    assert_ne!(third, first);

    ruling(
        &board,
        &RulingCommand {
            question: first,
            ruling: "Title and author.".to_owned(),
        },
    )
    .expect("ruling");
    let reasked = question(&board, &asked).expect("asked after the ruling");
    assert_ne!(
        reasked, first,
        "a ruled question is not open, so it is asked anew"
    );
}

#[test]
fn a_ruling_closes_the_question_once() {
    let board = board_with(&[(2, TicketState::Claimed)]);
    let asked = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let number = question(&board, &asked).expect("question");
    let command = RulingCommand {
        question: number,
        ruling: "Title and author.".to_owned(),
    };
    ruling(&board, &command).expect("ruling");
    ruling(&board, &command).expect("the same ruling again is quiet");
    assert_eq!(
        board.question(number),
        Some((false, Some("Title and author.".to_owned())))
    );
    let different = RulingCommand {
        ruling: "Title only.".to_owned(),
        ..command
    };
    assert!(matches!(
        ruling(&board, &different),
        Err(BoardError::Port(PortError::Conflict(_)))
    ));
}

#[test]
fn a_ruling_returns_the_task_to_ready_so_it_can_be_claimed_again() {
    let board = board_with(&[(2, TicketState::Claimed)]);
    let asked = NewQuestion {
        on: TicketId::from(2),
        text: "Which columns?".to_owned(),
        door: false,
    };
    let number = question(&board, &asked).expect("question");
    assert!(matches!(
        claim(&board, &claim_command(2, 2, 1)),
        Err(BoardError::Port(PortError::Rejected(_)))
    ));
    let command = RulingCommand {
        question: number,
        ruling: "Title and author.".to_owned(),
    };
    assert_eq!(ruling(&board, &command), Ok(Some(TicketId::from(2))));
    assert_eq!(ruling(&board, &command), Ok(None));
    assert_eq!(
        claim(&board, &claim_command(2, 2, 1)),
        Ok(ClaimOutcome::Claimed)
    );
}

/// A project on disk and the fakes behind it, holding one claimed task.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    workspace: PathBuf,
    board: FakeBoard,
    forge: FakeForge,
    vcs: FakeVcs,
}

const TASK: u64 = 319;
const SLICE: &str = "feature/0.2.0-103-319";
const BASE: &str = "feature/0.2.0";

fn put(file: &Path, text: &str) {
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    std::fs::write(file, text).expect("write");
}

impl Scene {
    /// A claimed task with a slice, a pull request at the slice's head and a clean review of it.
    fn landable() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(dir.path()).expect("real path");
        let (root, workspace) = (base.join("project"), base.join("workspace"));
        std::fs::create_dir_all(&workspace).expect("mkdir");
        let scene = Self {
            _dir: dir,
            root,
            workspace,
            board: board_with(&[(TASK, TicketState::Ready)]),
            forge: FakeForge::default(),
            vcs: FakeVcs::default(),
        };
        scene
            .board
            .claim(scene.task(), &instance(1))
            .expect("claim");
        let workspace = scene.workspace.display().to_string();
        put(&scene.root.join(".zaf/slices/319"), &workspace);
        let slice = SliceName::from(SLICE);
        scene.vcs.open(&slice, &BaseName::from(BASE));
        scene.vcs.place(&slice, &scene.workspace).expect("place");
        let head = scene.commit();
        let pr = scene.open_pr(&head);
        let record = CheckRecord {
            target: format!("pr:{}:#{TASK}", pr.get()),
            verdict: Verdict::Clean,
            lines: Vec::new(),
            head: Some(head),
            at: Some("2001-01-01T00:00:00Z".to_owned()),
        };
        let file = format!(".zaf/runs/check-pr-{}.json", pr.get());
        let json = serde_json::to_string(&record).expect("json");
        put(&scene.root.join(file), &json);
        scene
    }

    fn task(&self) -> TicketId {
        TicketId::from(TASK)
    }

    fn commit(&self) -> String {
        let slice = SliceName::from(SLICE);
        self.vcs.edit(&slice, "a.rs", 1, 0, "+a").expect("edit");
        self.vcs.commit(&slice, "Work").expect("commit").to_string()
    }

    fn open_pr(&self, head: &str) -> PrNumber {
        let request = PrRequest {
            slice: SliceName::from(SLICE),
            base: BaseName::from(BASE),
            title: "Task".to_owned(),
            body: String::new(),
        };
        let pr = self.forge.open_pr(&request).expect("open pr");
        self.forge.set_head(pr, Some(head)).expect("head");
        put(&self.root.join(".zaf/slices/319.pr"), &pr.get().to_string());
        pr
    }

    fn records(&self) -> SliceRecords {
        SliceRecords::new(&self.root)
    }

    fn phase(&self) -> Phase {
        let (board, forge, vcs) = (&self.board, &self.forge, &self.vcs);
        let facts = gather_phase_facts(board, forge, vcs, &self.records(), self.task());
        derive_phase(&facts.expect("facts"))
    }

    fn done(&self) -> Result<DoneOutcome, BoardError> {
        let (board, forge, vcs) = (&self.board, &self.forge, &self.vcs);
        done(board, forge, vcs, &self.records(), self.task())
    }

    fn label(&self) -> Option<TicketState> {
        self.board.task(self.task()).expect("task").state
    }

    fn comments(&self) -> Vec<String> {
        self.board.comments(self.task())
    }
}

#[test]
fn board_done_in_land_marks_the_task() {
    let scene = Scene::landable();
    assert_eq!(scene.phase(), Phase::Land);
    assert_eq!(scene.label(), Some(TicketState::Claimed));
    assert_eq!(scene.done(), Ok(DoneOutcome::Marked));
    assert_eq!(scene.label(), Some(TicketState::Done));
    assert_eq!(scene.phase(), Phase::Done);
}

#[test]
fn board_done_after_the_recorded_pr_merged() {
    let scene = Scene::landable();
    // `slice-merge` merges the base in first, which moves the head: the task has left `land`.
    scene.commit();
    let pr = scene.records().pr(scene.task()).expect("a pull request");
    scene.forge.merge(pr).expect("merge");
    assert_eq!(scene.phase(), Phase::Build);
    assert_eq!(scene.done(), Ok(DoneOutcome::Marked));
    assert_eq!(scene.label(), Some(TicketState::Done));
}

#[test]
fn board_done_twice_is_quiet() {
    let scene = Scene::landable();
    assert_eq!(scene.done(), Ok(DoneOutcome::Marked));
    let comments = scene.comments();
    assert_eq!(scene.done(), Ok(DoneOutcome::AlreadyDone));
    assert_eq!(scene.label(), Some(TicketState::Done));
    assert_eq!(scene.comments(), comments);
}

#[test]
fn board_done_outside_land_refuses() {
    // Work past the reviewed head is `build`, and no pull request has merged.
    let scene = Scene::landable();
    scene.commit();
    let refusal = scene.done().expect_err("a refusal");
    assert_eq!(
        refusal,
        BoardError::NotInLand {
            task: scene.task(),
            phase: Phase::Build,
        }
    );
    assert_eq!(refusal.to_string(), "#319 is in build, not land");
    assert_eq!(scene.label(), Some(TicketState::Claimed));

    // A task with no recorded pull request at all.
    let bare = Scene::landable();
    std::fs::remove_file(bare.root.join(".zaf/slices/319.pr")).expect("remove");
    let refusal = bare.done().expect_err("a refusal");
    assert!(matches!(refusal, BoardError::NotInLand { .. }), "{refusal}");
    assert_eq!(bare.label(), Some(TicketState::Claimed));

    // A task with no slice at all.
    let unclaimed = Scene::landable();
    std::fs::remove_file(unclaimed.root.join(".zaf/slices/319.pr")).expect("remove");
    std::fs::remove_file(unclaimed.root.join(".zaf/slices/319")).expect("remove");
    let refusal = unclaimed.done().expect_err("a refusal");
    assert_eq!(
        refusal.to_string(),
        "#319 is in claim, not land",
        "claimed, nothing built"
    );
    assert_eq!(unclaimed.label(), Some(TicketState::Claimed));
}

#[test]
fn board_done_open_pr_refuses() {
    let scene = Scene::landable();
    scene.commit();
    let pr = scene.records().pr(scene.task()).expect("a pull request");
    assert_eq!(scene.forge.merged(pr), Ok(false));
    assert!(matches!(
        scene.done(),
        Err(BoardError::NotInLand {
            phase: Phase::Build,
            ..
        })
    ));
    assert_eq!(scene.label(), Some(TicketState::Claimed));
}

#[test]
fn a_forge_that_cannot_answer_changes_nothing() {
    // The merged read fails, outside land: the label stays.
    let scene = Scene::landable();
    scene.commit();
    scene.forge.fail_merged_reads();
    assert!(matches!(scene.done(), Err(BoardError::Port(_))));
    assert_eq!(scene.label(), Some(TicketState::Claimed));

    // A pull request the forge does not know: the phase cannot be read.
    let unknown = Scene::landable();
    put(&unknown.root.join(".zaf/slices/319.pr"), "404");
    assert!(matches!(unknown.done(), Err(BoardError::Unreadable(_))));
    assert_eq!(unknown.label(), Some(TicketState::Claimed));
}

/// A task body in the template, as a planning instance files it.
const TASK_BODY: &str = "\
Goal: Members can export their reading list as a CSV file.
Owned paths: src/export/**
Interfaces: ExportSink (defined here)
Design pages: 1001
Architecture: export, ExportSink
Acceptance:
  - an empty list exports a header row only
Tests: export_empty_list_writes_header
Security trigger: no
Model: sonnet
Size: S
Blocked by: none
";

fn new_task(title: &str, body: &str, state: TicketState) -> NewTask {
    NewTask {
        epic: TicketId::from(1),
        title: title.into(),
        body: body.into(),
        state,
        group: Some("export".into()),
    }
}

fn board_with_epic() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(1), "exports");
    board
}

#[test]
fn a_filed_task_opens_under_its_epic_and_a_second_filing_is_quiet() {
    let board = board_with_epic();
    let task = new_task("export as CSV", TASK_BODY, TicketState::Ready);
    let filed = file(&board, &task).expect("files");
    let FileOutcome::Filed(id) = filed else {
        panic!("expected a new task, got {filed:?}");
    };
    let epic = board.epic(TicketId::from(1)).expect("the epic");
    let listed = epic.tasks.iter().find(|t| t.number == id).expect("listed");
    assert_eq!(listed.state, Some(TicketState::Ready));
    assert_eq!(listed.group.as_ref().map(|g| g.as_str()), Some("export"));
    assert_eq!(file(&board, &task), Ok(FileOutcome::AlreadyFiled(id)));
    assert_eq!(
        board.epic(TicketId::from(1)).expect("the epic").tasks.len(),
        1
    );
}

#[test]
fn a_body_outside_the_template_or_a_state_a_task_cannot_open_in_files_nothing() {
    let board = board_with_epic();
    let loose = new_task("export", "Just do the export.", TicketState::Ready);
    assert!(matches!(file(&board, &loose), Err(BoardError::NotATask(_))));
    let claimed = new_task("export", TASK_BODY, TicketState::Claimed);
    assert_eq!(
        file(&board, &claimed),
        Err(BoardError::NotAnOpeningState(TicketState::Claimed))
    );
    assert!(
        board
            .epic(TicketId::from(1))
            .expect("the epic")
            .tasks
            .is_empty()
    );
}

#[test]
fn a_task_filed_under_no_open_epic_is_refused() {
    let board = FakeBoard::default();
    let task = new_task("export", TASK_BODY, TicketState::Blocked);
    assert!(matches!(file(&board, &task), Err(BoardError::Port(_))));
}
