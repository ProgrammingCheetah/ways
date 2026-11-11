use std::cell::Cell;

use super::*;

#[test]
fn the_exit_code_is_recorded() {
    let dir = tempfile::tempdir().expect("temp dir");

    record_exit(dir.path(), 3).expect("record");

    // The decimal code and a newline, and no staging file left behind.
    assert_eq!(
        std::fs::read_to_string(dir.path().join(EXIT_FILE)).expect("exit"),
        "3\n"
    );
    assert!(!dir.path().join("exit.new").exists());
    assert_eq!(exit_code(Some(3), None), 3);
    assert_eq!(exit_code(Some(0), None), 0);
}

#[test]
fn a_killed_phase_records_its_signal() {
    // SIGTERM is 15, so the shell's convention is 143; SIGKILL is 137.
    assert_eq!(exit_code(None, Some(15)), 143);
    assert_eq!(exit_code(None, Some(9)), 137);
    let dir = tempfile::tempdir().expect("temp dir");

    record_exit(dir.path(), exit_code(None, Some(15))).expect("record");

    assert_eq!(
        std::fs::read_to_string(dir.path().join(EXIT_FILE)).expect("exit"),
        "143\n"
    );
}

#[test]
fn the_exit_is_written_before_post_phase() {
    let dir = tempfile::tempdir().expect("temp dir");
    let seen = Cell::new(None);

    // What follows the exit sees the file already there, whole.
    end(dir.path(), 7, &|| {
        seen.set(std::fs::read_to_string(dir.path().join(EXIT_FILE)).ok());
    })
    .expect("end");

    assert_eq!(seen.take().as_deref(), Some("7\n"));
}

#[test]
fn what_follows_still_runs_when_the_exit_cannot_be_written() {
    let dir = tempfile::tempdir().expect("temp dir");
    let ran = Cell::new(false);

    // A run folder that is gone cannot take the file; the error comes back, the rest still runs.
    let result = end(&dir.path().join("gone"), 1, &|| ran.set(true));

    assert!(result.is_err());
    assert!(ran.get());
}
