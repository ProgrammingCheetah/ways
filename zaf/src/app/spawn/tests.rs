use super::*;
use crate::app::status;
use crate::domain::{InstanceNumber, TaskLabel};
use crate::fakes::{FakeBoard, FakeRunner};
use crate::ports::{RunOutput, Ticket};

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
