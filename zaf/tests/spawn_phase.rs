//! `zaf spawn phase <task> <action>`, through the binary: the caller gets its prompt back while
//! the action runs as a process of its own, which sees `ZAF_RUN`, `ZAF_SIZE` and `ZAF_INSTANCE`;
//! an action that is not the task's current phase, a phase that already runs, a task whose epic
//! is unsigned or not challenged yet, and a task of no epic are refused with exit 1; and the core
//! places a task whose phase was started as running, from its run folder. Two starts of a model
//! phase never go one over `[spawn] slots`, and a phase that ends asks for a tick: the running
//! scheduler through its request file, else a tick of its own.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use common::{Project, outcome, scheduler_up};
use zaf::adapters::process;
use zaf::app::actions::{ActionKind, ListedPhase, PHASES_DIR, PhaseEnd};
use zaf::app::schedule::gather::phases_of;
use zaf::app::schedule::record::read_runs;
use zaf::domain::schedule::{Place, TicketFacts, place};
use zaf::domain::{BodyHash, TaskSize, TicketBody, TicketId};
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
    project_of(MANIFEST, body)
}

/// The same project with `manifest` as its manifest.
fn project_of(manifest: &str, body: impl Fn(&Project) -> String) -> Project {
    let mut project = Project::new(manifest);
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
    // A start gathers the task from the open tickets, as the tick does.
    project.on(
        "issue list --label task --state open *",
        &format!("[{ticket}]"),
    );
    // And the open epics: none here, so no epic holds the task.
    project.on("issue list --label epic *", "[]");
    project.on("issue list --label task --state all *", "[]");
    // And the claims, to release one whose instance has ended: none here.
    project.on("issue list --label claimed *", "[]");
    // A start reads the open questions: a run that raised one waits on it.
    project.on("issue list --label question *", "[]");
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

/// Where the core places task 4 from its run folders alone (no pull request, no epic, no
/// question), as the tick and `zaf spawn phase` place it.
fn place_of(project: &Project) -> Place {
    let root = project.root.canonicalize().expect("root");
    let task = TicketId::from(4);
    let runs = read_runs(&root.join(".zaf").join(PHASES_DIR), task, &process::alive).expect("runs");
    place(&facts(task, runs), SystemTime::now())
}

/// The facts of `task` with the list [`list`] and `runs`, and nothing else.
fn facts(task: TicketId, runs: Vec<zaf::domain::schedule::RunRecord>) -> TicketFacts {
    TicketFacts {
        ticket: task,
        epic: None,
        open_questions: std::collections::BTreeSet::new(),
        dependencies: Vec::new(),
        pull: None,
        runs,
        phases: phases_of(&list()),
        size: TaskSize::S,
        priority: None,
        ruled: Vec::new(),
        ready_since: None,
        merging: Default::default(),
        no_check_ruled: None,
        trees: Default::default(),
    }
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
    // The folder is in the format the record reads.
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
    assert!(matches!(place_of(&project), Place::InLane { .. }));

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    // From the moment the verb returns, and for as long as the action runs.
    assert!(
        matches!(place_of(&project), Place::Running(ref run) if run.phase == ActionName::from("implement")),
        "{:?}",
        place_of(&project)
    );
    await_file(&project.root.join("started"));
    assert!(
        matches!(place_of(&project), Place::Running(ref run) if run.phase == ActionName::from("implement")),
        "{:?}",
        place_of(&project)
    );
    release(&project);
    // The launcher goes once the action has.
    let deadline = Instant::now() + Duration::from_secs(20);
    while matches!(place_of(&project), Place::Running(_)) {
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

/// A body that records pull request #12 for task 4, as `slice-pr` would, then exits 0.
fn opens_pull_request_12(project: &Project) -> String {
    format!(
        "mkdir -p '{root}/.zaf/slices'\nprintf 12 > '{root}/.zaf/slices/4.pr'\nexit 0\n",
        root = project.root.display()
    )
}

/// What the run folder's `head-end` holds; none when there is no such file.
fn head_end_of(run: &Path) -> Option<String> {
    fs::read_to_string(run.join("head-end")).ok()
}

#[test]
fn the_head_a_phase_left_is_recorded() {
    let mut project = project_running(opens_pull_request_12);
    project.on("pr view 12 --json headRefOid *", "abc123\n");

    let run = start(&project);

    // Read once the action has ended, and written before the exit, which comes last.
    assert_eq!(exit_of(&run), "0\n");
    assert_eq!(head_end_of(&run).as_deref(), Some("abc123"));
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("pr view 12 --json headRefOid")),
        "{calls:?}"
    );
}

#[test]
fn a_head_that_cannot_be_read_leaves_no_head_end_and_the_exit_is_still_recorded() {
    let mut project = project_running(opens_pull_request_12);
    project.fail_on("pr view 12 *", "HTTP 502: Bad Gateway");

    let run = start(&project);

    assert_eq!(exit_of(&run), "0\n");
    assert_eq!(head_end_of(&run), None, "an unknown head is no file");
}

#[test]
fn a_phase_with_no_pull_request_leaves_an_empty_head_end() {
    let project = project_running(|_| "exit 4\n".to_owned());

    let run = start(&project);

    assert_eq!(exit_of(&run), "4\n");
    assert_eq!(
        head_end_of(&run).as_deref(),
        Some(""),
        "no pull request, no head"
    );
    assert!(!run.join("questions").exists(), "the run raised none");
}

#[test]
fn a_question_asked_inside_a_phase_is_recorded_in_its_run() {
    let mut project = project_running(|project| {
        format!(
            "cd '{root}'\n\
             zaf board question 4 'Which way does the export go?' > asked 2>&1\n\
             exit 0\n",
            root = project.root.display()
        )
    });
    project
        .env("PATH", &common::path_with_zaf(&[&common::fixtures()]))
        .on("issue list --label question *", "[]")
        .on(
            "issue create *",
            "https://github.com/example/tracker/issues/30\n",
        )
        .on("issue edit *", "");

    let run = start(&project);

    assert_eq!(exit_of(&run), "0\n");
    let asked = fs::read_to_string(project.root.join("asked")).expect("asked");
    assert_eq!(asked, "question #30 on #4\n");
    assert_eq!(
        fs::read_to_string(run.join("questions")).expect("questions"),
        "30\n"
    );
    // Outside a run, the same question records nothing anywhere.
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["board", "question", "4", "Which way does the export go?"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        fs::read_to_string(run.join("questions")).expect("questions"),
        "30\n"
    );
}

#[test]
fn an_exit_zero_ends_the_phase_on_read() {
    let project = project_running(|_| "exit 0\n".to_owned());
    let run = start(&project);

    assert_eq!(exit_of(&run), "0\n");

    // The exit end of `implement` holds, so the task's current phase is the next one.
    let Place::InLane { next: action, .. } = place_of(&project) else {
        panic!("the task is not ready for its next phase");
    };
    assert_eq!(action, ActionName::from("review"));
}

#[test]
fn a_failed_exit_does_not_end_the_phase_on_read() {
    let project = project_running(|_| "exit 3\n".to_owned());
    let run = start(&project);

    assert_eq!(exit_of(&run), "3\n");

    let Place::InLane { next: action, .. } = place_of(&project) else {
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
    // It first holds the launcher's own, until the launcher has read its child's start: the action
    // may say it started before that, so wait for the process that runs `zaf do`.
    let deadline = Instant::now() + Duration::from_secs(20);
    let pid = loop {
        let holder = fs::read_to_string(run.join("pid")).expect("pid");
        let pid = holder.split_whitespace().next().expect("pid").to_owned();
        let command = std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pid])
            .output()
            .expect("ps");
        if String::from_utf8_lossy(&command.stdout).contains(" do implement") {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "the pid file never named the action"
        );
        std::thread::sleep(Duration::from_millis(50));
    };

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
    while matches!(place_of(project), Place::Running(_)) {
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
    let Place::InLane { next: action, .. } = place_of(&project) else {
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

/// The open epic #1 as `issue list --label epic` prints it: signed for its body as it reads when
/// `signed`, and with the challenger's report on it when `challenged`.
fn epic_one(signed: bool, challenged: bool) -> String {
    let body = "Members export their reading list as CSV.";
    let mut comments = Vec::new();
    if signed {
        let hash = BodyHash::of(&TicketBody::from(body));
        comments.push(serde_json::json!({
            "body": format!(
                "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                hash.as_str()
            ),
            "createdAt": "2026-11-01T00:00:00Z",
        }));
    }
    if challenged {
        comments.push(serde_json::json!({
            "body": "<!-- zaf:report challenger instance 3 on #1 -->\nEvery task is exact.",
            "createdAt": "2026-11-02T00:00:00Z",
        }));
    }
    serde_json::json!([{
        "number": 1,
        "title": "Export",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": comments,
    }])
    .to_string()
}

#[test]
fn a_ticket_of_an_unsigned_epic_is_refused_and_one_of_a_signed_and_challenged_epic_starts() {
    let mut project = project();

    // Unsigned: refused in the gate's words, and nothing is written.
    project.on_first("issue list --label epic *", &epic_one(false, false));
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains(
            "implement is not #4's current phase: implement waits: its epic #1 is not signed"
        ),
        "{stderr}"
    );
    assert!(run_folders(&project).is_empty(), "nothing was written");

    // Signed, not challenged yet: still refused.
    project.on_first("issue list --label epic *", &epic_one(true, false));
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains("implement waits: its epic #1 is not challenged yet"),
        "{stderr}"
    );
    assert!(run_folders(&project).is_empty(), "nothing was written");

    // Signed and challenged: it starts.
    project.on_first("issue list --label epic *", &epic_one(true, true));
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    await_file(&project.root.join("started"));
    assert_eq!(run_folders(&project).len(), 1);
    release(&project);
}

#[test]
fn a_ticket_with_no_epic_is_refused() {
    let mut project = project();
    // Task 4 as filed by hand: no parent epic.
    let ticket = serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "ready"}],
        "state": "OPEN",
        "comments": [],
        "issueType": {"name": "Task"},
    });
    project
        .on_first("issue view 4 *", &ticket.to_string())
        .on_first(
            "issue list --label task --state open *",
            &format!("[{ticket}]"),
        );

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains("#4 cannot start: it is not a task of an epic"),
        "{stderr}"
    );
    assert!(run_folders(&project).is_empty(), "nothing was written");
    assert!(!project.root.join("started").exists());
}

#[test]
fn spawn_phase_starts_a_second_run_after_the_first_instance_ended() {
    let mut project = project_running(|_| "exit 3\n".to_owned());
    // The first run, implement instance 1, claimed the task as the pack's implement does.
    let claimed = serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": TASK_BODY,
        "labels": [{"name": "task"}, {"name": "claimed"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [{"body": "claimed by implement instance 1 on #1 at 2027-01-15T08:00:00Z"}],
        "issueType": {"name": "Task"},
    });
    project
        .on_first("issue view 4 *", &claimed.to_string())
        .on_first(
            "issue list --label task --state open *",
            &format!("[{claimed}]"),
        )
        .on_first("issue list --label claimed *", r#"[{"number":4}]"#)
        .on_first("issue edit *", "")
        .on_first("issue comment *", "");
    let first = start(&project);
    assert_eq!(exit_of(&first), "3\n");
    await_not_running(&project);
    let calls = project.gh_calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("issue edit")),
        "the first start releases nothing: its instance had not run yet: {calls:?}"
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "implement"]));

    // The first instance has ended, so its claim is released before the second run starts.
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("released #4 from implement instance 1 on #1"),
        "{stdout}"
    );
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue edit 4") && call.contains("claimed")),
        "{calls:?}"
    );
    assert_eq!(run_folders(&project).len(), 2, "a second run folder");
}

#[test]
fn a_phase_end_with_no_scheduler_still_starts_a_tick() {
    let mut project = project_running(|_| "exit 0\n".to_owned());
    project.env("ZAF_VERSION", "0.3.0");
    pre_tick(&project, "touch ticked");

    let run = start(&project);

    // No scheduler holds the lock: the launcher runs the tick itself, and asks no one.
    await_file(&project.root.join("ticked"));
    assert_eq!(exit_of(&run), "0\n");
    await_not_running(&project);
    assert!(!project.root.join(".zaf/tick.request").exists());
}

#[test]
fn a_phase_end_asks_the_running_scheduler_and_runs_no_tick_itself() {
    let mut project = project_running(|_| "exit 0\n".to_owned());
    project.env("ZAF_VERSION", "0.3.0");
    pre_tick(&project, "touch ticked");
    let _scheduler = scheduler_up(&project);

    let run = start(&project);

    let request = project.root.join(".zaf/tick.request");
    await_file(&request);
    assert_eq!(exit_of(&run), "0\n");
    await_not_running(&project);
    let line = fs::read_to_string(&request).expect("the request");
    assert!(line.ends_with(" phase-end implement #4\n"), "{line:?}");
    assert_eq!(line.lines().count(), 1, "{line:?}");
    // The scheduler ticks; the launcher did not.
    assert!(!project.root.join("ticked").exists());
}

#[test]
fn a_phase_end_that_cannot_ask_says_so_and_never_claims_a_tick() {
    let mut project = project_running(|_| "exit 0\n".to_owned());
    project.env("ZAF_VERSION", "0.3.0");
    pre_tick(&project, "touch ticked");
    let _scheduler = scheduler_up(&project);
    // The request file cannot be written: something else is in its place.
    fs::create_dir_all(project.root.join(".zaf/tick.request")).expect("in the way");

    let run = start(&project);

    // The launcher has no stderr anyone reads: the run folder says no tick was asked for.
    let log = run.join("post-phase.log");
    await_file(&log);
    assert_eq!(exit_of(&run), "0\n");
    await_not_running(&project);
    let said = fs::read_to_string(&log).expect("the log");
    assert!(
        said.starts_with(&format!(
            "the running scheduler (pid {}, since ",
            std::process::id()
        )) && said.ends_with("; no tick was asked for\n"),
        "{said}"
    );
    // Give a launcher that wrongly ticked the time to leave its mark.
    std::thread::sleep(Duration::from_millis(500));
    assert!(!project.root.join("ticked").exists());
}

#[test]
fn a_tick_whose_binary_was_replaced_on_disk_still_starts_its_phase() {
    // A tick needs a runner and current profiles, though it starts no launch here.
    let manifest = format!(
        "{MANIFEST}\n[runners.claude]\ncommand = \"true\"\n\n\
         [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\n\
         batch = 2\n"
    );
    let root_dir = |project: &Project| project.root.display().to_string();
    let mut project = project_of(&manifest, |project| {
        format!(
            "cd '{root}'\ntouch started\nwhile [ ! -f release ]; do sleep 0.05; done\ntouch ended\n",
            root = root_dir(project)
        )
    });
    // Whatever else the tick reads is empty.
    project.on("*", "[]");
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project.env("ZAF_VERSION", "0.3.0");
    let root = project.root.display().to_string();
    // The tick waits in its `pre-tick` hook until the binary it runs has been replaced.
    pre_tick(
        &project,
        &format!(
            "touch '{root}/paused'\n\
             i=0\nwhile [ ! -f '{root}/go' ] && [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done"
        ),
    );
    let bin = project.temp.path().join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let zaf = bin.join("zaf");
    let install = || {
        let staged = bin.join("zaf.new");
        fs::copy(env!("CARGO_BIN_EXE_zaf"), &staged).expect("copy the binary");
        fs::rename(&staged, &zaf).expect("into place");
    };
    install();
    let tick = common::Running::start(project.command_of(
        &zaf,
        &project.root,
        &["spawn", "--once", "--version", "0.3.0"],
    ));
    await_file(&project.root.join("paused"));

    // Replaced as an install replaces it: the running image's file is gone, a new one in its place.
    install();
    fs::write(project.root.join("go"), "").expect("go");

    tick.wait_for_lines("started implement #4", 1);
    await_file(&project.root.join("started"));
    let folders = run_folders(&project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    release(&project);
    exit_of(&folders[0]);
    await_not_running(&project);
}

#[test]
fn two_starts_cannot_go_one_over_the_slots() {
    let mut project = project();
    let mut manifest = MANIFEST.to_owned();
    manifest.push_str("\n[spawn]\nslots = 1\n");
    fs::write(project.root.join(".zaf.toml"), manifest).expect("one slot");
    // A second task at `implement`, read by both starts.
    let ticket = |number: u64| {
        serde_json::json!({
            "number": number,
            "title": format!("Task {number}"),
            "body": TASK_BODY,
            "labels": [{"name": "task"}, {"name": "ready"}],
            "parent": {"number": 1},
            "state": "OPEN",
            "comments": [],
            "issueType": {"name": "Task"},
        })
    };
    project.on_first("issue view 5 *", &ticket(5).to_string());
    project.on_first(
        "issue list --label task --state open *",
        &serde_json::json!([ticket(4), ticket(5)]).to_string(),
    );

    // Both at once: each would see the one slot free before the other started.
    let starts: Vec<_> = ["4", "5"]
        .iter()
        .map(|task| {
            project
                .command_in(&project.root, &["spawn", "phase", task, "implement"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("start")
        })
        .collect();
    let outcomes: Vec<_> = starts
        .into_iter()
        .map(|child| outcome(&child.wait_with_output().expect("output")))
        .collect();

    let all = format!("{outcomes:?}");
    let started = outcomes.iter().filter(|(code, _, _)| *code == 0).count();
    assert_eq!(started, 1, "{all}");
    let refused = outcomes
        .iter()
        .find(|(code, _, _)| *code == 1)
        .unwrap_or_else(|| panic!("none refused: {all}"));
    assert!(refused.2.contains("every slot is in use (1 of 1)"), "{all}");
    assert_eq!(run_folders(&project).len(), 1, "{all}");
    release(&project);
}

/// The phases of a task that stands at its last: `review` done at the head, `merge` ends at
/// `merged`, the land phase.
const LAND_MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [phases]\nTask = [\"review\", \"merge\"]\n";

/// A project whose task 4 has pull request 12 at head `abc123`, reviewed there: its current phase
/// is `merge`, which ends at `merged` and runs `body`.
fn project_at_land(body: &str) -> Project {
    let mut project = project_of(LAND_MANIFEST, |_| "true\n".to_owned());
    let slices = project.root.join(".zaf/slices");
    fs::create_dir_all(&slices).expect("slices");
    fs::write(slices.join("4.pr"), "12").expect("record");
    project
        .on("pr view 12 --json headRefOid *", "abc123\n")
        .on(
            "pr view 12 --json state,createdAt,mergedAt,mergeable*",
            r#"{"state":"OPEN","createdAt":"2027-01-15T09:00:00Z","mergedAt":null,"mergeable":"MERGEABLE","headRefOid":"abc123"}"#,
        )
        .on("pr view 12 --json state *", r#"{"state":"OPEN"}"#)
        .on(
            "pr checks 12 *",
            r#"[{"name":"ci","bucket":"pass","completedAt":"2027-01-15T09:05:00Z"}]"#,
        )
        .on("api *pulls/12/files*", r#"[[{"filename":"a.txt"}]]"#)
        .on(
            "api *pulls/12/reviews*",
            r#"[[{"user":{"login":"r"},"body":"ok","submitted_at":"2027-01-15T09:10:00Z","commit_id":"abc123"}]]"#,
        )
        .on("api *", "[[]]");
    action(
        &project,
        "merge",
        "# zaf kind: wait\n# zaf ends: merged\n",
        body,
    );
    project
}

#[test]
fn land_run_carries_the_derived_head() {
    let project = project_at_land("");
    let root = project.root.display().to_string();
    action(
        &project,
        "merge",
        "# zaf kind: wait\n# zaf ends: merged\n",
        &format!(
            "printf '%s' \"${{ZAF_HEAD-unset}}\" > '{root}/head'\n\
             zaf do inner\n"
        ),
    );
    action(
        &project,
        "inner",
        "",
        &format!("printf '%s' \"${{ZAF_HEAD-unset}}\" > '{root}/nested'\n"),
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "phase", "4", "merge"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    let folders = run_folders(&project);
    assert_eq!(folders.len(), 1, "{folders:?}");
    assert_eq!(exit_of(&folders[0]), "0\n");
    // The head Land was derived at, to the action and to a `zaf do` it makes.
    let read = |name: &str| fs::read_to_string(project.root.join(name)).expect(name);
    assert_eq!(read("head"), "abc123");
    assert_eq!(read("nested"), "abc123");
    assert_eq!(
        fs::read_to_string(folders[0].join("land-head")).expect("land-head"),
        "abc123"
    );
}

#[test]
fn other_phases_get_no_zaf_head() {
    // `implement` is no land phase, whatever head the caller's environment holds.
    let mut project = project_running(|project| {
        format!(
            "printf '%s' \"${{ZAF_HEAD-unset}}\" > '{root}/head'\n",
            root = project.root.display()
        )
    });
    project.env("ZAF_HEAD", "forged");

    let run = start(&project);

    assert_eq!(exit_of(&run), "0\n");
    assert_eq!(
        fs::read_to_string(project.root.join("head")).expect("head"),
        "unset"
    );
    assert!(!run.join("land-head").exists());
}
