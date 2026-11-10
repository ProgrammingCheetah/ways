use std::sync::Arc;

use super::*;
use crate::app::status;
use crate::domain::{InstanceNumber, Ticket, TicketState};
use crate::fakes::{FakeBoard, FakeRunner};
use crate::ports::{ClaimOutcome, ClaimRecord, NewQuestion, RunOutput};

/// Lends one fake board to every launch thread.
struct SharedBoard(Arc<FakeBoard>);

impl Board for SharedBoard {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.0.ready()
    }
    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        self.0.task(number)
    }
    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        self.0.claim(task, instance)
    }
    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.0.heartbeat(task, instance)
    }
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        self.0.claims()
    }
    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.0.release(task, instance)
    }
    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        self.0.set_state(task, label)
    }
    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.0.comment(issue, body)
    }
    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.0.report(task, instance, body)
    }
    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.0.questions()
    }
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        self.0.open_question(question)
    }
    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
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

/// The tick running the test, as the holder of what it reserves.
fn tick() -> Holder {
    holder(OWN_PID)
}

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

fn manifest() -> Manifest {
    MANIFEST.parse().expect("manifest parses")
}

fn ticket(number: u64, epic: Option<u64>, body: &str, label: TicketState) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: epic.map(TicketId::from),
        title: format!("Issue {number}").into(),
        body: body.into(),
        state: Some(label),
        url: "".into(),
        kind: None,
    }
}

/// Epic #2 with ready tasks #3, #4, #5 (#5 blocked by the open #6) and a
/// closed blocker #7 on #4; epic #12 with ready task #13.
fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(ticket(3, Some(2), "Blocked by: —", TicketState::Ready));
    board.add_ticket(ticket(4, Some(2), "Blocked by: #7", TicketState::Ready));
    board.add_ticket(ticket(5, Some(2), "Blocked by: #6", TicketState::Ready));
    board.add_ticket(ticket(6, Some(2), "", TicketState::Blocked));
    board.add_ticket(ticket(7, Some(2), "", TicketState::Done));
    board.add_challenged_epic(TicketId::from(12), "Issue 12");
    board.add_ticket(ticket(13, Some(12), "", TicketState::Ready));
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

/// The instances running now, every recorded process taken as alive.
fn running(state_dir: &Path) -> Vec<String> {
    derive(state_dir, &|_| true)
        .expect("derives")
        .0
        .instances()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn team() -> RoleName {
    RoleName::from(TEAM_ROLE)
}

fn instance(number: u32, epic: u64) -> Instance {
    Instance {
        role: team(),
        epic: TicketId::from(epic),
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

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let first = &prepared.plan.launches[0];
    assert_eq!(first.instance, instance(1, 2));
    assert_eq!(first.tasks, [TicketId::from(3), TicketId::from(4)]);

    // Reserved before running, held by the tick: `zaf status` reads the same run folders.
    let (reserved, _) = derive(&state_dir, &|_| true).expect("derives");
    assert_eq!(reserved.next_instance, 3);
    assert_eq!(
        load(&state_dir).expect("loads").next_instance,
        3,
        "the counter is saved"
    );
    assert_eq!(
        reserved.instances(),
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    let run_dir = state_dir.join(RUNS_DIR).join("team-instance-1-on-2");
    let held = std::fs::read_to_string(run_dir.join(runs::HOLDER_FILE)).expect("holder");
    assert_eq!(Holder::parse(&held), Some(tick()));
    let screen = status::status(&board, &state_dir, 2, &|_| true).expect("status reads the state");
    assert_eq!(screen.running.len(), 3);
    assert_eq!(screen.running[0].task, Some(3));
    assert_eq!(screen.running[0].pid, Some(OWN_PID));

    let done = launch(&board, &runner, &context(&state_dir), first).expect("launches");
    assert_eq!(done.exit, LaunchExit::Code(0));
    assert_eq!(
        board.comments(TicketId::from(2)),
        [
            "<!-- zaf:launch team instance 1 on #2 -->",
            "<!-- zaf:exit team instance 1 on #2 0 -->",
        ]
    );
    assert_eq!(done.run_dir, run_dir);
    assert_eq!(
        std::fs::read_to_string(run_dir.join(runs::EXIT_FILE)).expect("exit"),
        "0\n"
    );
    assert!(
        !run_dir.join(runs::HOLDER_FILE).exists(),
        "nothing holds an ended launch"
    );
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
    assert_eq!(
        request.holder_file,
        Some(run_dir.join(runs::HOLDER_FILE)),
        "the runner records the instance's own process"
    );
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

    let (after, _) = derive(&state_dir, &|_| true).expect("derives");
    assert_eq!(after.instances(), ["team instance 2 on #12"]);
    assert_eq!(after.next_instance, 3);
}

#[test]
fn a_launch_marks_its_start_in_the_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let (state, _) = derive(&state_dir, &|_| true).expect("derives");
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
        board.comments(TicketId::from(12)),
        [
            "<!-- zaf:launch team instance 2 on #12 -->",
            "<!-- zaf:exit team instance 2 on #12 error -->",
        ]
    );
    assert_eq!(running(&state_dir), ["team instance 1 on #2"]);
    let run_dir = state_dir.join(RUNS_DIR).join("team-instance-2-on-12");
    assert_eq!(
        std::fs::read_to_string(run_dir.join(runs::EXIT_FILE)).expect("exit"),
        "error\n"
    );
    let found = runs::read_all(&state_dir).expect("reads");
    let started = found[1]
        .record
        .as_ref()
        .and_then(|record| record.started_at.clone());
    assert!(started.is_some(), "the start is recorded in the run folder");
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
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let first = &prepared.plan.launches[0];
    let task = TicketId::from(3);
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
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let first = &prepared.plan.launches[0];
    let (stuck, waiting, other) = (TicketId::from(3), TicketId::from(4), TicketId::from(13));
    board.claim(stuck, &first.instance).expect("claims #3");
    board.claim(waiting, &first.instance).expect("claims #4");
    board
        .set_state(waiting, TicketState::Question)
        .expect("#4 waits on a question");
    // Held by another instance: stays held.
    board.claim(other, &instance(2, 12)).expect("claims #13");

    let done = launch(&board, &timeout(), &context(&state_dir), first).expect("records the run");

    assert!(done.exit.timed_out());
    assert_eq!(done.exit.status(), "timeout");
    let epic_comments = board.comments(TicketId::from(2));
    assert!(
        epic_comments.contains(&"<!-- zaf:exit team instance 1 on #2 timeout -->".to_owned()),
        "{epic_comments:?}"
    );
    let label = |task| board.task(task).expect("task").state;
    assert_eq!(label(stuck), Some(TicketState::Ready));
    assert_eq!(label(waiting), Some(TicketState::Question));
    assert_eq!(label(other), Some(TicketState::Claimed));
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
        running(&state_dir),
        ["team instance 2 on #12"],
        "the slot is freed"
    );
}

#[test]
fn a_launch_that_did_not_time_out_keeps_its_claims() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let first = &prepared.plan.launches[0];
    let task = TicketId::from(3);
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
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
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
            .comments(TicketId::from(2))
            .contains(&"<!-- zaf:exit team instance 1 on #2 3 -->".to_owned())
    );
}

#[test]
fn prepare_skips_blocked_and_claimed_tasks_read_from_the_board() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let holder = instance(9, 2);
    board.claim(TicketId::from(3), &holder).expect("claims #3");
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let plan = &prepared.plan;
    assert_eq!(plan.launches[0].tasks, [TicketId::from(4)]);
    assert!(
        plan.launches
            .iter()
            .all(|launch| !launch.tasks.contains(&TicketId::from(3)))
    );
    assert_eq!(
        plan.tasks,
        [(
            TicketId::from(5),
            TaskSkip::Blocked(vec![TicketId::from(6)])
        )]
    );
    assert!(plan.render_text().contains("#5 skipped: blocked by #6"));
}

#[test]
fn a_missing_blocker_is_dead() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(ticket(3, Some(2), "Blocked by: #99", TicketState::Ready));

    let prepared = plan_only(&board, &state_dir, &manifest(), &|_| true).expect("plans");
    let text = prepared.plan.render_text();
    assert!(text.contains("dead blocker #99 on #3: not an issue\n"));
    assert!(text.contains("#3 skipped: blocked by #99\n"));

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
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
fn a_blocker_closed_as_not_planned_is_dead() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(ticket(3, Some(2), "Blocked by: #6 #7", TicketState::Ready));
    board.add_ticket(ticket(6, Some(2), "", TicketState::Blocked));
    board.close_not_planned(TicketId::from(6));
    board.add_ticket(ticket(7, Some(2), "", TicketState::Done));
    board.close(TicketId::from(7));

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let text = prepared.plan.render_text();
    assert!(text.contains("dead blocker #6 on #3: closed as not planned\n"));
    assert!(
        !text.contains("dead blocker #7"),
        "a closed issue labelled done is closed for good"
    );
    assert!(text.contains("#3 skipped: blocked by #6\n"));
}

#[test]
fn a_blocker_closed_without_the_done_label_lets_its_dependent_launch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(ticket(3, Some(2), "Blocked by: #6", TicketState::Ready));
    board.add_ticket(ticket(6, Some(2), "", TicketState::Claimed));
    board.close(TicketId::from(6));

    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");

    assert_eq!(
        prepared.plan.launches.len(),
        1,
        "{}",
        prepared.plan.render_text()
    );
    assert_eq!(prepared.plan.launches[0].tasks, [TicketId::from(3)]);
}

#[test]
fn the_snapshot_reads_a_cycle_through_open_tasks_that_are_not_ready() {
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    board.add_ticket(ticket(3, Some(2), "Blocked by: #4", TicketState::Ready));
    board.add_ticket(ticket(4, Some(2), "Blocked by: #5", TicketState::Blocked));
    board.add_ticket(ticket(5, Some(2), "Blocked by: #4", TicketState::Blocked));

    let read = snapshot(&board, &[]).expect("snapshot");
    assert_eq!(
        read.open.keys().copied().collect::<Vec<_>>(),
        [TicketId::from(4), TicketId::from(5)]
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
    prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("first tick");
    let second = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("second tick");
    assert!(second.plan.launches.is_empty());
    assert_eq!(second.plan.slots.free, 0);
}

#[test]
fn a_launch_whose_process_is_gone_is_recorded_as_lost() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    let second = prepare(
        &board,
        &state_dir,
        &manifest(),
        &|held| held.pid != 100,
        &tick(),
    )
    .expect("second tick");
    assert_eq!(
        second.lost,
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    assert!(
        board
            .comments(TicketId::from(2))
            .contains(&"<!-- zaf:exit team instance 1 on #2 lost -->".to_owned())
    );
    assert_eq!(second.plan.launches[0].instance, instance(3, 2));
}

#[test]
fn a_lost_instances_tasks_are_released_and_a_question_is_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    let lost = instance(1, 2);
    let (waiting, stuck, other) = (TicketId::from(4), TicketId::from(3), TicketId::from(13));
    board.claim(stuck, &lost).expect("claims #3");
    board.claim(waiting, &lost).expect("claims #4");
    board
        .set_state(waiting, TicketState::Question)
        .expect("#4 waits on a question");
    // Held by an instance that is not lost: stays held.
    board.claim(other, &instance(9, 12)).expect("claims #13");

    let second = prepare(
        &board,
        &state_dir,
        &manifest(),
        &|held| held.pid != 100,
        &tick(),
    )
    .expect("second tick");

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
    let label = |task| board.task(task).expect("task").state;
    assert_eq!(label(stuck), Some(TicketState::Ready));
    assert_eq!(label(waiting), Some(TicketState::Question));
    assert_eq!(label(other), Some(TicketState::Claimed));
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
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    board
        .claim(TicketId::from(3), &instance(1, 2))
        .expect("claims #3");
    let second = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("second tick");
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
    let prepared = prepare(&board, state_dir, &manifest(), &|_| true, &tick()).expect("tick");
    assert!(prepared.plan.launches.is_empty());
    std::fs::write(dir.path().join(GUARD_FILE), "{").expect("writes");
    assert!(read_guard(dir.path()).is_err());
}

#[test]
fn a_signed_epic_with_no_task_launches_planning_and_only_release_is_unbuilt() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    let prepared = prepare(&board, dir.path(), &manifest(), &|_| true, &tick()).expect("prepares");
    let launched: Vec<String> = prepared
        .plan
        .launches
        .iter()
        .map(|launch| launch.instance.to_string())
        .collect();
    assert_eq!(launched, ["planning instance 1 on #2"]);
    assert!(prepared.plan.launches[0].tasks.is_empty());
    let text = prepared.plan.render_text();
    assert!(text.contains("release: skipped: not built"), "{text}");
    assert!(!text.contains("planning: skipped"), "{text}");
    let json = prepared.plan.to_json(&[], &[]);
    assert_eq!(json["skipped"]["branches"][0]["branch"], "release");
    assert_eq!(
        json["skipped"]["branches"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn json_names_every_launch_and_every_skipped_epic() {
    let dir = tempfile::tempdir().expect("temp dir");
    let board = board();
    let manifest: Manifest = MANIFEST
        .replace("slots = 2", "slots = 1")
        .parse()
        .expect("manifest parses");
    let prepared = prepare(&board, dir.path(), &manifest, &|_| true, &tick()).expect("tick");
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
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
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
    let prepared = prepare(&board, &state_dir, &manifest, &|_| true, &tick()).expect("prepares");
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
    let prepared = prepare(&*board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
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
    let ports = |_: &RoleName| -> Result<LaunchPorts, PortError> {
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
        running(&state_dir).is_empty(),
        "every slot is freed when the tick returns, none lost to a racing write"
    );
    let mut seen: Vec<String> = runner
        .requests()
        .into_iter()
        .map(|request| request.env["ZAF_INSTANCE"].clone())
        .collect();
    seen.sort();
    assert_eq!(seen, ["team instance 1 on #2", "team instance 2 on #12"]);
    assert_eq!(board.comments(TicketId::from(2)).len(), 2);
    assert_eq!(board.comments(TicketId::from(12)).len(), 2);
}

#[test]
fn a_thread_without_ports_frees_its_slot_and_reports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| (planned, context(&state_dir)))
        .collect();
    let ports = |_: &RoleName| -> Result<LaunchPorts, PortError> {
        Err(PortError::Unavailable("no gh".to_owned()))
    };
    let results = launch_all(&jobs, &ports);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(SpawnError::Port(_))))
    );
    assert!(running(&state_dir).is_empty());
}

#[test]
fn a_panicking_launch_thread_frees_its_slot() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let jobs: Vec<_> = prepared
        .plan
        .launches
        .iter()
        .map(|planned| (planned, context(&state_dir)))
        .collect();
    let ports =
        |_: &RoleName| -> Result<LaunchPorts, PortError> { panic!("a launch thread panics") };
    let results = launch_all(&jobs, &ports);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(SpawnError::Port(PortError::Backend(_)))))
    );
    assert!(running(&state_dir).is_empty());
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
    let dir_of = |_| profile_dir.clone();
    let dry = dry_run(&jobs, &dir_of, &describe).expect("describes");
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
    assert!(board.comments(TicketId::from(2)).is_empty());
    assert!(board.comments(TicketId::from(12)).is_empty());
}

#[test]
fn a_dry_run_reports_a_dead_launch_as_lost_without_recording_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("reserves");
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
        board.comments(TicketId::from(2)).is_empty(),
        "no lost comment"
    );
}

/// The instance's own process takes over its launch from the tick, as the runner does.
fn instance_holds(state_dir: &Path, slug: &str, pid: u32) {
    std::fs::write(
        state_dir.join(RUNS_DIR).join(slug).join(runs::HOLDER_FILE),
        format!("{}\n", holder(pid)),
    )
    .expect("the instance records itself");
}

#[test]
fn a_killed_tick_leaves_its_running_instance_its_slot_and_its_claims() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    instance_holds(&state_dir, "team-instance-1-on-2", 500);
    board
        .claim(TicketId::from(3), &instance(1, 2))
        .expect("the instance claims #3");

    // The first tick (100) is gone; instance 1 (500) still runs; instance 2 never started.
    let table = |held: &Holder| held.pid == 500;
    let second = prepare(&board, &state_dir, &manifest(), &table, &tick()).expect("second tick");

    assert_eq!(second.lost, ["team instance 2 on #12"]);
    assert!(second.released.is_empty(), "{:?}", second.released);
    assert_eq!(
        board.claims().expect("claims").len(),
        1,
        "the claim is kept"
    );
    assert_eq!(second.plan.slots.running, 1, "instance 1 keeps its slot");
    assert_eq!(second.plan.launches[0].instance, instance(3, 12));
}

#[test]
fn a_tick_that_outlives_its_instance_holds_no_slot_and_the_next_tick_takes_its_task() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    instance_holds(&state_dir, "team-instance-1-on-2", 500);
    instance_holds(&state_dir, "team-instance-2-on-12", 600);
    let stuck = TicketId::from(3);
    board
        .claim(stuck, &instance(1, 2))
        .expect("the instance claims #3");

    // The first tick (100) still runs; both instances have died without an exit.
    let table = |held: &Holder| held.pid == 100;
    let screen = status::status(&board, &state_dir, 2, &table).expect("status");
    assert_eq!(screen.slots.free, 2, "free at once, before any tick");
    assert_eq!(screen.orphaned.len(), 1);

    let second = prepare(&board, &state_dir, &manifest(), &table, &tick()).expect("second tick");
    assert_eq!(
        second.lost,
        ["team instance 1 on #2", "team instance 2 on #12"]
    );
    assert_eq!(
        second.released,
        [Released {
            task: stuck,
            instance: instance(1, 2),
        }]
    );
    assert_eq!(second.plan.slots.running, 0);
    assert_eq!(
        second.plan.launches[0].tasks,
        [TicketId::from(3), TicketId::from(4)],
        "the freed task launches again"
    );
}

#[test]
fn a_lost_launch_is_recorded_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = board();
    prepare(&board, &state_dir, &manifest(), &|_| true, &holder(100)).expect("first tick");
    let table = |held: &Holder| held.pid != 100;
    let second = prepare(&board, &state_dir, &manifest(), &table, &tick()).expect("second tick");
    assert_eq!(second.lost.len(), 2);
    let third = prepare(&board, &state_dir, &manifest(), &table, &tick()).expect("third tick");
    assert!(third.lost.is_empty(), "{:?}", third.lost);
    let lost_comments = board
        .comments(TicketId::from(2))
        .into_iter()
        .filter(|comment| comment.ends_with(" lost -->"))
        .count();
    assert_eq!(lost_comments, 1);
}

#[test]
fn a_planning_launch_runs_its_role_with_the_planning_brief_and_no_task() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_dir = dir.path().join(".zaf");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(2), "Issue 2");
    let planning = RoleName::from(PLANNING_ROLE);
    let runner = FakeRunner::default();
    runner.record(
        planning.clone(),
        RunOutput {
            exit_code: 0,
            transcript: "filed #3\n".to_owned(),
        },
    );
    let prepared = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    let planned = &prepared.plan.launches[0];
    assert_eq!(planned.instance.role, planning);
    let (reserved, _) = derive(&state_dir, &|_| true).expect("derives");
    assert_eq!(
        reserved.running(),
        1,
        "a launch with no task holds its slot"
    );

    let done = launch(&board, &runner, &context(&state_dir), planned).expect("launches");
    assert_eq!(done.exit, LaunchExit::Code(0));
    let requests = runner.requests();
    let request = &requests[0];
    assert_eq!(request.role, planning);
    let run_dir = state_dir.join(RUNS_DIR).join("planning-instance-1-on-2");
    let brief = run_dir.join(BRIEF_FILE);
    assert_eq!(
        request.prompt,
        format!(
            "You are planning instance 1 on #2, the planning instance for epic #2 of project \
             bookshelf. Read {} first and follow it exactly.",
            brief.display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(&brief).expect("brief"),
        PLANNING_BRIEF
    );

    // The run ended after the signature, so the next tick does not plan the epic again.
    let again = prepare(&board, &state_dir, &manifest(), &|_| true, &tick()).expect("prepares");
    assert!(again.plan.launches.is_empty());
    assert_eq!(
        again.plan.epics,
        [(TicketId::from(2), EpicSkip::PlannedNothing)]
    );
}

#[test]
fn each_role_has_its_pack_brief() {
    assert_eq!(pack_brief(&RoleName::from(PLANNING_ROLE)), PLANNING_BRIEF);
    assert_eq!(
        pack_brief(&RoleName::from(CHALLENGER_ROLE)),
        CHALLENGER_BRIEF
    );
    assert_eq!(pack_brief(&team()), TEAM_BRIEF);
    for brief in [PLANNING_BRIEF, CHALLENGER_BRIEF] {
        assert!(brief.contains("zaf epic show <epic>"), "{brief}");
        assert!(brief.contains("zaf board report <epic>"), "{brief}");
    }
    assert!(PLANNING_BRIEF.contains("zaf board file <epic>"));
}
