use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use super::*;
use crate::app::spawn::record::watch_running;
use crate::app::spawn::watch::Watches;

/// A stand-in for `zaf` that runs `body` as a shell script, in a folder of its own.
fn fake_zaf(dir: &Path, body: &str) -> PathBuf {
    let file = dir.join("zaf");
    std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("mode");
    file
}

fn process(program: PathBuf, root: &Path) -> Process {
    Process {
        program,
        root: root.to_owned(),
        version: "0.4.0".to_owned(),
    }
}

#[test]
fn a_tick_is_the_verb_run_in_the_projects_root_for_the_hubs_scheduler() {
    let (bin, project) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("project"),
    );
    let tick = process(fake_zaf(bin.path(), "true"), project.path());

    let command = tick.command();

    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["spawn", "--once", "--version", "0.4.0"]);
    assert_eq!(command.get_current_dir(), Some(project.path()));
    let envs: Vec<_> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(envs.contains(&(INSTANCE_VAR.to_owned(), None)), "{envs:?}");
    assert!(envs.contains(&(RUN_VAR.to_owned(), None)), "{envs:?}");
    assert!(
        envs.contains(&(
            SCHEDULED_BY_VAR.to_owned(),
            Some(std::process::id().to_string())
        )),
        "the tick is told whose it is: {envs:?}"
    );
}

#[test]
fn a_tick_gives_the_exit_of_the_process_and_two_for_one_that_cannot_start() {
    let (bin, project) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("project"),
    );
    let asked = Asked::new(["hub".to_owned()]);
    let fails = process(fake_zaf(bin.path(), "exit 7"), project.path());
    let missing = process(bin.path().join("nothing-here"), project.path());

    let code = (0..20).find_map(|_| {
        // A script written a moment ago can meet `ETXTBSY` in another test's fork.
        let code = fails.tick(&asked);
        (code == 7).then_some(code).or_else(|| {
            std::thread::sleep(Duration::from_millis(50));
            None
        })
    });

    assert_eq!(code, Some(7));
    assert_eq!(missing.tick(&asked), 2);
}

#[test]
fn the_hubs_watch_ticks_the_projects_root_at_once_and_ends_with_the_hub() {
    let (bin, home, project) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("project"),
    );
    std::fs::write(
        project.path().join(".zaf.toml"),
        "[project]\nname = \"example\"\n\n\
         [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
         [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n",
    )
    .expect("manifest");
    let root = std::fs::canonicalize(project.path()).expect("root");
    let seen = bin.path().join("seen");
    // Each tick leaves where it ran, with what, and whose it was told it is.
    let zaf = fake_zaf(
        bin.path(),
        &format!(
            "echo \"$(pwd) $@ by=$ZAF_SCHEDULED_BY\" >> '{}'",
            seen.display()
        ),
    );
    let watches = Watches::new();
    let hub = Hub::of(watches.handle(), Some(home.path().to_owned()), zaf);

    let started = hub.start(&root, "0.4.0").expect("started");

    assert_eq!(started.version, "0.4.0");
    assert!(watch_running(&root.join(STATE_DIR)));
    let limit = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(&seen)
        .unwrap_or_default()
        .is_empty()
    {
        assert!(Instant::now() < limit, "no tick ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let ticked = std::fs::read_to_string(&seen).expect("seen");
    assert_eq!(
        ticked.lines().next(),
        Some(
            format!(
                "{} spawn --once --version 0.4.0 by={}",
                root.display(),
                std::process::id()
            )
            .as_str()
        )
    );

    drop(watches);
    assert!(
        !watch_running(&root.join(STATE_DIR)),
        "it ended with the hub"
    );
}
