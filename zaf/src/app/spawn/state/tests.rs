use super::*;
use crate::domain::{Instance, InstanceNumber, IssueNumber, RoleName};

fn planned(number: u32, epic: u64, tasks: &[u64]) -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: IssueNumber::from(epic),
            number: InstanceNumber::from(number),
        },
        tasks: tasks.iter().copied().map(IssueNumber::from).collect(),
    }
}

#[test]
fn an_absent_file_is_the_empty_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = load(dir.path()).expect("loads");
    assert_eq!(state, SpawnState::default());
    assert_eq!(state.next_instance, 1);
}

#[test]
fn the_state_round_trips_in_the_shape_status_reads() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut state = SpawnState::default();
    state.reserve(&[planned(1, 2, &[3, 4]), planned(2, 12, &[13])], 4242);
    state.start("team instance 1 on #2", "2026-10-28T01:00:00Z");
    save(dir.path(), &state).expect("saves");

    assert_eq!(load(dir.path()).expect("loads"), state);
    let text = std::fs::read_to_string(dir.path().join(SPAWN_STATE_FILE)).expect("reads");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json["next_instance"], 3);
    assert_eq!(json["questions"], serde_json::json!([]));
    assert_eq!(json["launches"][0]["task"], 3);
    assert_eq!(json["launches"][0]["instance"], "team instance 1 on #2");
    assert_eq!(json["launches"][0]["pid"], 4242);
    assert_eq!(json["launches"][0]["started_at"], "2026-10-28T01:00:00Z");
    assert_eq!(json["launches"][2]["task"], 13);
}

#[test]
fn a_garbled_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join(SPAWN_STATE_FILE), "not json").expect("writes");
    let error = load(dir.path()).expect_err("garbled");
    assert!(error.to_string().contains(SPAWN_STATE_FILE), "{error}");
}

#[test]
fn questions_written_by_others_are_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join(SPAWN_STATE_FILE),
        r#"{"launches": [], "questions": [9]}"#,
    )
    .expect("writes");
    let state = load(dir.path()).expect("loads");
    assert_eq!(state.questions, [9]);
    assert_eq!(state.next_instance, 1);
}

#[test]
fn prune_drops_instances_whose_process_is_gone() {
    let mut state = SpawnState::default();
    state.reserve(&[planned(1, 2, &[3, 4])], 100);
    state.reserve(&[planned(2, 12, &[13])], 200);
    state.launches.push(Launch {
        task: 23,
        instance: "team instance 3 on #22".to_owned(),
        pid: None,
        started_at: None,
    });
    let lost = state.prune(&|pid| pid == 200);
    assert_eq!(lost, ["team instance 1 on #2", "team instance 3 on #22"]);
    assert_eq!(state.instances(), ["team instance 2 on #12"]);
}

#[test]
fn an_instance_with_two_tasks_is_one_running_instance() {
    let mut state = SpawnState::default();
    assert_eq!(state.running(), 0);
    state.reserve(&[planned(1, 2, &[3, 4])], 100);
    assert_eq!(state.launches.len(), 2);
    assert_eq!(state.running(), 1);
    state.reserve(&[planned(2, 12, &[13])], 200);
    assert_eq!(state.running(), 2);

    let names: Vec<String> = state
        .running_instances()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["team instance 1 on #2", "team instance 2 on #12"]);
    assert_eq!(state.tasks_of(&state.running_instances()[0]), [3, 4]);
}

#[test]
fn a_pruned_instance_is_gone_from_both() {
    let mut state = SpawnState::default();
    state.reserve(&[planned(1, 2, &[3, 4]), planned(2, 12, &[13])], 100);
    state.prune(&|_| false);
    assert_eq!(state.running(), 0);
    assert!(state.running_instances().is_empty());

    state.reserve(&[planned(3, 2, &[3, 4]), planned(4, 12, &[13])], 100);
    state.prune(&|pid| pid == 100);
    assert_eq!(state.running(), 2);
    state.finish("team instance 3 on #2");
    assert_eq!(state.running(), 1);
    let names: Vec<String> = state
        .running_instances()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["team instance 4 on #12"]);
}

#[test]
fn finish_frees_the_instance_and_keeps_the_counter() {
    let mut state = SpawnState::default();
    state.reserve(&[planned(1, 2, &[3, 4])], 100);
    state.finish("team instance 1 on #2");
    assert!(state.launches.is_empty());
    assert_eq!(state.next_instance, 2);
}
