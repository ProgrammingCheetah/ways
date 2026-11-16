use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::*;

/// A stand-in for `zaf` that runs `body` as a shell script, in a folder of its own.
fn fake_zaf(dir: &Path, body: &str) -> PathBuf {
    let file = dir.join("zaf");
    std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("mode");
    file
}

/// [`super::run`], tried again while the system says the script is still open for writing in
/// another test's fork (`ETXTBSY`), which a script written a moment ago can meet.
fn run(program: &Path, root: &Path, version: &str, wait: Duration) -> Ticked {
    let mut ticked = super::run(program, root, version, wait);
    for _ in 0..20 {
        match &ticked {
            Ticked::Failed(why) if why.contains("Text file busy") => {
                thread::sleep(Duration::from_millis(50));
                ticked = super::run(program, root, version, wait);
            }
            _ => break,
        }
    }
    ticked
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("project"),
    )
}

#[test]
fn the_tick_is_the_verb_run_in_the_projects_root() {
    let (bin, project) = fixture();
    let zaf = fake_zaf(bin.path(), "echo \"$@\"; pwd");

    let ticked = run(&zaf, project.path(), "0.4.0", Duration::from_secs(10));

    let root = std::fs::canonicalize(project.path()).expect("root");
    assert_eq!(
        ticked,
        Ticked::Ran(vec![
            "spawn --once --no-wait --version 0.4.0".to_owned(),
            root.display().to_string(),
        ])
    );
}

#[test]
fn the_hub_is_no_instance_and_no_phase_to_the_tick() {
    let (bin, project) = fixture();
    let zaf = fake_zaf(bin.path(), "true");

    let command = command(&zaf, project.path(), "0.4.0");

    // Both are taken out of what the hub passes on, whatever the hub itself was started in.
    let removed: Vec<_> = command
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(removed, [INSTANCE_VAR, RUN_VAR]);
}

#[test]
fn a_refused_tick_says_why() {
    let (bin, project) = fixture();
    let zaf = fake_zaf(bin.path(), "echo 'zaf spawn: stale profile' >&2; exit 1");

    let ticked = run(&zaf, project.path(), "0.4.0", Duration::from_secs(10));

    assert_eq!(
        ticked,
        Ticked::Refused("zaf spawn: stale profile".to_owned())
    );
}

#[test]
fn a_failed_tick_names_its_exit() {
    let (bin, project) = fixture();
    let zaf = fake_zaf(bin.path(), "exit 2");

    let ticked = run(&zaf, project.path(), "0.4.0", Duration::from_secs(10));

    assert_eq!(
        ticked,
        Ticked::Failed("the tick failed with exit 2".to_owned())
    );
}

#[test]
fn a_tick_that_will_not_start_is_a_failure() {
    let (_, project) = fixture();

    let ticked = run(
        Path::new("/nonexistent/zaf"),
        project.path(),
        "0.4.0",
        Duration::from_secs(10),
    );

    assert!(matches!(ticked, Ticked::Failed(_)), "{ticked:?}");
}

#[test]
fn a_tick_still_running_is_not_waited_for() {
    let (bin, project) = fixture();
    let zaf = fake_zaf(bin.path(), "echo started implement '#7'; sleep 30");
    let began = Instant::now();

    let ticked = run(&zaf, project.path(), "0.4.0", Duration::from_millis(500));

    assert!(began.elapsed() < Duration::from_secs(10), "it waited");
    assert_eq!(
        ticked,
        Ticked::Running(vec!["started implement #7".to_owned()])
    );
}

#[test]
fn a_model_holding_the_pipes_open_does_not_hold_the_answer() {
    let (bin, project) = fixture();
    // The tick ends at once; a process it started keeps its output open for a long time.
    let zaf = fake_zaf(
        bin.path(),
        "echo started implement '#7'; (sleep 30 &) ; exit 0",
    );
    let began = Instant::now();

    let ticked = run(&zaf, project.path(), "0.4.0", Duration::from_secs(20));

    assert!(began.elapsed() < Duration::from_secs(10), "it waited");
    assert_eq!(ticked, Ticked::Ran(vec!["started implement #7".to_owned()]));
}
