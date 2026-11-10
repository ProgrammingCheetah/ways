use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::app::actions::RunResult;
use crate::app::actions::phase::{Phase, derive_phase};
use crate::app::actions::{
    ACTION_VAR, BASE_VAR, CHAIN_VAR, DEPTH_VAR, INSTANCE_VAR, LOG_VAR, PR_VAR, ROOT_VAR, SLICE_VAR,
    TASK_TITLE_VAR,
};
use crate::app::check::{CheckRecord, Verdict};
use crate::domain::{Instance, Ticket};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
use crate::ports::{
    BaseName, Check, ClaimOutcome, ClaimRecord, NewQuestion, PrNumber, ReviewComment,
    ScanningAlert, SliceName,
};

const TASK: u64 = 203;
const SLICE: &str = "feature/0.2.0-103-203";
const BASE: &str = "feature/0.2.0";
/// Before any record a test writes.
const LONG_AGO: &str = "2001-01-01T00:00:00Z";
/// After any record a test writes.
const LATER: &str = "2999-01-01T00:00:00Z";

fn task() -> TicketId {
    TicketId::from(TASK)
}

/// A project on disk and the fakes behind it, holding one task.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    workspace: PathBuf,
    board: FakeBoard,
    forge: FakeForge,
    vcs: FakeVcs,
}

impl Scene {
    fn new(label: TicketState) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(dir.path()).expect("real path");
        let (root, workspace) = (base.join("project"), base.join("workspace"));
        fs::create_dir_all(&root).expect("mkdir");
        fs::create_dir_all(&workspace).expect("mkdir");
        let board = FakeBoard::default();
        board.add_ticket(Ticket {
            id: task(),
            epic: Some(TicketId::from(103)),
            title: "Team Phases".into(),
            body: "".into(),
            state: Some(label),
            url: "".into(),
        });
        Self {
            _dir: dir,
            root,
            workspace,
            board,
            forge: FakeForge::default(),
            vcs: FakeVcs::default(),
        }
    }

    fn records(&self) -> SliceRecords {
        SliceRecords::new(&self.root)
    }

    fn gather(&self) -> Result<PhaseFacts, PhaseUnreadable> {
        gather_phase_facts(&self.board, &self.forge, &self.vcs, &self.records(), task())
    }

    fn phase(&self) -> Phase {
        derive_phase(&self.gather().expect("facts"))
    }

    fn claim(&self) {
        let instance: Instance = "team instance 33 on #103".parse().expect("instance");
        self.board.claim(task(), &instance).expect("claim");
    }

    /// The task's workspace record, with no slice the VCS knows.
    fn record_workspace(&self) {
        let workspace = self.workspace.display().to_string();
        write(&self.root.join(".zaf/slices/203"), &workspace);
    }

    /// The workspace record, and the slice it names.
    fn open_slice(&self) {
        self.record_workspace();
        let slice = SliceName::from(SLICE);
        self.vcs.open(&slice, &BaseName::from(BASE));
        self.vcs.place(&slice, &self.workspace).expect("place");
    }

    /// A new commit in the workspace; its id.
    fn commit(&self) -> String {
        let slice = SliceName::from(SLICE);
        self.vcs.edit(&slice, "a.rs", 1, 0, "+a").expect("edit");
        self.vcs.commit(&slice, "Work").expect("commit").to_string()
    }

    /// The task's pull request record, and the pull request it names at `head`.
    fn open_pr(&self, head: Option<&str>) -> PrNumber {
        let pr = self
            .forge
            .open_pr(&PrRequest {
                slice: SliceName::from(SLICE),
                base: BaseName::from(BASE),
                title: "Team Phases".to_owned(),
                body: String::new(),
            })
            .expect("open pr");
        self.forge.set_head(pr, head).expect("set head");
        self.record_pr(pr);
        pr
    }

    fn record_pr(&self, pr: PrNumber) {
        write(&self.root.join(".zaf/slices/203.pr"), &pr.get().to_string());
    }

    fn task_check(&self, verdict: Verdict, at: &str) {
        let file = self.root.join(".zaf/runs/check-task-203.json");
        write(&file, &stored(verdict, None, at));
    }

    fn pr_check_file(&self, pr: PrNumber) -> PathBuf {
        self.root
            .join(format!(".zaf/runs/check-pr-{}.json", pr.get()))
    }

    fn pr_check(&self, pr: PrNumber, verdict: Verdict, head: &str, at: &str) {
        write(&self.pr_check_file(pr), &stored(verdict, Some(head), at));
    }

    /// The workspace at the pull request's head, with `verdict` the result for that head.
    fn reviewed(verdict: Verdict) -> Self {
        let scene = Self::new(TicketState::Claimed);
        scene.open_slice();
        let head = scene.commit();
        let pr = scene.open_pr(Some(&head));
        scene.pr_check(pr, verdict, &head, LONG_AGO);
        scene
    }
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

/// A check record as `zaf check` stores it.
fn stored(verdict: Verdict, head: Option<&str>, at: &str) -> String {
    let record = CheckRecord {
        target: "#203".to_owned(),
        verdict,
        lines: Vec::new(),
        head: head.map(str::to_owned),
        at: Some(at.to_owned()),
    };
    serde_json::to_string_pretty(&record).expect("json")
}

/// The name of the variable that makes a run of the test the child; it names the child's output.
const CHILD_VAR: &str = "ZAF_TEST_PHASE_CHILD_OUTPUT";

/// Every variable the children set or clear: zaf's own, and names a guard might be tempted by.
const VARIABLES: [&str; 14] = [
    INSTANCE_VAR,
    PR_VAR,
    SLICE_VAR,
    BASE_VAR,
    TASK_TITLE_VAR,
    ACTION_VAR,
    CHAIN_VAR,
    DEPTH_VAR,
    LOG_VAR,
    ROOT_VAR,
    "ZAF_VERSION",
    "ZAF_PHASE",
    "ZAF_TASK",
    "ZAF_HEAD",
];

/// What every phase's scene derives, in lifecycle order.
const EVERY_PHASE: &str = "unclaimed,claim,build,check,publish,review,land,done";

/// One scene per phase, each derived, in lifecycle order.
fn every_phase() -> String {
    let claim = Scene::new(TicketState::Ready);
    claim.claim();
    let build = Scene::new(TicketState::Claimed);
    build.open_slice();
    let check = Scene::new(TicketState::Claimed);
    check.open_slice();
    check.task_check(Verdict::Clean, LONG_AGO);
    let publish = Scene::new(TicketState::Claimed);
    publish.open_slice();
    let head = publish.commit();
    publish.open_pr(Some(&head));
    let scenes = [
        Scene::new(TicketState::Ready),
        claim,
        build,
        check,
        publish,
        Scene::reviewed(Verdict::StepFailed),
        Scene::reviewed(Verdict::Clean),
        Scene::new(TicketState::Done),
    ];
    let phases: Vec<String> = scenes
        .iter()
        .map(|scene| scene.phase().to_string())
        .collect();
    phases.join(",")
}

/// Runs this test again in a process with `set` as its only variables of [`VARIABLES`]:
/// the `ZAF_INSTANCE` it saw, then the phases it derived.
fn phases_under(set: &[(&str, &str)]) -> (String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = dir.path().join("child");
    let (_crate_name, tests) = module_path!().split_once("::").expect("a crate's module");
    let test = format!("{tests}::environment_cannot_change_the_phase");
    let mut child = Command::new(env::current_exe().expect("this test binary"));
    child.args([&test, "--exact"]);
    for name in VARIABLES {
        child.env_remove(name);
    }
    child.env(CHILD_VAR, &output);
    child.envs(set.iter().copied());
    let ran = child.output().expect("run the child");
    assert!(ran.status.success(), "child under {set:?}: {ran:?}");
    let printed = fs::read_to_string(&output).expect("the child ran this test");
    let (seen, phases) = printed.split_once('\n').expect("two lines");
    (seen.to_owned(), phases.to_owned())
}

#[test]
fn environment_cannot_change_the_phase() {
    if let Some(output) = env::var_os(CHILD_VAR) {
        let seen = env::var(INSTANCE_VAR).unwrap_or_default();
        fs::write(output, format!("{seen}\n{}", every_phase())).expect("write");
        return;
    }
    // Each one aims at the merge path: as the caller a mode lets through, and naming a phase.
    let environments: [&[(&str, &str)]; 4] = [
        &[],
        &[
            (INSTANCE_VAR, "team instance 1 on #103"),
            ("ZAF_PHASE", "land"),
            ("ZAF_TASK", "203"),
            ("ZAF_HEAD", "fake-1"),
            (PR_VAR, "1"),
            (SLICE_VAR, SLICE),
            (BASE_VAR, BASE),
            ("ZAF_VERSION", "0.2.0"),
        ],
        &[
            (INSTANCE_VAR, "release instance 7 on #103"),
            ("ZAF_PHASE", "done"),
            (PR_VAR, "999"),
            (DEPTH_VAR, "3"),
        ],
        &[(INSTANCE_VAR, ""), ("ZAF_PHASE", "unclaimed")],
    ];
    for set in environments {
        let (seen, phases) = phases_under(set);
        let instance = set
            .iter()
            .find(|(name, _)| *name == INSTANCE_VAR)
            .map_or("", |(_, value)| value);
        assert_eq!(seen, instance, "the child saw its environment");
        assert_eq!(phases, EVERY_PHASE, "under {set:?}");
    }
}

#[test]
fn nothing_the_guard_reads_is_read_from_the_environment() {
    let sources = [
        include_str!("../gather.rs"),
        include_str!("../../phase.rs"),
        include_str!("../../records.rs"),
    ];
    for source in sources {
        assert!(!source.contains("env::"), "{source}");
        assert!(!source.contains("option_env!"), "{source}");
    }
}

#[test]
fn the_facts_come_from_their_sources() {
    let scene = Scene::reviewed(Verdict::Clean);
    scene.task_check(Verdict::Clean, LONG_AGO);
    let facts = scene.gather().expect("facts");
    let head = scene.vcs.latest_commit(&scene.workspace).expect("commit");
    assert!(!facts.done);
    assert!(facts.claimed);
    assert_eq!(
        facts.workspace.expect("a workspace").latest_commit,
        Some(head.to_string())
    );
    let pr = facts.pr.expect("a pull request");
    assert_eq!(pr.head, Some(head.to_string()));
    let written = fs::metadata(scene.root.join(".zaf/slices/203.pr"))
        .and_then(|metadata| metadata.modified())
        .expect("mtime");
    assert_eq!(pr.recorded_at, Some(written));
    let task_check = facts.task_check.expect("a task check");
    assert_eq!(task_check.verdict, Verdict::Clean);
    assert_eq!(task_check.head, None);
    let pr_check = facts.pr_check.expect("a pull request check");
    assert_eq!(pr_check.head, Some(head.to_string()));
}

#[test]
fn nothing_recorded_gathers_no_records() {
    let facts = Scene::new(TicketState::Ready).gather().expect("facts");
    assert_eq!(facts, PhaseFacts::default());
}

#[test]
fn the_done_label_is_done_whatever_the_records_say() {
    let scene = Scene::reviewed(Verdict::Clean);
    scene
        .board
        .set_state(task(), TicketState::Done)
        .expect("label");
    assert_eq!(scene.phase(), Phase::Done);
}

#[test]
fn a_claimed_label_needs_the_claim_comment() {
    let without = Scene::new(TicketState::Claimed);
    let facts = without.gather().expect("facts");
    assert!(facts.claimed);
    assert!(!facts.claim_comment);
    assert_eq!(without.phase(), Phase::Unclaimed);

    let with = Scene::new(TicketState::Ready);
    with.claim();
    assert!(with.gather().expect("facts").claim_comment);
    assert_eq!(with.phase(), Phase::Claim);
}

#[test]
fn another_tasks_lease_is_not_this_ones() {
    let scene = Scene::new(TicketState::Claimed);
    let other = TicketId::from(TASK + 1);
    scene.board.add_ticket(Ticket {
        id: other,
        epic: None,
        title: "Another".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
    });
    let instance: Instance = "team instance 33 on #103".parse().expect("instance");
    scene.board.claim(other, &instance).expect("claim");
    assert!(!scene.gather().expect("facts").claim_comment);
}

#[test]
fn a_lease_without_the_claimed_label_does_not_count() {
    let scene = Scene::new(TicketState::Ready);
    scene.claim();
    scene
        .board
        .set_state(task(), TicketState::Question)
        .expect("label");
    let facts = scene.gather().expect("facts");
    assert!(!facts.claimed);
    assert!(!facts.claim_comment);
    assert_eq!(derive_phase(&facts), Phase::Unclaimed);
}

#[test]
fn a_workspace_ahead_of_the_head_is_not_land() {
    let scene = Scene::reviewed(Verdict::Clean);
    assert_eq!(scene.phase(), Phase::Land);
    scene.commit();
    assert_eq!(scene.phase(), Phase::Build);
}

#[test]
fn uncommitted_work_leaves_the_latest_commit_where_it_was() {
    let scene = Scene::reviewed(Verdict::Clean);
    scene
        .vcs
        .edit(&SliceName::from(SLICE), "b.rs", 1, 0, "+b")
        .expect("edit");
    assert_eq!(scene.phase(), Phase::Land);
}

#[test]
fn a_workspace_record_that_does_not_hold_is_no_workspace() {
    let scene = Scene::new(TicketState::Claimed);
    write(&scene.root.join(".zaf/slices/203"), "relative/path");
    assert_eq!(scene.gather().expect("facts").workspace, None);
    assert_eq!(scene.phase(), Phase::Unclaimed);
}

#[test]
fn a_clean_result_for_an_older_head_is_no_result() {
    let scene = Scene::new(TicketState::Claimed);
    scene.open_slice();
    let head = scene.commit();
    let pr = scene.open_pr(Some(&head));
    scene.pr_check(pr, Verdict::Clean, "older", LONG_AGO);
    assert_eq!(scene.phase(), Phase::Publish);
}

#[test]
fn a_forge_that_reports_no_head_is_no_result() {
    let scene = Scene::new(TicketState::Claimed);
    scene.open_slice();
    let head = scene.commit();
    let pr = scene.open_pr(None);
    scene.pr_check(pr, Verdict::Clean, &head, LONG_AGO);
    assert_eq!(scene.gather().expect("facts").pr.expect("pr").head, None);
    assert_eq!(scene.phase(), Phase::Publish);
}

#[test]
fn a_malformed_result_is_no_result() {
    let scene = Scene::new(TicketState::Claimed);
    scene.open_slice();
    let head = scene.commit();
    let pr = scene.open_pr(Some(&head));
    write(&scene.pr_check_file(pr), "{ not a record");
    assert_eq!(scene.gather().expect("facts").pr_check, None);
    assert_eq!(scene.phase(), Phase::Publish);
}

#[test]
fn the_pr_records_time_is_held_against_the_task_check() {
    let scene = Scene::new(TicketState::Claimed);
    scene.open_slice();
    let head = scene.commit();
    scene.open_pr(Some(&head));
    scene.task_check(Verdict::Clean, LONG_AGO);
    assert_eq!(scene.phase(), Phase::Publish);
    scene.task_check(Verdict::Clean, LATER);
    assert_eq!(scene.phase(), Phase::Check);
    scene.task_check(Verdict::StepFailed, LATER);
    assert_eq!(scene.phase(), Phase::Build);
}

/// What a failed read answers, for the tests below.
fn broken() -> PortError {
    PortError::Backend("the backend is down".to_owned())
}

/// The board of a scene, but for the reads it is told to fail.
struct BrokenBoard<'a> {
    inner: &'a FakeBoard,
    task: bool,
    claims: bool,
}

impl Board for BrokenBoard<'_> {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.inner.ready()
    }

    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        if self.task {
            return Err(broken());
        }
        self.inner.task(number)
    }

    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        self.inner.claim(task, instance)
    }

    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.inner.heartbeat(task, instance)
    }

    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        if self.claims {
            return Err(broken());
        }
        self.inner.claims()
    }

    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.inner.release(task, instance)
    }

    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        self.inner.set_state(task, label)
    }

    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.inner.comment(issue, body)
    }

    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.inner.report(task, instance, body)
    }

    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.inner.questions()
    }

    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        self.inner.open_question(question)
    }

    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        self.inner.record_ruling(question, ruling)
    }
}

/// The forge of a scene, but for a head read that fails.
struct BrokenForge<'a>(&'a FakeForge);

impl Forge for BrokenForge<'_> {
    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        self.0.checks(pr)
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        self.0.review_comments(pr)
    }

    fn head(&self, _pr: PrNumber) -> Result<Option<String>, PortError> {
        Err(broken())
    }

    fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
        self.0.merged(pr)
    }

    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.0.last_changed_file(pr)
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        self.0.scanning_alerts(pr)
    }
}

/// A scene that would be `land`, so that a failed read has a phase to fall back from.
fn landable() -> Scene {
    let scene = Scene::new(TicketState::Ready);
    scene.claim();
    scene.open_slice();
    let head = scene.commit();
    let pr = scene.open_pr(Some(&head));
    scene.pr_check(pr, Verdict::Clean, &head, LONG_AGO);
    assert_eq!(scene.phase(), Phase::Land);
    scene
}

/// The refusal a failed read comes to: `NOT_RUN`, exit 125, naming the task and the cause.
fn assert_refused(refusal: &PhaseUnreadable) {
    assert_eq!(refusal.task, task());
    let outcome = refusal.not_run("slice-merge", None);
    assert_eq!(outcome.result, RunResult::NotRun);
    assert_eq!(outcome.exit, 125);
    assert_eq!(outcome.log, None);
    let headline = outcome.headline().expect("a refusal prints");
    assert!(headline.starts_with("NOT_RUN slice-merge: "), "{headline}");
    assert!(headline.contains("#203"), "{headline}");
}

#[test]
fn a_board_that_cannot_answer_refuses() {
    let scene = landable();
    let board = BrokenBoard {
        inner: &scene.board,
        task: true,
        claims: false,
    };
    let refusal = gather_phase_facts(&board, &scene.forge, &scene.vcs, &scene.records(), task())
        .expect_err("a refusal");
    assert_eq!(refusal.source, broken());
    assert_refused(&refusal);
}

#[test]
fn a_task_the_board_does_not_know_refuses() {
    let scene = Scene::new(TicketState::Ready);
    let unknown = TicketId::from(TASK + 1);
    let refusal = gather_phase_facts(
        &scene.board,
        &scene.forge,
        &scene.vcs,
        &scene.records(),
        unknown,
    )
    .expect_err("a refusal");
    assert!(matches!(refusal.source, PortError::NotFound(_)));
    assert_eq!(refusal.task, unknown);
}

#[test]
fn claims_the_board_cannot_list_refuse() {
    let scene = landable();
    let board = BrokenBoard {
        inner: &scene.board,
        task: false,
        claims: true,
    };
    let refusal = gather_phase_facts(&board, &scene.forge, &scene.vcs, &scene.records(), task())
        .expect_err("a refusal");
    assert_eq!(refusal.source, broken());
    assert_refused(&refusal);
}

#[test]
fn claims_are_not_read_for_a_task_that_is_not_claimed() {
    let scene = Scene::new(TicketState::Ready);
    let board = BrokenBoard {
        inner: &scene.board,
        task: false,
        claims: true,
    };
    let facts = gather_phase_facts(&board, &scene.forge, &scene.vcs, &scene.records(), task())
        .expect("facts");
    assert!(!facts.claim_comment);
}

#[test]
fn a_forge_that_cannot_answer_refuses() {
    let scene = landable();
    let forge = BrokenForge(&scene.forge);
    let refusal = gather_phase_facts(&scene.board, &forge, &scene.vcs, &scene.records(), task())
        .expect_err("a refusal");
    assert_eq!(refusal.source, broken());
    assert_refused(&refusal);
}

#[test]
fn a_pull_request_the_forge_does_not_know_refuses() {
    let scene = Scene::new(TicketState::Claimed);
    scene.open_slice();
    scene.record_pr(PrNumber::from(404));
    let refusal = scene.gather().expect_err("a refusal");
    assert!(matches!(refusal.source, PortError::NotFound(_)));
    assert_refused(&refusal);
}

#[test]
fn a_latest_commit_that_cannot_be_read_refuses() {
    // The workspace record holds and the pull request is clean at "abc", but the VCS knows
    // no slice there: this is a failed read, so it is never a phase.
    let scene = Scene::new(TicketState::Claimed);
    scene.record_workspace();
    let pr = scene.open_pr(Some("abc"));
    scene.pr_check(pr, Verdict::Clean, "abc", LONG_AGO);
    let refusal = scene.gather().expect_err("a refusal");
    assert!(matches!(refusal.source, PortError::NotFound(_)));
    assert_refused(&refusal);
}

#[test]
fn a_refusal_names_the_read_that_failed() {
    let scene = landable();
    let forge = BrokenForge(&scene.forge);
    let refusal = gather_phase_facts(&scene.board, &forge, &scene.vcs, &scene.records(), task())
        .expect_err("a refusal");
    assert_eq!(
        refusal.to_string(),
        "#203's phase cannot be read: backend failed: the backend is down"
    );
}

#[test]
fn a_missing_workspace_is_an_absent_fact_not_a_refusal() {
    let scene = landable();
    fs::remove_dir_all(&scene.workspace).expect("remove");
    let facts = scene.gather().expect("facts");
    assert_eq!(facts.workspace, None);
    // The workspace is not at the head, so the task is not `land`.
    assert_eq!(derive_phase(&facts), Phase::Build);
}
