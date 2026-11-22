use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{Bounded, bounded, entries, fix_command, last_line, tabs};
use crate::app::chair::{Fix, FixVerb};

#[test]
fn the_project_here_is_a_tab_even_when_the_machine_does_not_list_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let project = dir.path().join("ways");

    let (found, active) = entries(Some(&project), Some(&home));

    assert_eq!(found.len(), 1);
    assert_eq!(active, 0);
    assert_eq!(found[0].folder, project);
    assert_eq!(found[0].name, "ways");
    assert_eq!(tabs(&found)[0].name.as_str(), "ways");
    assert!(tabs(&found)[0].state.is_none(), "not read yet");
}

#[test]
fn outside_a_project_the_machine_lists_the_tabs() {
    let (found, active) = entries(None, None);

    assert!(found.is_empty());
    assert_eq!(active, 0);
    let (found, _) = entries(None, Some(Path::new("/nowhere/at/all")));
    assert!(found.is_empty());
}

#[test]
fn the_last_line_with_words_is_what_a_fix_said() {
    assert_eq!(last_line(b"one\ntwo\n\n  \n"), "two");
    assert_eq!(last_line(b""), "");
}

#[test]
fn a_fix_runs_its_verb_in_the_project_with_no_phase_variable() {
    let fix = Fix::new(
        PathBuf::from("/srv/ways"),
        FixVerb::ProfileInit,
        "cd /etc && zaf do ship",
    );
    let names: Vec<OsString> = ["PATH", "ZAF_PHASE", "ZAF_RUN", "ZAF_HOME", "ZAF_READY"]
        .into_iter()
        .map(OsString::from)
        .collect();

    let command = fix_command(
        Path::new("/usr/bin/zaf"),
        &fix,
        Some(Path::new("/home/z/.zaf")),
        &names,
    );

    assert_eq!(command.get_program(), "/usr/bin/zaf");
    let args: Vec<&OsStr> = command.get_args().collect();
    assert_eq!(args, ["profile", "init"]);
    assert_eq!(command.get_current_dir(), Some(Path::new("/srv/ways")));
    let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
    for gone in ["ZAF_PHASE", "ZAF_READY", "ZAF_RUN"] {
        assert!(envs.contains(&(OsStr::new(gone), None)), "{gone}: {envs:?}");
    }
    assert!(
        envs.contains(&(OsStr::new("ZAF_HOME"), Some(OsStr::new("/home/z/.zaf")))),
        "{envs:?}"
    );
    assert!(!envs.iter().any(|(name, _)| *name == "PATH"), "{envs:?}");
}

#[test]
fn a_fix_past_its_limit_is_killed_with_what_it_started() {
    let mut command = std::process::Command::new("/bin/sh");
    // The background sleep keeps the pipes open after its parent: the group is killed with it.
    command.args(["-c", "sleep 30 & sleep 30"]);
    let started = std::time::Instant::now();

    let ran = bounded(command, std::time::Duration::from_millis(300)).expect("it starts");

    assert!(matches!(ran, Bounded::Killed), "{ran:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_fix_within_its_limit_says_what_it_printed() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "echo out; echo err >&2; exit 3"]);

    let ran = bounded(command, std::time::Duration::from_secs(10)).expect("it starts");

    let Bounded::Ended {
        status,
        stdout,
        stderr,
    } = ran
    else {
        panic!("{ran:?}");
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(stdout, b"out\n");
    assert_eq!(stderr, b"err\n");
}
