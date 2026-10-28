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
    fn release(&self, task: IssueNumber) -> Result<(), PortError> {
        self.0.release(task)
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
    fn record_ruling(&self, question: IssueNumber, ruling: &str) -> Result<(), PortError> {
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
