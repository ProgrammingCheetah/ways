//! The `pre-tick` hook, through the binary: `zaf spawn --once` runs `.zaf/actions/hooks/pre-tick`
//! first, from the project root and before any forge read; one that exits non-zero ends the tick
//! with exit 2 having started nothing, and without a hook the tick goes on.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{Project, outcome};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n";

const SPAWN: [&str; 4] = ["spawn", "--once", "--version", "0.3.0"];

/// A project with current profiles and an empty board.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// Writes `.zaf/actions/hooks/pre-tick` with `body` under its metadata.
fn pre_tick(project: &Project, body: &str) {
    let hooks = project.root.join(".zaf/actions/hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let file = hooks.join("pre-tick");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: the tick must not run\n{body}\n"
        ),
    )
    .expect("hook");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

fn logs(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join(".zaf/logs")) else {
        return Vec::new();
    };
    entries
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn a_failing_pre_tick_fails_the_tick() {
    let project = project();
    pre_tick(&project, "echo refused by the hook\nexit 1");
    let before = project.gh_calls().len();

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stderr.contains("FAIL hooks/pre-tick: exit 1"), "{stderr}");
    assert!(stderr.contains("refused by the hook"), "{stderr}");
    // Nothing was read from the forge and nothing was reserved or launched.
    assert_eq!(project.gh_calls().len(), before, "no forge read");
    assert!(!project.root.join(".zaf/spawn.state").exists());
    assert_eq!(stdout, "", "no plan was printed");
    // The output and the exit code are in the hook's log.
    let log = logs(&project.root)
        .into_iter()
        .find(|name| name.ends_with("_hooks_pre-tick.log"))
        .expect("the hook's log");
    let text = fs::read_to_string(project.root.join(".zaf/logs").join(log)).expect("log");
    assert!(text.contains("refused by the hook"), "{text}");
    assert!(text.contains("hooks/pre-tick exit 1"), "{text}");
}

#[test]
fn a_passing_pre_tick_lets_it_run() {
    let project = project();
    // The hook runs from the project root.
    pre_tick(&project, "pwd > ran-here");

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("slots 1 of 1"),
        "the tick ran and printed its plan: {stdout}"
    );
    assert!(!project.gh_calls().is_empty(), "the tick read the forge");
    let here = fs::read_to_string(project.root.join("ran-here")).expect("the hook ran");
    let root = project.root.canonicalize().expect("root");
    assert_eq!(Path::new(here.trim()).canonicalize().expect("cwd"), root);
}

#[test]
fn no_pre_tick_runs() {
    let project = project();
    assert!(!project.root.join(".zaf/actions/hooks").exists());

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!project.gh_calls().is_empty(), "the tick read the forge");
    assert!(
        logs(&project.root).is_empty(),
        "no hook, no log: {:?}",
        logs(&project.root)
    );
}
