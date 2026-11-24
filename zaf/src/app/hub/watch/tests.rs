use std::cell::RefCell;
use std::collections::BTreeSet;

use super::*;
use crate::app::hub::{Report, record};
use crate::app::spawn::slug;
use crate::domain::{Instance, TicketId};

/// The processes that have ended.
type Dead = RefCell<BTreeSet<u32>>;

fn instance(number: u32) -> Instance {
    Instance {
        role: "programmer".into(),
        epic: TicketId::from(1_u64),
        number: number.into(),
    }
}

fn run_folder(state_dir: &Path, number: u32, pid: u32) {
    let instance = instance(number);
    let dir = state_dir.join("runs").join(slug(&instance));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let launch = runs::LaunchRecord {
        instance: instance.to_string(),
        tasks: Vec::new(),
        reserved_at: "2026-11-13T10:00:00Z".to_owned(),
        started_at: None,
        graph: None,
    };
    std::fs::write(
        dir.join(runs::LAUNCH_FILE),
        serde_json::to_string(&launch).expect("json"),
    )
    .expect("launch");
    std::fs::write(dir.join(runs::HOLDER_FILE), format!("{pid} 1000\n")).expect("pid");
}

fn report(state_dir: &Path, seconds: u64) {
    record(
        state_dir,
        &Report {
            at: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            task: Some(TicketId::from(10_u64)),
            worker: "programmer".to_owned(),
            what: "testing".to_owned(),
        },
    );
}

fn why(changes: &[Change]) -> Vec<(&Path, Why)> {
    changes
        .iter()
        .map(|change| (change.project.as_path(), change.why))
        .collect()
}

#[test]
fn report_sends_change() {
    let (first, second) = (
        tempfile::tempdir().expect("a"),
        tempfile::tempdir().expect("b"),
    );
    let listed = [Watched::of(first.path()), Watched::of(second.path())];
    let dead = Dead::default();
    let alive = |holder: &Holder| !dead.borrow().contains(&holder.pid);
    let mut changes = Changes::new();
    assert!(
        changes.poll(&listed, &alive).is_empty(),
        "the first look says nothing"
    );
    report(&listed[1].state_dir, 1000);
    let seen = changes.poll(&listed, &alive);
    assert_eq!(why(&seen), vec![(second.path(), Why::Report)], "{seen:?}");
    assert!(
        changes.poll(&listed, &alive).is_empty(),
        "one event for one report"
    );
}

#[test]
fn start_and_end_send_change() {
    let dir = tempfile::tempdir().expect("dir");
    let listed = [Watched::of(dir.path())];
    let dead = Dead::default();
    let alive = |holder: &Holder| !dead.borrow().contains(&holder.pid);
    let mut changes = Changes::new();
    run_folder(&listed[0].state_dir, 1, 41);
    assert!(
        changes.poll(&listed, &alive).is_empty(),
        "a run before the first look is the baseline"
    );

    run_folder(&listed[0].state_dir, 2, 42);
    let seen = changes.poll(&listed, &alive);
    assert_eq!(why(&seen), vec![(dir.path(), Why::Started)], "{seen:?}");

    dead.borrow_mut().insert(42);
    let seen = changes.poll(&listed, &alive);
    assert_eq!(why(&seen), vec![(dir.path(), Why::Ended)], "{seen:?}");
    assert!(
        changes.poll(&listed, &alive).is_empty(),
        "an end is said once"
    );

    dead.borrow_mut().insert(41);
    let seen = changes.poll(&listed, &alive);
    assert_eq!(
        why(&seen),
        vec![(dir.path(), Why::Ended)],
        "the run of the baseline ends too"
    );
}

#[test]
fn quiet_sends_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let listed = [Watched::of(dir.path())];
    let dead = Dead::default();
    let alive = |holder: &Holder| !dead.borrow().contains(&holder.pid);
    report(&listed[0].state_dir, 1000);
    run_folder(&listed[0].state_dir, 1, 41);
    let mut changes = Changes::new();
    changes.poll(&listed, &alive);
    for _ in 0..5 {
        assert!(changes.poll(&listed, &alive).is_empty());
    }
}

#[test]
fn a_project_listed_later_starts_quiet() {
    let (first, second) = (
        tempfile::tempdir().expect("a"),
        tempfile::tempdir().expect("b"),
    );
    let alive = |_: &Holder| true;
    let mut changes = Changes::new();
    changes.poll(&[Watched::of(first.path())], &alive);
    run_folder(&Watched::of(second.path()).state_dir, 1, 41);
    let both = [Watched::of(first.path()), Watched::of(second.path())];
    assert!(
        changes.poll(&both, &alive).is_empty(),
        "its runs are its baseline"
    );
    run_folder(&both[1].state_dir, 2, 42);
    let seen = changes.poll(&both, &alive);
    assert_eq!(why(&seen), vec![(second.path(), Why::Started)]);
}
