use super::*;

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

/// A process table where only these pids run.
fn running(pids: &'static [u32]) -> impl Fn(&Holder) -> bool {
    move |holder| pids.contains(&holder.pid)
}

#[test]
fn second_refused_with_url() {
    let home = tempfile::tempdir().unwrap();
    let alive = running(&[10, 20]);
    let first = HubLock::take(home.path(), &holder(10), 8080, &alive).unwrap();
    assert_eq!((first.pid, first.port), (10, 8080));

    let refused = HubLock::take(home.path(), &holder(20), 9090, &alive).unwrap_err();
    let LockError::Held(held) = refused else {
        panic!("expected Held, got {refused:?}");
    };
    // The first hub's port is what the refusal carries; the second's start left nothing behind.
    assert_eq!((held.pid, held.port), (10, 8080));
    assert_eq!(HubLock::held(home.path(), &alive), Some(first));
}

#[test]
fn stale_lock_taken() {
    let home = tempfile::tempdir().unwrap();
    HubLock::take(home.path(), &holder(10), 8080, &running(&[10])).unwrap();

    // Process 10 has died; 20 takes over.
    let alive = running(&[20]);
    assert_eq!(HubLock::held(home.path(), &alive), None);
    let second = HubLock::take(home.path(), &holder(20), 9090, &alive).unwrap();
    assert_eq!((second.pid, second.port), (20, 9090));
    assert_eq!(HubLock::held(home.path(), &alive), Some(second));
}

#[test]
fn a_pid_with_another_start_is_stale() {
    let home = tempfile::tempdir().unwrap();
    HubLock::take(home.path(), &holder(10), 8080, &|_| true).unwrap();
    // The OS handed pid 10 to a process that started later.
    let reused = |holder: &Holder| holder.pid == 10 && holder.start == "2000";
    assert_eq!(HubLock::held(home.path(), &reused), None);
    assert!(HubLock::take(home.path(), &holder(20), 9090, &reused).is_ok());
}

#[test]
fn a_file_that_is_not_a_lock_is_taken_over() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(LOCK_FILE), "garbage").unwrap();
    assert_eq!(HubLock::held(home.path(), &|_| true), None);
    assert!(HubLock::take(home.path(), &holder(20), 9090, &|_| true).is_ok());
}

#[test]
fn terminal_frame_unaffected() {
    let home = tempfile::tempdir().unwrap();
    let alive = running(&[10, 20]);
    HubLock::take(home.path(), &holder(10), 8080, &alive).unwrap();

    // No port: no lock is read or taken, so the live hub's lock stays as it was.
    assert!(
        claim(home.path(), None, &holder(20), &alive)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        HubLock::held(home.path(), &alive).map(|lock| lock.pid),
        Some(10)
    );
    // With a port the same start is refused.
    assert!(matches!(
        claim(home.path(), Some(9090), &holder(20), &alive),
        Err(LockError::Held(_))
    ));
}

#[test]
fn release_gives_up_only_its_own() {
    let home = tempfile::tempdir().unwrap();
    let alive = running(&[10, 20]);
    let first = HubLock::take(home.path(), &holder(10), 8080, &alive).unwrap();
    first.release(home.path());
    assert_eq!(HubLock::held(home.path(), &alive), None);

    let second = HubLock::take(home.path(), &holder(20), 9090, &alive).unwrap();
    // A stale release by the first does not drop the second's lock.
    first.release(home.path());
    assert_eq!(HubLock::held(home.path(), &alive), Some(second));
}

#[test]
fn the_home_is_made_when_missing() {
    let home = tempfile::tempdir().unwrap();
    let nested = home.path().join("zaf");
    assert!(HubLock::take(&nested, &holder(10), 8080, &|_| true).is_ok());
}

#[test]
fn starts_at_once_leave_one_hub() {
    let home = tempfile::tempdir().unwrap();
    // A dead hub's lock is there for every start to find stale.
    HubLock::take(home.path(), &holder(1), 8080, &|_| true).unwrap();
    let taken: Vec<bool> = std::thread::scope(|scope| {
        let starts: Vec<_> = (10..26_u32)
            .map(|pid| {
                let home = home.path();
                scope.spawn(move || {
                    HubLock::take(home, &holder(pid), 9000, &|holder| holder.pid != 1).is_ok()
                })
            })
            .collect();
        starts
            .into_iter()
            .map(|start| start.join().unwrap())
            .collect()
    });
    assert_eq!(taken.iter().filter(|took| **took).count(), 1, "{taken:?}");
}

#[test]
fn a_start_leaves_no_temporary_file() {
    let home = tempfile::tempdir().unwrap();
    HubLock::take(home.path(), &holder(10), 8080, &|_| true).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(home.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [LOCK_FILE, GUARD_FILE]);
}
