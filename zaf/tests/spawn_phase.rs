//! `zaf spawn phase <task> <action>`, through the binary: the caller gets its prompt back while
//! the action runs as a process of its own, which sees `ZAF_RUN`, `ZAF_SIZE` and `ZAF_INSTANCE`;
//! an action that is not the task's current phase, or a phase that already runs, is refused with
//! exit 1; and the standing of a started phase reads `Running` from its run folder.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use common::{Project, outcome};
use zaf::adapters::process;
use zaf::app::actions::{
    ActionKind, ListedPhase, PHASES_DIR, PhaseEnd, Standing, derive_standing, gather_standing_facts,
};
use zaf::domain::TicketId;
use zaf::fakes::FakeForge;
use zaf::manifest::ActionName;

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [phases]\nTask = [\"implement\", \"review\"]\n";

const TASK_BODY: &str = "Goal: Members export their reading list as CSV.\n\
    Owned paths: src/export/**\n\
    Interfaces: none\n\
    Design pages: none\n\
    Architecture: app over ports\n\
    Acceptance:\n  - an empty list exports the header row only\n\
    Tests: beside the use case\n\
    Security trigger: no\n\
    Model: sonnet\n\
    Size: S\n\
    Blocked by: none\n";

/// A project whose task 4 (of epic 1) stands at its first phase, `implement`. The action records
/// what it sees in `seen`, says it started in `started`, waits for `release`, then says `ended`.
fn project() -> Project {
    let root_dir = |project: &Project| project.root.display().to_string();
    project_running(|project| {
        format!(
            "cd '{root}'\n\
             echo \"$ZAF_RUN $ZAF_SIZE $ZAF_INSTANCE $1\" > seen.new\n\
             mv seen.new seen\n\
             touch started\n\
             while [ ! -f release ]; do sleep 0.05; done\n\
             touch ended\n",
            root = root_dir(project)
        )
    })
}

/// The same project, whose `implement` runs the body `body` makes of it.
fn project_running(body: impl Fn(&Project) -> String) -> Project {
    let mut project = Project::new(MANIFEST);
    let ticket = serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "ready"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
        "issueType": {"name": "Task"},
    });
    project.on("issue view 4 *", &ticket.to_string());
    project.provide_slice_actions();
    action(
        &project,
        "implement",
        "# zaf kind: model\n# zaf ends: exit\n",
        &body(&project),
    );
    action(
        &project,
        "review",
        "# zaf kind: wait\n# zaf ends: review-posted\n",
        "true\n",
    );
    project
}

/// Writes `.zaf/actions/<name>` with `metadata` under its description, then `body`.
fn action(project: &Project, name: &str, metadata: &str, body: &str) {
    let dir = project.root.join(".zaf/actions");
    fs::create_dir_all(&dir).expect("actions");
    let file = dir.join(name);
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: {name}\n# zaf 1: failed\n{metadata}{body}"),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// Waits until `file` exists; a test that waits for nothing would hide a hang.
fn await_file(file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !file.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            file.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Lets the running action end and waits for it to say so.
fn release(project: &Project) {
    fs::write(project.root.join("release"), "").expect("release");
    await_file(&project.root.join("ended"));
}

/// The phase run folders there are.
fn run_folders(project: &Project) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(project.root.join(".zaf").join(PHASES_DIR)) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect()
}

fn list() -> Vec<ListedPhase> {
    vec![
        ListedPhase {
            action: ActionName::from("implement"),
            kind: ActionKind::Model,
            ends: PhaseEnd::Exit,
            on_fail: None,
        },
        ListedPhase {
            action: ActionName::from("review"),
            kind: ActionKind::Wait,
            ends: PhaseEnd::ReviewPosted,
            on_fail: None,
        },
    ]
}

fn standing(project: &Project) -> Standing {
    let root = project.root.canonicalize().expect("root");
    let facts = gather_standing_facts(
        &FakeForge::default(),
        &root,
        TicketId::from(4),
        SystemTime::now(),
        &process::alive,
    )
    .expect("facts");
    derive_standing(&list(), &facts)
}

#[test]
fn the_caller_does_not_wait() {
    let project = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));

    // The verb returned while the action, which waits for a release that has not come, goes on.
    assert_eq!(code, 0, "{stdout}{stderr}");
    await_file(&project.root.join("started"));
    assert!(
        !project.root.join("ended").exists(),
        "the verb returned before the action ended"
    );
    release(&project);
}

#[test]
fn the_run_sees_size_run_and_instance() {
    let project = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    await_file(&project.root.join("started"));
    let folders = run_folders(&project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    let seen = fs::read_to_string(project.root.join("seen")).expect("seen");
    let fields: Vec<&str> = seen.split_whitespace().collect();
    // ZAF_RUN is the run folder, absolute; ZAF_SIZE the task's size; ZAF_INSTANCE the form of an
    // instance, its number the spawner's next one; the task is the action's argument.
    assert_eq!(
        Path::new(fields[0]).canonicalize().expect("run"),
        folders[0].canonicalize().expect("folder")
    );
    assert!(Path::new(fields[0]).is_absolute(), "{seen}");
    assert_eq!(fields[1], "S", "{seen}");
    assert_eq!(
        fields[2..7].join(" "),
        "implement instance 1 on #1",
        "{seen}"
    );
    assert_eq!(fields[7], "4", "{seen}");
    // The folder is in the format the standing reads.
    assert_eq!(
        fs::read_to_string(folders[0].join("task"))
            .expect("task")
            .trim(),
        "4"
    );
    assert_eq!(
        fs::read_to_string(folders[0].join("action"))
            .expect("action")
            .trim(),
        "implement"
    );
    release(&project);
}

#[test]
fn a_phase_not_current_is_refused() {
    let project = project();

    // `review` is listed, but the task has not got to it.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "review"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("not #4's current phase"), "{stderr}");
    // An action the task's list does not hold is no phase either.
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "slice-pr"]));
    assert_eq!(code, 1, "{stdout}{stderr}");

    assert!(run_folders(&project).is_empty(), "nothing was written");
    assert!(!project.root.join("started").exists());
}

#[test]
fn a_running_phase_is_not_started_twice() {
    let project = project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    await_file(&project.root.join("started"));

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("already runs"), "{stderr}");
    assert_eq!(run_folders(&project).len(), 1, "no second run folder");
    release(&project);
}

#[test]
fn a_started_phase_reads_running() {
    let project = project();
    assert!(matches!(standing(&project), Standing::Ready { .. }));

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    // From the moment the verb returns, and for as long as the action runs.
    assert_eq!(
        standing(&project),
        Standing::Running {
            action: ActionName::from("implement")
        }
    );
    await_file(&project.root.join("started"));
    assert_eq!(
        standing(&project),
        Standing::Running {
            action: ActionName::from("implement")
        }
    );
    release(&project);
    // The launcher goes once the action has.
    let deadline = Instant::now() + Duration::from_secs(20);
    while matches!(standing(&project), Standing::Running { .. }) {
        assert!(
            Instant::now() < deadline,
            "the run never stopped reading running"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Starts `implement` for task 4 and returns its run folder, once the verb has returned.
fn start(project: &Project) -> PathBuf {
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let folders = run_folders(project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    folders[0].clone()
}

/// Waits for the launcher to record the run's exit and returns what the file holds.
fn exit_of(run: &Path) -> String {
    await_file(&run.join("exit"));
    fs::read_to_string(run.join("exit")).expect("exit")
}

#[test]
fn the_exit_code_is_recorded() {
    let project = project_running(|_| "exit 3\n".to_owned());

    let run = start(&project);

    // The decimal code and a newline; the launcher is not told the code, it reads the child's.
    assert_eq!(exit_of(&run), "3\n");
    assert!(!run.join("exit.new").exists());
}

#[test]
fn an_exit_zero_ends_the_phase_on_read() {
    let project = project_running(|_| "exit 0\n".to_owned());
    let run = start(&project);

    assert_eq!(exit_of(&run), "0\n");

    // The exit end of `implement` holds, so the task's current phase is the next one.
    let Standing::Ready { action, .. } = standing(&project) else {
        panic!("the task is not ready for its next phase");
    };
    assert_eq!(action, ActionName::from("review"));
}

#[test]
fn a_failed_exit_does_not_end_the_phase_on_read() {
    let project = project_running(|_| "exit 3\n".to_owned());
    let run = start(&project);

    assert_eq!(exit_of(&run), "3\n");

    let Standing::Ready { action, .. } = standing(&project) else {
        panic!("the task is not ready");
    };
    assert_eq!(action, ActionName::from("implement"));
}

#[test]
fn a_killed_phase_records_its_signal() {
    let project = project();
    let run = start(&project);
    await_file(&project.root.join("started"));
    // The `pid` file holds the action's process, the one the launcher waits on.
    let holder = fs::read_to_string(run.join("pid")).expect("pid");
    let pid = holder.split_whitespace().next().expect("pid").to_owned();

    let status = std::process::Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .expect("kill");
    assert!(status.success());

    assert_eq!(exit_of(&run), "143\n");
    // Let the orphaned action end, so the test leaves nothing running.
    release(&project);
}

/// Writes the project's `post-phase` hook: `body` under its description.
fn post_phase(project: &Project, body: &str) {
    let hooks = project.root.join(".zaf/actions/hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let file = hooks.join("post-phase");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n# zaf description: after each phase\n# zaf 1: the hook failed\n{body}\n"
        ),
    )
    .expect("hook");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// Waits until the phase no longer reads running: the launcher, hook included, has gone.
fn await_not_running(project: &Project) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while matches!(standing(project), Standing::Running { .. }) {
        assert!(Instant::now() < deadline, "the run never stopped running");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn post_phase_gets_the_exit_code() {
    let project = project_running(|_| "exit 3\n".to_owned());
    // The hook keeps what it is told, and whether the exit was already recorded when it ran.
    post_phase(
        &project,
        "cat > post-phase-stdin\ncp \"$ZAF_RUN/exit\" exit-at-hook",
    );

    let run = start(&project);

    await_file(&project.root.join("exit-at-hook"));
    let told = fs::read_to_string(project.root.join("post-phase-stdin")).expect("stdin");
    let told: serde_json::Value = serde_json::from_str(&told).expect("json");
    assert_eq!(
        told,
        serde_json::json!({"task": 4, "action": "implement", "exit": 3})
    );
    // The exit file was there before the hook ran, and says what the hook was told.
    assert_eq!(
        fs::read_to_string(project.root.join("exit-at-hook")).expect("exit"),
        "3\n"
    );
    assert_eq!(exit_of(&run), "3\n");
}

#[test]
fn a_failing_post_phase_changes_nothing() {
    let project = project_running(|_| "exit 3\n".to_owned());
    post_phase(&project, "echo the hook broke\nexit 1");

    let run = start(&project);

    // The failure is in the run folder, with what the hook said.
    await_file(&run.join("post-phase.log"));
    let log = fs::read_to_string(run.join("post-phase.log")).expect("log");
    assert!(log.contains("the hook broke"), "{log}");
    // Nothing else moved: the exit is the child's, the phase did not end, and the run stopped.
    assert_eq!(exit_of(&run), "3\n");
    await_not_running(&project);
    let Standing::Ready { action, .. } = standing(&project) else {
        panic!("the task is not ready");
    };
    assert_eq!(action, ActionName::from("implement"));
}

/// Writes the project's `pre-tick` hook: `body` under its description.
fn pre_tick(project: &Project, body: &str) {
    let hooks = project.root.join(".zaf/actions/hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let file = hooks.join("pre-tick");
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\n{body}\n"),
    )
    .expect("hook");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

#[test]
fn a_phase_end_runs_a_tick() {
    let mut project = project_running(|_| "exit 0\n".to_owned());
    // The tick needs the milestone; the launcher passes on the one it has.
    project.env("ZAF_VERSION", "0.3.0");
    // Each step says its name in `order`: the tick is told apart by its `pre-tick`.
    post_phase(&project, "echo post-phase >> order");
    pre_tick(&project, "echo pre-tick >> order\ntouch ticked");

    let run = start(&project);

    await_file(&project.root.join("ticked"));
    // The phase ended, its `post-phase` ran, and only then did the tick.
    assert_eq!(exit_of(&run), "0\n");
    let order = fs::read_to_string(project.root.join("order")).expect("order");
    assert_eq!(order, "post-phase\npre-tick\n");
}

#[test]
fn a_failing_tick_changes_nothing() {
    // The manifest has no runner, so the tick the launcher asks for fails; the launcher's exit and
    // the recorded one stay the child's.
    let mut project = project_running(|_| "exit 3\n".to_owned());
    project.env("ZAF_VERSION", "0.3.0");
    pre_tick(&project, "touch ticked");

    let run = start(&project);

    await_file(&project.root.join("ticked"));
    assert_eq!(exit_of(&run), "3\n");
    await_not_running(&project);
}

#[test]
fn no_hook_no_run() {
    let project = project_running(|_| "exit 3\n".to_owned());
    assert!(!project.root.join(".zaf/actions/hooks").exists());

    let run = start(&project);

    assert_eq!(exit_of(&run), "3\n");
    await_not_running(&project);
    // Give a launcher that wrongly ran a hook the time to leave its mark.
    std::thread::sleep(Duration::from_millis(500));
    assert!(!run.join("post-phase.log").exists());
    let logs: Vec<String> = fs::read_dir(project.root.join(".zaf/logs"))
        .map(|entries| {
            entries
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        logs.iter().all(|name| !name.contains("post-phase")),
        "{logs:?}"
    );
}
