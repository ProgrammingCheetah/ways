use super::*;
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName, TicketId};

fn planned(number: u32, epic: u64, _tasks: &[u64]) -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(epic),
            number: InstanceNumber::from(number),
        },
    }
}

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

const AT: &str = "2026-11-09T08:00:00Z";

#[test]
fn an_absent_file_is_the_empty_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = load(dir.path()).expect("loads");
    assert_eq!(state, SpawnState::default());
    assert_eq!(state.next_instance, 1);
}

#[test]
fn the_file_holds_the_counter_and_nothing_a_live_source_answers() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = SpawnState {
        next_instance: 3,
        launches: vec![Launch {
            task: Some(3),
            instance: "team instance 1 on #2".to_owned(),
            pid: Some(100),
            started_at: None,
        }],
    };
    save(dir.path(), &state).expect("saves");
    let text = std::fs::read_to_string(dir.path().join(SPAWN_STATE_FILE)).expect("reads");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json, serde_json::json!({ "next_instance": 3 }));
    assert_eq!(load(dir.path()).expect("loads").next_instance, 3);
}

#[test]
fn a_garbled_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), "not json").expect("writes");
    let error = load(dir.path()).expect_err("garbled");
    assert!(error.to_string().contains(SPAWN_STATE_FILE), "{error}");
}

#[test]
fn an_older_files_launches_and_questions_are_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join(SPAWN_STATE_FILE),
        r#"{"next_instance": 18, "launches": [{"task": 72, "instance": "team instance 16 on #71", "pid": 368333}], "questions": [9]}"#,
    )
    .expect("writes");
    let (state, _) = derive(dir.path(), &|_| true).expect("derives");
    assert_eq!(state.next_instance, 18);
    assert_eq!(state.running(), 0, "a dead record holds no slot");
}

#[test]
fn only_launches_whose_holder_lives_are_running() {
    let dir = tempfile::tempdir().expect("temp dir");
    runs::reserve(dir.path(), &planned(1, 2, &[3, 4]), &holder(100), AT).expect("reserves");
    runs::reserve(dir.path(), &planned(2, 12, &[13]), &holder(200), AT).expect("reserves");
    let (state, found) = derive(dir.path(), &|holder| holder.pid == 200).expect("derives");
    assert_eq!(found.len(), 2);
    assert_eq!(state.instances(), ["team instance 2 on #12"]);
    assert_eq!(state.launches[0].pid, Some(200));
}

#[test]
fn an_instance_with_two_tasks_is_one_running_instance() {
    let dir = tempfile::tempdir().expect("temp dir");
    runs::reserve(dir.path(), &planned(1, 2, &[3, 4]), &holder(100), AT).expect("reserves");
    runs::reserve(dir.path(), &planned(2, 12, &[13]), &holder(100), AT).expect("reserves");
    let (state, _) = derive(dir.path(), &|_| true).expect("derives");
    assert_eq!(state.launches.len(), 2, "a launch carries no task");
    assert_eq!(state.running(), 2);
    let names: Vec<String> = state
        .running_instances()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["team instance 1 on #2", "team instance 2 on #12"]);
    assert!(state.tasks_of(&state.running_instances()[0]).is_empty());
}

#[test]
fn the_counter_never_falls_behind_a_run_folder() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("runs/team-instance-17-on-96")).expect("creates");
    let (state, _) = derive(dir.path(), &|_| true).expect("derives");
    assert_eq!(
        state.next_instance, 18,
        "no saved counter, one old run folder"
    );

    save(
        dir.path(),
        &SpawnState {
            next_instance: 30,
            launches: Vec::new(),
        },
    )
    .expect("saves");
    let (state, _) = derive(dir.path(), &|_| true).expect("derives");
    assert_eq!(state.next_instance, 30, "a counter past every folder stays");
}

#[test]
fn a_launch_with_no_task_holds_a_slot_and_carries_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut planning = planned(1, 2, &[]);
    planning.instance.role = RoleName::from("planning");
    runs::reserve(dir.path(), &planning, &holder(100), AT).expect("reserves");
    let (state, _) = derive(dir.path(), &|_| true).expect("derives");
    assert_eq!(state.running(), 1);
    assert_eq!(state.instances(), ["planning instance 1 on #2"]);
    assert_eq!(state.launches[0].task, None);
    assert!(state.tasks_of(&planning.instance).is_empty());
}
