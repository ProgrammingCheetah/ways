use super::*;
use crate::domain::{InstanceNumber, RoleName, TicketId};

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
fn a_reserved_launch_runs_while_its_holder_lives() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_dir =
        reserve(dir.path(), &planned(16, 71, &[72, 85]), &holder(100), AT).expect("reserves");
    assert_eq!(run_dir, dir.path().join("runs/team-instance-16-on-71"));

    let runs = read_all(dir.path()).expect("reads");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(
        run.record,
        Some(LaunchRecord {
            instance: "team instance 16 on #71".to_owned(),
            tasks: Vec::new(),
            reserved_at: AT.to_owned(),
            started_at: None,
        })
    );
    assert_eq!(run.holder, Some(holder(100)));
    assert_eq!(run.number(), Some(16));
    assert!(run.running(&|_| true));
    assert!(!run.lost(&|_| true));
    assert!(!run.running(&|_| false), "the holder died");
    assert!(
        run.lost(&|_| false),
        "it ended with nobody there to record it"
    );
}

#[test]
fn an_instance_number_is_reserved_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    reserve(dir.path(), &planned(1, 2, &[3]), &holder(100), AT).expect("reserves");
    let error = reserve(dir.path(), &planned(1, 2, &[4]), &holder(100), AT)
        .expect_err("the folder is there");
    assert!(
        error.to_string().contains("team-instance-1-on-2"),
        "{error}"
    );
}

#[test]
fn a_started_launch_records_when() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_dir = reserve(dir.path(), &planned(1, 2, &[3]), &holder(100), AT).expect("reserves");
    mark_started(&run_dir, "2026-11-09T08:00:05Z").expect("marks");
    let runs = read_all(dir.path()).expect("reads");
    let record = runs[0].record.as_ref().expect("a record");
    assert_eq!(record.started_at.as_deref(), Some("2026-11-09T08:00:05Z"));
    assert_eq!(record.reserved_at, AT);
}

#[test]
fn a_finished_launch_neither_runs_nor_is_lost() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_dir = reserve(dir.path(), &planned(1, 2, &[3]), &holder(100), AT).expect("reserves");
    finish(&run_dir, "0").expect("finishes");
    let run = &read_all(dir.path()).expect("reads")[0];
    assert_eq!(run.exit.as_deref(), Some("0"));
    assert_eq!(run.holder, None, "the holder is gone");
    assert!(!run.running(&|_| true));
    assert!(!run.lost(&|_| false));
    finish(&run_dir, "0").expect("finishing again succeeds");
}

#[test]
fn a_folder_from_before_launch_records_is_a_known_instance_that_does_not_run() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("runs/team-instance-17-on-96")).expect("creates");
    std::fs::write(
        dir.path().join("runs/team-instance-17-on-96/report.md"),
        "done\n",
    )
    .expect("writes");
    let run = &read_all(dir.path()).expect("reads")[0];
    assert_eq!(run.slug, "team-instance-17-on-96");
    assert_eq!(run.number(), Some(17));
    assert_eq!(run.record, None);
    assert!(!run.running(&|_| true));
    assert!(
        !run.lost(&|_| false),
        "nothing records it, so nothing marks it lost"
    );
}

#[test]
fn a_garbled_launch_record_is_an_error_naming_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("runs/team-instance-1-on-2")).expect("creates");
    std::fs::write(
        dir.path()
            .join("runs/team-instance-1-on-2")
            .join(LAUNCH_FILE),
        "not json",
    )
    .expect("writes");
    let error = read_all(dir.path()).expect_err("garbled");
    assert!(error.to_string().contains(LAUNCH_FILE), "{error}");
}

#[test]
fn a_garbled_holder_holds_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_dir = reserve(dir.path(), &planned(1, 2, &[3]), &holder(100), AT).expect("reserves");
    std::fs::write(run_dir.join(HOLDER_FILE), "garbage").expect("writes");
    let run = &read_all(dir.path()).expect("reads")[0];
    assert_eq!(run.holder, None);
    assert!(!run.running(&|_| true));
}

#[test]
fn runs_read_in_instance_order_and_an_absent_folder_is_none() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert!(read_all(dir.path()).expect("reads").is_empty());
    for number in [10, 2, 1] {
        reserve(dir.path(), &planned(number, 2, &[3]), &holder(100), AT).expect("reserves");
    }
    std::fs::create_dir_all(dir.path().join("runs/scratch")).expect("creates");
    let slugs: Vec<String> = read_all(dir.path())
        .expect("reads")
        .into_iter()
        .map(|run| run.slug)
        .collect();
    assert_eq!(
        slugs,
        [
            "team-instance-1-on-2",
            "team-instance-2-on-2",
            "team-instance-10-on-2",
            "scratch"
        ]
    );
}
