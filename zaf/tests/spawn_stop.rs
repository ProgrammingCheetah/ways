//! `zaf spawn stop <task>`: ends the instance running a task, with its group, and releases the
//! task's claim, so a later tick starts it again; a task with no live instance is refused with
//! exit 1 and nothing changes.

mod common;

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use common::{Project, outcome};
use zaf::adapters::process;
use zaf::app::actions::{ActionKind, ListedPhase, PHASES_DIR, PhaseEnd};
use zaf::app::schedule::gather::phases_of;
use zaf::app::schedule::record::read_runs;
use zaf::app::spawn::stop::{self, GRACE};
use zaf::cli::spawn_stop::Machine;
use zaf::domain::schedule::{Place, TicketFacts, place};
use zaf::domain::{Instance, InstanceNumber, RoleName, TaskSize, Ticket, TicketId, TicketState};
use zaf::fakes::FakeBoard;
use zaf::manifest::ActionName;
use zaf::ports::Board;

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n\n\
    [phases]\nTask = [\"implement\", \"review\"]\n";

const TASK: u64 = 4;

/// Task 4 of epic 1, claimed by the instance the run folder names, as `gh issue view` prints it.
const CLAIMED: &str = r#"{"number":4,"title":"A task","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[{"body":"claimed by implement instance 3 on #1 at 2027-01-15T08:00:00Z"}]}"#;

/// A long-lived process of its own process group, as a launcher's action is: it, and what it
/// starts, goes with the group.
fn live_child() -> Child {
    Command::new("sh")
        .args(["-c", "sleep 60 & wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .expect("child")
}

/// Writes the phase run folder of task 4 under `state_dir`, held by `child`; returns it.
fn write_run(state_dir: &Path, child: &Child) -> PathBuf {
    let run = state_dir
        .join(PHASES_DIR)
        .join("implement-instance-3-on-1-task-4");
    fs::create_dir_all(&run).expect("run folder");
    let holder = process::holder_of(child.id()).expect("the child runs");
    fs::write(run.join("action"), "implement").expect("action");
    fs::write(run.join("head"), "").expect("head");
    fs::write(run.join("size"), "S").expect("size");
    fs::write(run.join("instance"), "implement instance 3 on #1").expect("instance");
    fs::write(run.join("pid"), holder.to_string()).expect("pid");
    fs::write(run.join("task"), TASK.to_string()).expect("task");
    run
}

/// Waits for the child to be reaped, which is when it is surely gone; a hang fails the test.
fn reaped(mut child: Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            return status;
        }
        assert!(Instant::now() < deadline, "the child never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn instance() -> Instance {
    Instance {
        role: RoleName::from("implement"),
        epic: TicketId::from(1),
        number: InstanceNumber::from(3),
    }
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

/// Where the core places the task from its run folders under `root` alone (no pull request, no
/// epic, no question), as the tick places it.
fn place_of(root: &Path) -> Place {
    let task = TicketId::from(TASK);
    let runs = read_runs(&root.join(".zaf").join(PHASES_DIR), task, &process::alive).expect("runs");
    let facts = TicketFacts {
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
    };
    place(&facts, SystemTime::now())
}

#[test]
fn stopped_task_is_ready_again() {
    let root = tempfile::tempdir().expect("root");
    let state_dir = root.path().join(".zaf");
    let child = live_child();
    write_run(&state_dir, &child);
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        id: TicketId::from(TASK),
        epic: Some(TicketId::from(1)),
        title: "A task".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    board
        .claim(TicketId::from(TASK), &instance())
        .expect("claim");
    assert!(board.ready().expect("ready").is_empty(), "claimed");
    assert!(
        matches!(place_of(root.path()), Place::Running(_)),
        "its process runs"
    );

    let stopped =
        stop::stop(TicketId::from(TASK), &state_dir, &board, &Machine, GRACE).expect("stops");

    // The process is gone, killed by the terminate signal.
    assert_eq!(stopped.runs.len(), 1);
    assert!(!stopped.runs[0].killed, "it ended within the grace");
    let status = reaped(child);
    assert!(!status.success(), "{status:?}");
    // The board shows the task released and ready: a tick plans from `ready`.
    assert_eq!(
        board
            .ready()
            .expect("ready")
            .iter()
            .map(|ticket| ticket.id)
            .collect::<Vec<_>>(),
        vec![TicketId::from(TASK)]
    );
    assert!(board.claims().expect("claims").is_empty());
    // And its phase is not running: the core places it in the lane at `implement` again.
    assert!(
        matches!(
            place_of(root.path()),
            Place::InLane { ref next, .. } if next.as_str() == "implement"
        ),
        "{:?}",
        place_of(root.path())
    );
}

#[test]
fn a_child_that_shrugs_off_terminate_is_killed() {
    let root = tempfile::tempdir().expect("root");
    let state_dir = root.path().join(".zaf");
    let child = Command::new("sh")
        .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .expect("child");
    // Let the shell install its trap before the signal comes.
    std::thread::sleep(Duration::from_millis(300));
    write_run(&state_dir, &child);
    let board = FakeBoard::default();

    let stopped = stop::stop(
        TicketId::from(TASK),
        &state_dir,
        &board,
        &Machine,
        Duration::from_millis(200),
    )
    .expect("stops");

    assert!(stopped.runs[0].killed);
    assert!(!reaped(child).success());
}

#[test]
fn the_verb_stops_and_releases() {
    let mut project = Project::new(MANIFEST);
    project.on(
        "issue list *",
        r#"[{"number":4,"labels":[{"name":"claimed"}]}]"#,
    );
    project.on("issue view 4 *", CLAIMED);
    project.on("issue edit *", "");
    project.on("issue comment *", "");
    let child = live_child();
    write_run(&project.root.join(".zaf"), &child);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "stop", "4"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("stopped implement instance 3 on #1"),
        "{stdout}"
    );
    assert!(reaped(child).code().is_none(), "ended by a signal");
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call
                .contains("issue comment 4 --body released from implement instance 3 on #1")),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue edit 4") && call.contains("--add-label ready")),
        "{calls:?}"
    );
}

#[test]
fn no_instance_exits_one_and_changes_nothing() {
    let project = Project::new(MANIFEST);

    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "stop", "4"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("no instance is running"), "{stderr}");
    assert!(project.gh_calls().is_empty(), "{:?}", project.gh_calls());

    // A run folder whose process ended is no instance either.
    let child = live_child();
    let run = write_run(&project.root.join(".zaf"), &child);
    let mut child = child;
    child.kill().expect("kill");
    child.wait().expect("wait");
    let (code, _, stderr) = outcome(&project.zaf(&["spawn", "stop", "4"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(project.gh_calls().is_empty());
    assert!(run.join("pid").exists(), "the run folder is left as it was");
}
