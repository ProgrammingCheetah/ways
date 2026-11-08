use std::sync::Arc;

use super::*;
use crate::app::status;
use crate::domain::{InstanceNumber, TaskLabel};
use crate::fakes::{FakeBoard, FakeRunner};
use crate::ports::{ClaimOutcome, ClaimRecord, NewQuestion, RunOutput, Ticket};

/// Lends one fake board to every launch thread.
struct SharedBoard(Arc<FakeBoard>);

impl Board for SharedBoard {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.0.ready()
    }
    fn task(&self, number: IssueNumber) -> Result<Ticket, PortError> {
        self.0.task(number)
    }
    fn claim(&self, task: IssueNumber, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        self.0.claim(task, instance)
    }
    fn heartbeat(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError> {
        self.0.heartbeat(task, instance)
    }
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        self.0.claims()
    }
    fn release(&self, task: IssueNumber, instance: &Instance) -> Result<(), PortError> {
        self.0.release(task, instance)
    }
    fn label(&self, task: IssueNumber, label: TaskLabel) -> Result<(), PortError> {
        self.0.label(task, label)
    }
    fn comment(&self, issue: IssueNumber, body: &str) -> Result<(), PortError> {
        self.0.comment(issue, body)
    }
    fn report(&self, task: IssueNumber, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.0.report(task, instance, body)
    }
    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.0.questions()
    }
    fn open_question(&self, question: &NewQuestion) -> Result<IssueNumber, PortError> {
        self.0.open_question(question)
    }
    fn record_ruling(
        &self,
        question: IssueNumber,
        ruling: &str,
    ) -> Result<Option<IssueNumber>, PortError> {
        self.0.record_ruling(question, ruling)
    }
}

/// Lends one fake runner to every launch thread.
struct SharedRunner(Arc<FakeRunner>);

impl Runner for SharedRunner {
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError> {
        self.0.run(request)
    }
}

const MANIFEST: &str = r#"
[project]
name = "bookshelf"

[board]
kind = "github-issues"
repo = "example/bookshelf-board"

[spawn]
slots = 2

[roles.team]
model = "opus"
effort = "xhigh"
profile = "build"
turns = 200
batch = 2
"#;

const OWN_PID: u32 = 4242;

fn manifest() -> Manifest {
    MANIFEST.parse().expect("manifest parses")
}

fn ticket(number: u64, epic: Option<u64>, body: &str, label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(number),
        epic: epic.map(IssueNumber::from),
        title: format!("Issue {number}"),
        body: body.to_owned(),
        label: Some(label),
    }
}

fn epic(number: u64) -> Ticket {
    Ticket {
        label: None,
        ..ticket(number, None, "", TaskLabel::Ready)
    }
}

/// Epic #2 with ready tasks #3, #4, #5 (#5 blocked by the open #6) and a
/// closed blocker #7 on #4; epic #12 with ready task #13.
fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(epic(2));
    board.add_ticket(ticket(3, Some(2), "Blocked by: —", TaskLabel::Ready));
    board.add_ticket(ticket(4, Some(2), "Blocked by: #7", TaskLabel::Ready));
    board.add_ticket(ticket(5, Some(2), "Blocked by: #6", TaskLabel::Ready));
    board.add_ticket(ticket(6, Some(2), "", TaskLabel::Blocked));
    board.add_ticket(ticket(7, Some(2), "", TaskLabel::Done));
    board.add_ticket(epic(12));
    board.add_ticket(ticket(13, Some(12), "", TaskLabel::Ready));
    board
}

fn context(state_dir: &Path) -> LaunchContext {
    LaunchContext {
        state_dir: state_dir.to_owned(),
        project: "bookshelf".to_owned(),
        profile: Profile::Build,
        env: BTreeMap::new(),
    }
}

fn team() -> RoleName {
    RoleName::from(TEAM_ROLE)
}

fn instance(number: u32, epic: u64) -> Instance {
    Instance {
        role: team(),
        epic: IssueNumber::from(epic),
        number: InstanceNumber::from(number),
    }
}

#[test]
fn a_launch_through_the_runner_records_launch_and_exit_and_frees_its_slot() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let runner = FakeRunner::default();
    runner.record(
        team(),
        RunOutput {
            exit_code: 0,
            transcript: "merged #3 and #4\n".to_owned(),
        },
    );

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let first = &prepared.plan.launches[0];
    assert_eq!(first.instance, instance(1, 2));
    assert_eq!(first.tasks, [IssueNumber::from(3), IssueNumber::from(4)]);

    // Reserved before running: `zaf status` reads the same file.
    let reserved = load(&state_dir).expect("loads");
    assert_eq!(reserved.next_instance, 3);
    assert_eq!(
        reserved.instances(),
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    let screen = status::status(&board, &state_dir, 2).expect("status reads the state");
    assert_eq!(screen.running.len(), 3);
    assert_eq!(screen.running[0].task, 3);
    assert_eq!(screen.running[0].pid, Some(OWN_PID));

    let done = launch(&board, &runner, &context(&state_dir), first).expect("launches");
    assert_eq!(done.exit, LaunchExit::Code(0));
    assert_eq!(
        board.comments(IssueNumber::from(2)),
        [
            "<!-- zaf:launch team instance 1 on #2 -->",
            "<!-- zaf:exit team instance 1 on #2 0 -->",
        ]
    );
    let run_dir = state_dir.join(RUNS_DIR).join("team-instance-1-on-2");
    assert_eq!(done.run_dir, run_dir);
    assert_eq!(
        std::fs::read_to_string(run_dir.join(REPORT_FILE)).expect("report"),
        "merged #3 and #4\n"
    );

    let requests = runner.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.role, team());
    assert_eq!(request.profile, Profile::Build);
    assert_eq!(request.dir, run_dir);
    let brief = run_dir.join(BRIEF_FILE);
    assert_eq!(
        request.prompt,
        format!(
            "You are team instance 1 on #2, the team instance for epic #2 of project bookshelf. \
             Tasks: #3 #4. Read {} first and follow it exactly.",
            brief.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&brief).expect("brief"), TEAM_BRIEF);

    let after = load(&state_dir).expect("loads");
    assert_eq!(after.instances(), ["team instance 2 on #12"]);
    assert_eq!(after.next_instance, 3);
}

#[test]
fn a_launch_marks_its_start_in_the_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let mut state = load(&state_dir).expect("loads");
    assert!(
        state
            .launches
            .iter()
            .all(|launch| launch.started_at.is_none())
    );

    // A runner with nothing recorded is unavailable: the exit is an error, the slot is freed.
    let runner = FakeRunner::default();
    let done = launch(
        &board,
        &runner,
        &context(&state_dir),
        &prepared.plan.launches[1],
    )
    .expect("records the failed run");
    assert!(matches!(done.exit, LaunchExit::Error(_)));
    assert_eq!(
        board.comments(IssueNumber::from(12)),
        [
            "<!-- zaf:launch team instance 2 on #12 -->",
            "<!-- zaf:exit team instance 2 on #12 error -->",
        ]
    );
    state = load(&state_dir).expect("loads");
    assert_eq!(state.instances(), ["team instance 1 on #2"]);
}

/// A runner whose every run fails with the given error.
struct FailingRunner(PortError);

impl Runner for FailingRunner {
    fn run(&self, _request: &RunRequest) -> Result<RunOutput, PortError> {
        Err(self.0.clone())
    }
}

fn timeout() -> FailingRunner {
    FailingRunner(PortError::TimedOut {
        bound: std::time::Duration::from_secs(60),
        command: "claude -p".to_owned(),
    })
}

#[test]
fn timeout_text_in_another_error_is_not_a_timeout() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let first = &prepared.plan.launches[0];
    let task = IssueNumber::from(3);
    board.claim(task, &first.instance).expect("claims #3");

    let lookalike = FailingRunner(PortError::Unavailable(
        "run timed out after 60s: claude -p".to_owned(),
    ));
    let done = launch(&board, &lookalike, &context(&state_dir), first).expect("records the run");

    assert!(!done.exit.timed_out());
    assert_eq!(done.exit.status(), "error");
    assert!(matches!(done.exit, LaunchExit::Error(_)));
    assert_eq!(board.claims().expect("claims").len(), 1, "claims are kept");
}

#[test]
fn timed_out_launch_releases_its_claims() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let first = &prepared.plan.launches[0];
    let (stuck, waiting, other) = (
        IssueNumber::from(3),
        IssueNumber::from(4),
        IssueNumber::from(13),
    );
    board.claim(stuck, &first.instance).expect("claims #3");
    board.claim(waiting, &first.instance).expect("claims #4");
    board
        .label(waiting, TaskLabel::Question)
        .expect("#4 waits on a question");
    // Held by another instance: stays held.
    board.claim(other, &instance(2, 12)).expect("claims #13");

    let done = launch(&board, &timeout(), &context(&state_dir), first).expect("records the run");

    assert!(done.exit.timed_out());
    assert_eq!(done.exit.status(), "timeout");
    let epic_comments = board.comments(IssueNumber::from(2));
    assert!(
        epic_comments.contains(&"<!-- zaf:exit team instance 1 on #2 timeout -->".to_owned()),
        "{epic_comments:?}"
    );
    let label = |task| board.task(task).expect("task").label;
    assert_eq!(label(stuck), Some(TaskLabel::Ready));
    assert_eq!(label(waiting), Some(TaskLabel::Question));
    assert_eq!(label(other), Some(TaskLabel::Claimed));
    for task in [stuck, waiting] {
        let comments = board.comments(task);
        assert_eq!(comments.len(), 1, "{comments:?}");
        assert!(
            comments[0].starts_with("released from team instance 1 on #2 at "),
            "{comments:?}"
        );
    }
    assert!(board.comments(other).is_empty());
    let claims = board.claims().expect("claims");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].task, other);
    assert_eq!(
        load(&state_dir).expect("loads").instances(),
        ["team instance 2 on #12"],
        "the slot is freed"
    );
}

#[test]
fn a_launch_that_did_not_time_out_keeps_its_claims() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let first = &prepared.plan.launches[0];
    let task = IssueNumber::from(3);
    board.claim(task, &first.instance).expect("claims #3");

    let exited = FakeRunner::default();
    exited.record(
        team(),
        RunOutput {
            exit_code: 1,
            transcript: String::new(),
        },
    );
    let done = launch(&board, &exited, &context(&state_dir), first).expect("launches");
    assert_eq!(done.exit, LaunchExit::Code(1));
    assert!(!done.exit.timed_out());

    // A runner that could not run is an `error`, not a timeout.
    let missing = FailingRunner(PortError::Unavailable("no claude".to_owned()));
    let done = launch(&board, &missing, &context(&state_dir), first).expect("records the run");
    assert_eq!(done.exit.status(), "error");

    assert_eq!(board.claims().expect("claims").len(), 1);
    assert!(board.comments(task).is_empty());
}

#[test]
fn the_projects_own_brief_wins_over_the_pack() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let own = state_dir.join(ROLES_DIR).join("team.md");
    std::fs::create_dir_all(own.parent().expect("parent")).expect("dirs");
    std::fs::write(&own, "our own brief").expect("writes");
    let board = board();
    let runner = FakeRunner::default();
    runner.record(
        team(),
        RunOutput {
            exit_code: 3,
            transcript: String::new(),
        },
    );
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let done = launch(
        &board,
        &runner,
        &context(&state_dir),
        &prepared.plan.launches[0],
    )
    .expect("launches");
    assert_eq!(done.exit, LaunchExit::Code(3));
    assert!(
        runner.requests()[0]
            .prompt
            .contains(&own.display().to_string())
    );
    assert!(!done.run_dir.join(BRIEF_FILE).exists());
    assert!(
        board
            .comments(IssueNumber::from(2))
            .contains(&"<!-- zaf:exit team instance 1 on #2 3 -->".to_owned())
    );
}

#[test]
fn prepare_skips_blocked_and_claimed_tasks_read_from_the_board() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let holder = instance(9, 2);
    board
        .claim(IssueNumber::from(3), &holder)
        .expect("claims #3");
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let plan = &prepared.plan;
    assert_eq!(plan.launches[0].tasks, [IssueNumber::from(4)]);
    assert!(
        plan.launches
            .iter()
            .all(|launch| !launch.tasks.contains(&IssueNumber::from(3)))
    );
    assert_eq!(
        plan.tasks,
        [(
            IssueNumber::from(5),
            TaskSkip::Blocked(vec![IssueNumber::from(6)])
        )]
    );
    assert!(plan.render_text().contains("#5 skipped: blocked by #6"));
}

#[test]
fn a_missing_blocker_is_dead() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_ticket(epic(2));
    board.add_ticket(ticket(3, Some(2), "Blocked by: #99", TaskLabel::Ready));

    let prepared = plan_only(&board, &state_dir, &manifest(), &|_| true).expect("plans");
    let text = prepared.plan.render_text();
    assert!(text.contains("dead blocker #99 on #3: not an issue\n"));
    assert!(text.contains("#3 skipped: blocked by #99\n"));

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    assert!(
        prepared
            .plan
            .render_text()
            .contains("dead blocker #99 on #3: not an issue\n")
    );
    assert_eq!(
        prepared.plan.to_json(&[], &[])["dead_blockers"],
        serde_json::json!([{ "blocker": 99, "on": 3, "reason": "not an issue" }])
    );
}

#[test]
fn a_blocker_closed_without_done_is_dead() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_ticket(epic(2));
    board.add_ticket(ticket(3, Some(2), "Blocked by: #6 #7", TaskLabel::Ready));
    board.add_ticket(ticket(6, Some(2), "", TaskLabel::Blocked));
    board.close(IssueNumber::from(6));
    board.add_ticket(ticket(7, Some(2), "", TaskLabel::Done));
    board.close(IssueNumber::from(7));

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let text = prepared.plan.render_text();
    assert!(text.contains("dead blocker #6 on #3: closed without done\n"));
    assert!(
        !text.contains("dead blocker #7"),
        "a closed issue labelled done is closed for good"
    );
    assert!(text.contains("#3 skipped: blocked by #6\n"));
}

#[test]
fn the_snapshot_reads_a_cycle_through_open_tasks_that_are_not_ready() {
    let board = FakeBoard::default();
    board.add_ticket(epic(2));
    board.add_ticket(ticket(3, Some(2), "Blocked by: #4", TaskLabel::Ready));
    board.add_ticket(ticket(4, Some(2), "Blocked by: #5", TaskLabel::Blocked));
    board.add_ticket(ticket(5, Some(2), "Blocked by: #4", TaskLabel::Blocked));

    let read = snapshot(&board).expect("snapshot");
    assert_eq!(
        read.open.keys().copied().collect::<Vec<_>>(),
        [IssueNumber::from(4), IssueNumber::from(5)]
    );
    let text = tick::plan(
        &read,
        &SpawnState::default(),
        Level::Green,
        Settings::from(&manifest()),
    )
    .render_text();
    assert!(text.contains("#3 skipped: blocked by #4 (in a blocker cycle)\n"));
    assert!(text.contains("blocker cycle: #4 → #5 → #4\n"));
}

#[test]
fn a_second_tick_sees_the_reserved_slots_taken() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("first tick");
    let second = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("second tick");
    assert!(second.plan.launches.is_empty());
    assert_eq!(second.plan.slots.free, 0);
}

#[test]
fn a_launch_whose_process_is_gone_is_recorded_as_lost() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, 100).expect("first tick");
    let second =
        prepare(&board, &state_dir, &manifest(), &|pid| pid != 100, OWN_PID).expect("second tick");
    assert_eq!(
        second.lost,
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    assert!(
        board
            .comments(IssueNumber::from(2))
            .contains(&"<!-- zaf:exit team instance 1 on #2 lost -->".to_owned())
    );
    assert_eq!(second.plan.launches[0].instance, instance(3, 2));
}

#[test]
fn a_lost_instances_tasks_are_released_and_a_question_is_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, 100).expect("first tick");
    let lost = instance(1, 2);
    let (waiting, stuck, other) = (
        IssueNumber::from(4),
        IssueNumber::from(3),
        IssueNumber::from(13),
    );
    board.claim(stuck, &lost).expect("claims #3");
    board.claim(waiting, &lost).expect("claims #4");
    board
        .label(waiting, TaskLabel::Question)
        .expect("#4 waits on a question");
    // Held by an instance that is not lost: stays held.
    board.claim(other, &instance(9, 12)).expect("claims #13");

    let second =
        prepare(&board, &state_dir, &manifest(), &|pid| pid != 100, OWN_PID).expect("second tick");

    let expected = vec![
        Released {
            task: stuck,
            instance: lost.clone(),
        },
        Released {
            task: waiting,
            instance: lost,
        },
    ];
    assert_eq!(second.released, expected);
    let label = |task| board.task(task).expect("task").label;
    assert_eq!(label(stuck), Some(TaskLabel::Ready));
    assert_eq!(label(waiting), Some(TaskLabel::Question));
    assert_eq!(label(other), Some(TaskLabel::Claimed));
    for task in [stuck, waiting] {
        let comments = board.comments(task);
        assert_eq!(comments.len(), 1, "{comments:?}");
        assert!(
            comments[0].starts_with("released from team instance 1 on #2 at "),
            "{comments:?}"
        );
    }
    assert!(board.comments(other).is_empty());
    let claims = board.claims().expect("claims");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].task, other);
}

#[test]
fn a_tick_with_nothing_lost_releases_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, 100).expect("first tick");
    board
        .claim(IssueNumber::from(3), &instance(1, 2))
        .expect("claims #3");
    let second = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("second tick");
    assert!(second.released.is_empty());
    assert_eq!(board.claims().expect("claims").len(), 1);
}

#[test]
fn the_guard_file_sets_the_level_and_its_absence_is_green() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(read_guard(dir.path()).expect("absent"), Level::Green);
    std::fs::write(
        dir.path().join(GUARD_FILE),
        r#"{"level": "orange", "percent": 88}"#,
    )
    .expect("writes");
    assert_eq!(read_guard(dir.path()).expect("reads"), Level::Orange);
    std::fs::write(
        dir.path().join(GUARD_FILE),
        r#"{"level": "red", "percent": 97}"#,
    )
    .expect("writes");
    let board = board();
    let state_dir = dir.path();
    let prepared = prepare(&board, state_dir, &manifest(), &|_| true, OWN_PID).expect("tick");
    assert!(prepared.plan.launches.is_empty());
    std::fs::write(dir.path().join(GUARD_FILE), "{").expect("writes");
    assert!(read_guard(dir.path()).is_err());
}

#[test]
fn the_unbuilt_branches_print_skipped_and_launch_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = FakeBoard::default();
    board.add_ticket(epic(2));
    let prepared = prepare(&board, dir.path(), &manifest(), &|_| true, OWN_PID).expect("prepares");
    assert!(prepared.plan.launches.is_empty());
    let text = prepared.plan.render_text();
    for branch in ["planning", "challenger", "release"] {
        assert!(
            text.contains(&format!("{branch}: skipped: not built")),
            "{text}"
        );
    }
    let json = prepared.plan.to_json(&[], &[]);
    assert_eq!(json["skipped"]["branches"][0]["reason"], "not built");
    assert_eq!(json["launched"], serde_json::json!([]));
}

#[test]
fn json_names_every_launch_and_every_skipped_epic() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    let manifest: Manifest = MANIFEST
        .replace("slots = 2", "slots = 1")
        .parse()
        .expect("manifest parses");
    let prepared = prepare(&board, dir.path(), &manifest, &|_| true, OWN_PID).expect("tick");
    let json = prepared.plan.to_json(&[], &[]);
    assert_eq!(json["skipped"]["epics"][0]["epic"], 12);
    assert_eq!(json["skipped"]["epics"][0]["reason"], "no_free_slot");
    assert_eq!(json["skipped"]["tasks"][0]["reason"], "blocked");
    assert_eq!(
        json["skipped"]["tasks"][0]["detail"],
        serde_json::json!([6])
    );
    assert_eq!(json["slots"]["free"], 1);
}

#[test]
fn the_slug_turns_spaces_and_hashes_into_single_dashes() {
    assert_eq!(slug(&instance(12, 16)), "team-instance-12-on-16");
}

#[test]
fn the_team_profile_comes_from_the_manifest() {
    let lean: Manifest = MANIFEST
        .replace("profile = \"build\"", "profile = \"lean\"")
        .parse()
        .expect("manifest parses");
    assert_eq!(team_profile(&lean), Profile::Lean);
    assert_eq!(Settings::from(&lean).batch, 2);
}

#[test]
fn a_launch_puts_its_environment_on_the_request() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let runner = FakeRunner::default();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let context = LaunchContext {
        env: BTreeMap::from([
            (
                "ZAF_INSTANCE".to_owned(),
                "team instance 1 on #2".to_owned(),
            ),
            ("ZAF_VERSION".to_owned(), "0.3.0".to_owned()),
        ]),
        ..context(&state_dir)
    };
    launch(&board, &runner, &context, &prepared.plan.launches[0]).expect("records the run");
    let requests = runner.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].env, context.env);
}

#[test]
fn spawn_launches_with_env() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let manifest: Manifest = format!(
        "{MANIFEST}\n[env]\nCARGO_TARGET_DIR = \"{{root}}/.zaf/target\"\nGATE_BASE = \"feature/release-{{version}}@origin\"\nHOME = \"/from/manifest\"\n"
    )
    .parse()
    .expect("manifest parses");
    let root = Path::new("/work/bookshelf");
    let board = board();
    let runner = FakeRunner::default();
    let prepared = prepare(&board, &state_dir, &manifest, &|_| true, OWN_PID).expect("prepares");
    let context = LaunchContext {
        env: manifest_env(&manifest, root, "0-3").expect("resolves"),
        ..context(&state_dir)
    };
    launch(&board, &runner, &context, &prepared.plan.launches[0]).expect("records the run");
    let requests = runner.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].env,
        BTreeMap::from([
            (
                "CARGO_TARGET_DIR".to_owned(),
                "/work/bookshelf/.zaf/target".to_owned()
            ),
            (
                "GATE_BASE".to_owned(),
                "feature/release-0-3@origin".to_owned()
            ),
            ("HOME".to_owned(), "/from/manifest".to_owned()),
        ])
    );
}

#[test]
fn a_table_that_needs_a_version_resolves_with_the_one_given() {
    let manifest: Manifest = format!("{MANIFEST}\n[env]\nBASE = \"release-{{version}}\"\n")
        .parse()
        .expect("manifest parses");
    let env = manifest_env(&manifest, Path::new("/p"), "0.2.0").expect("resolves");
    assert_eq!(env["BASE"], "release-0.2.0");
    assert!(manifest_env(&self::manifest(), Path::new("/p"), "0.2.0").is_ok_and(|e| e.is_empty()));
}

#[test]
fn launches_run_in_parallel_threads_and_all_are_joined() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = Arc::new(board());
    let runner = Arc::new(FakeRunner::default());
    for _ in 0..2 {
        runner.record(
            team(),
            RunOutput {
                exit_code: 0,
                transcript: "done\n".to_owned(),
            },
        );
    }
    let prepared = prepare(&*board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    assert_eq!(prepared.plan.launches.len(), 2);
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| {
            let env = BTreeMap::from([("ZAF_INSTANCE".to_owned(), planned.instance.to_string())]);
            (
                planned,
                LaunchContext {
                    env,
                    ..context(&state_dir)
                },
            )
        })
        .collect();
    let ports = || -> Result<LaunchPorts, PortError> {
        Ok((
            Box::new(SharedBoard(Arc::clone(&board))),
            Box::new(SharedRunner(Arc::clone(&runner))),
        ))
    };

    let results = launch_all(&jobs, &ports);

    assert_eq!(results.len(), 2);
    let done: Vec<Launched> = results.into_iter().map(|r| r.expect("launches")).collect();
    assert_eq!(done[0].instance, instance(1, 2));
    assert_eq!(done[1].instance, instance(2, 12));
    assert!(done.iter().all(|d| d.exit == LaunchExit::Code(0)));
    assert!(
        load(&state_dir).expect("loads").instances().is_empty(),
        "every slot is freed when the tick returns, none lost to a racing write"
    );
    let mut seen: Vec<String> = runner
        .requests()
        .into_iter()
        .map(|request| request.env["ZAF_INSTANCE"].clone())
        .collect();
    seen.sort();
    assert_eq!(seen, ["team instance 1 on #2", "team instance 2 on #12"]);
    assert_eq!(board.comments(IssueNumber::from(2)).len(), 2);
    assert_eq!(board.comments(IssueNumber::from(12)).len(), 2);
}

#[test]
fn a_thread_without_ports_frees_its_slot_and_reports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| (planned, context(&state_dir)))
        .collect();
    let ports =
        || -> Result<LaunchPorts, PortError> { Err(PortError::Unavailable("no gh".to_owned())) };
    let results = launch_all(&jobs, &ports);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(SpawnError::Port(_))))
    );
    assert!(load(&state_dir).expect("loads").instances().is_empty());
}

#[test]
fn a_panicking_launch_thread_frees_its_slot() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("prepares");
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| (planned, context(&state_dir)))
        .collect();
    let ports = || -> Result<LaunchPorts, PortError> { panic!("a launch thread panics") };
    let results = launch_all(&jobs, &ports);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(SpawnError::Port(PortError::Backend(_)))))
    );
    assert!(load(&state_dir).expect("loads").instances().is_empty());
}

#[test]
fn a_dry_run_names_each_launch_and_its_command_line_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = plan_only(&board, &state_dir, &manifest(), &|_| true).expect("plans");
    assert_eq!(prepared.plan.launches.len(), 2);
    let profile_dir = state_dir.join("profiles/build");
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| {
            let mut context = context(&state_dir);
            context
                .env
                .insert("ZAF_INSTANCE".to_owned(), planned.instance.to_string());
            (planned, context)
        })
        .collect();
    // Stands in for the composition: the command line a runner would hand to `sh -c`.
    let home = profile_dir.display().to_string();
    let describe = |request: &RunRequest| -> Result<(String, BTreeMap<String, String>), PortError> {
        let mut env = request.env.clone();
        env.insert("CLAUDE_CONFIG_DIR".to_owned(), home.clone());
        Ok((format!("claude -p '{}' --model opus", request.prompt), env))
    };
    let dry = dry_run(&jobs, &profile_dir, &describe).expect("describes");
    let text: String = dry.iter().map(DryLaunch::render_text).collect();
    let brief = state_dir
        .join(RUNS_DIR)
        .join("team-instance-1-on-2")
        .join(BRIEF_FILE);
    let first = format!(
        "would launch team instance 1 on #2\n  epic #2\n  tasks #3 #4\n  profile {home}\n  \
         env CLAUDE_CONFIG_DIR={home}\n  env ZAF_INSTANCE=team instance 1 on #2\n  \
         command claude -p 'You are team instance 1 on #2, the team instance for epic #2 of \
         project bookshelf. Tasks: #3 #4. Read {} first and follow it exactly.' --model opus\n",
        brief.display()
    );
    assert!(text.starts_with(&first), "{text}");
    assert!(text.contains("would launch team instance 2 on #12\n  epic #12\n  tasks #13\n"));
    assert!(
        !state_dir.join(status::SPAWN_STATE_FILE).exists(),
        "no state file"
    );
    assert!(
        !state_dir.exists(),
        "nothing written under the state folder"
    );
    assert!(board.comments(IssueNumber::from(2)).is_empty());
    assert!(board.comments(IssueNumber::from(12)).is_empty());
}

#[test]
fn a_dry_run_reports_a_dead_launch_as_lost_without_recording_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, OWN_PID).expect("reserves");
    let before = std::fs::read(state_dir.join(status::SPAWN_STATE_FILE)).expect("state");
    let dry = plan_only(&board, &state_dir, &manifest(), &|_| false).expect("plans");
    assert_eq!(
        dry.lost,
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    assert_eq!(
        std::fs::read(state_dir.join(status::SPAWN_STATE_FILE)).expect("state"),
        before,
        "the state is untouched"
    );
    assert!(
        board.comments(IssueNumber::from(2)).is_empty(),
        "no lost comment"
    );
}
