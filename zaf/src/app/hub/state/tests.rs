use std::time::{Duration, SystemTime};

use super::*;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{InstanceNumber, RoleName, Ticket};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs};

mod line;

const EPIC: u64 = 100;

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// A task body that follows the template.
fn body() -> &'static str {
    "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
     Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
     Model: sonnet\nSize: S\nBlocked by: —\n"
}

/// A task of the epic in `state`.
fn task(number: u64, title: &str, state: TicketState) -> Ticket {
    Ticket {
        id: TicketId::from(number),
        epic: Some(TicketId::from(EPIC)),
        title: title.into(),
        body: body().into(),
        state: Some(state),
        url: format!("https://board.invalid/tickets/{number}").into(),
        kind: None,
    }
}

fn instance(role: &str, number: u32) -> Instance {
    Instance {
        role: RoleName::from(role),
        epic: TicketId::from(EPIC),
        number: InstanceNumber::from(number),
    }
}

fn alive(_: &Holder) -> bool {
    true
}

/// What every test reads: the board, nothing on the forge, and the state folder.
struct World {
    board: FakeBoard,
    forge: FakeForge,
    vcs: FakeVcs,
    dir: tempfile::TempDir,
    records: SliceRecords,
    prices: PriceTable,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let board = FakeBoard::default();
        board.add_challenged_epic(TicketId::from(EPIC), "The hub");
        Self {
            records: SliceRecords::in_state_dir(dir.path()),
            board,
            forge: FakeForge::default(),
            vcs: FakeVcs::default(),
            prices: PriceTable::default(),
            dir,
        }
    }

    /// The inputs of a read at `now`, which a test may change before it gathers.
    fn inputs(&self, now: SystemTime) -> StateInputs<'_> {
        StateInputs {
            project: "example",
            root: self.dir.path(),
            state_dir: self.dir.path(),
            board: Ok(&self.board),
            forge: Some(&self.forge),
            vcs: Some(&self.vcs),
            records: &self.records,
            ledger: &[],
            prices: &self.prices,
            pull_base: Some("https://forge.invalid/example/repo/pulls/"),
            alive: &alive,
            now,
            places: None,
            gathered: None,
        }
    }

    fn gather(&self, now: SystemTime) -> ProjectState {
        gather_state(&self.inputs(now))
    }
}

#[test]
fn epics_with_stage_and_groups() {
    let world = World::new();
    world.board.add_ticket(task(1, "Read", TicketState::Done));
    world.board.add_ticket(task(2, "Write", TicketState::Ready));
    world.board.add_ticket(task(3, "Serve", TicketState::Ready));
    world.board.add_ticket(task(4, "Loose", TicketState::Ready));
    for number in [1, 2] {
        world.board.set_group(TicketId::from(number), "state");
    }
    world.board.set_group(TicketId::from(3), "http");

    let state = world.gather(at(1_000));

    assert_eq!(state.reason, None);
    assert_eq!(state.project, "example");
    assert_eq!(state.epics.len(), 1);
    let epic = &state.epics[0];
    assert_eq!(epic.number, EPIC);
    assert_eq!(epic.title, "The hub");
    assert_eq!(epic.stage, EpicStage::Challenged);
    assert_eq!((epic.done, epic.total), (1, 4));
    assert_eq!(
        epic.groups,
        [
            GroupState {
                name: "http".to_owned(),
                done: 0,
                total: 1
            },
            GroupState {
                name: "state".to_owned(),
                done: 1,
                total: 2
            },
        ]
    );
    assert_eq!(epic.dollars, 0.0);
}

#[test]
fn queue_states() {
    let world = World::new();
    world
        .board
        .add_ticket(task(1, "Ready one", TicketState::Ready));
    world
        .board
        .add_ticket(task(2, "Claimed one", TicketState::Ready));
    world
        .board
        .add_ticket(task(3, "Blocked one", TicketState::Blocked));
    world
        .board
        .add_ticket(task(4, "Asked one", TicketState::Question));
    world
        .board
        .add_ticket(task(5, "Done one", TicketState::Done));
    world
        .board
        .claim(TicketId::from(2), &instance("programmer", 7))
        .expect("claims");
    // A pull request recorded for the claimed task.
    std::fs::create_dir_all(world.dir.path().join("slices")).expect("slices");

    let state = world.gather(at(1_000));

    let queue: Vec<(u64, TicketState)> = state
        .queue
        .iter()
        .map(|task| (task.number, task.state))
        .collect();
    assert_eq!(
        queue,
        [
            (1, TicketState::Ready),
            (2, TicketState::Claimed),
            (3, TicketState::Blocked),
            (4, TicketState::Question),
        ],
        "a done task is not in the queue"
    );
    let ready = &state.queue[0];
    assert_eq!(ready.epic, EPIC);
    assert_eq!(
        ready.issue_url.as_deref(),
        Some("https://board.invalid/tickets/1")
    );
    assert_eq!(ready.phase, None);
    assert_eq!(ready.instance, None);
    let claimed = &state.queue[1];
    assert_eq!(
        claimed.instance.as_deref(),
        Some("programmer instance 7 on #100")
    );
    assert_eq!(claimed.phase.as_deref(), Some("claim"));
    assert_eq!(
        claimed.issue_url.as_deref(),
        Some("https://board.invalid/tickets/2")
    );
    assert_eq!(claimed.pull_request, None);
    assert_eq!(claimed.pull_request_url, None);
}

#[test]
fn running_instance() {
    let world = World::new();
    world
        .board
        .add_ticket(task(2, "Claimed one", TicketState::Ready));
    let programmer = instance("programmer", 7);
    world
        .board
        .claim(TicketId::from(2), &programmer)
        .expect("claims");
    let holder = Holder {
        pid: 4242,
        start: "1000".to_owned(),
    };
    let folder = runs::reserve(
        world.dir.path(),
        &PlannedLaunch {
            instance: programmer,
        },
        &holder,
        "2026-10-28T01:00:00Z",
    )
    .expect("reserves");
    runs::mark_started(&folder, "2026-10-28T01:00:10Z").expect("starts");
    let started = humantime::parse_rfc3339("2026-10-28T01:00:10Z").expect("time");

    let state = world.gather(started + Duration::from_secs(95));

    assert_eq!(state.instances.len(), 1);
    let running = &state.instances[0];
    assert_eq!(running.instance, "programmer instance 7 on #100");
    assert_eq!(running.role, "programmer");
    assert_eq!(running.task, Some(2));
    assert_eq!(running.phase.as_deref(), Some("claim"));
    assert_eq!(running.run_seconds, Some(95));
    assert_eq!(running.last_tool_call, None);

    let dead = gather_state(&StateInputs {
        alive: &|_| false,
        ..world.inputs(started)
    });
    assert!(dead.instances.is_empty(), "a dead process is not running");
}

#[test]
fn unreachable_board_reason() {
    const MANIFEST: &str = "[project]\nname = \"example\"\n\n[board]\nkind = \"forgejo-issues\"\n\
        repo = \"example/tracker\"\nurl = \"http://forgejo.invalid\"\n\
        token_env = \"ZAF_TEST_HUB_STATE_TOKEN\"\n";
    assert!(
        std::env::var_os("ZAF_TEST_HUB_STATE_TOKEN").is_none(),
        "the test's variable is never set"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".zaf.toml"), MANIFEST).expect("manifest");

    let state = gather_project(dir.path(), at(1_000));

    let reason = state.reason.expect("a reason");
    assert!(reason.contains("ZAF_TEST_HUB_STATE_TOKEN"), "{reason}");
    assert!(state.epics.is_empty() && state.queue.is_empty());
    assert_eq!(state.project, "example");
    assert_eq!(state.root, dir.path());

    // A project with no manifest is a project with a reason too.
    let bare = tempfile::tempdir().expect("tempdir");
    let state = gather_project(bare.path(), at(1_000));
    assert!(state.reason.expect("a reason").contains("manifest"));
}

#[test]
fn hub_reports_one_problem_for_all_stale_profiles() {
    const MANIFEST: &str = "[project]\nname = \"example\"\n\n[board]\nkind = \"forgejo-issues\"\n\
        repo = \"example/tracker\"\nurl = \"http://forgejo.invalid\"\n\
        token_env = \"ZAF_TEST_HUB_STATE_TOKEN\"\n\n\
        [runners.claude]\ncommand = \"true\"\n\n\
        [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
        [roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\nprofile = \"lean\"\nturns = 5\nbatch = 2\n";
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".zaf.toml"), MANIFEST).expect("manifest");
    // Both profiles have a home and no stamp: both are stale.
    for name in ["build", "lean"] {
        std::fs::create_dir_all(dir.path().join(".zaf/profiles").join(name)).expect("home");
    }

    let state = gather_project(dir.path(), at(1_000));

    let profiles: Vec<&Problem> = state
        .problems
        .iter()
        .filter(|problem| problem.kind == ProblemKind::Profile)
        .collect();
    assert_eq!(profiles.len(), 1, "{:?}", state.problems);
    let problem = profiles[0];
    assert_eq!(
        problem.detail,
        "profiles lean and build have no stamp: run zaf profile init"
    );
    assert_eq!(
        problem.fix.as_deref(),
        Some(format!("cd {} && zaf profile init", dir.path().display()).as_str())
    );
}

#[test]
fn the_value_serializes_with_its_documented_names() {
    let world = World::new();
    world
        .board
        .add_ticket(task(2, "Claimed one", TicketState::Ready));
    world
        .board
        .claim(TicketId::from(2), &instance("programmer", 7))
        .expect("claims");

    let state = world.gather(at(1_000));
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&state).expect("serializes")).expect("json");

    let names = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    for key in [
        "project",
        "root",
        "gathered_at",
        "reason",
        "epics",
        "queue",
        "instances",
        "watch",
        "last_tick",
    ] {
        assert!(json.get(key).is_some(), "{key} in {:?}", names(&json));
    }
    assert_eq!(json["gathered_at"], "1970-01-01T00:16:40Z");
    assert!(json["reason"].is_null());
    // No scheduler runs and no tick has run: the watch reads as down, with nothing else to say.
    assert_eq!(
        json["watch"],
        serde_json::json!({
            "running": false, "pid": null, "since": null, "zaf": null, "version": null,
            "last_tick": null,
        })
    );
    assert!(json["last_tick"].is_null());
    let epic = &json["epics"][0];
    for key in [
        "number", "title", "stage", "done", "total", "dollars", "groups",
    ] {
        assert!(epic.get(key).is_some(), "{key} in {:?}", names(epic));
    }
    assert_eq!(epic["stage"], "challenged");
    let queued = &json["queue"][0];
    for key in [
        "number",
        "title",
        "epic",
        "state",
        "group",
        "instance",
        "phase",
        "issue_url",
        "pull_request",
        "pull_request_url",
    ] {
        assert!(queued.get(key).is_some(), "{key} in {:?}", names(queued));
    }
    assert_eq!(queued["state"], "claimed");

    let unreachable = ProjectState {
        reason: Some("the board cannot be reached: no token".to_owned()),
        ..state.clone()
    };
    let json = serde_json::to_value(&unreachable).expect("serializes");
    assert_eq!(json["reason"], "the board cannot be reached: no token");
    let instance_keys = [
        "instance",
        "role",
        "task",
        "number",
        "title",
        "phase",
        "run_seconds",
        "last_tool_call",
    ];
    let sample = serde_json::to_value(InstanceState {
        instance: "team instance 1 on #1".to_owned(),
        role: "team".to_owned(),
        task: None,
        number: None,
        title: None,
        phase: None,
        run_seconds: None,
        last_tool_call: None,
    })
    .expect("serializes");
    for key in instance_keys {
        assert!(sample.get(key).is_some(), "{key}");
    }
}

#[test]
fn a_queue_task_shows_its_place_not_its_label() {
    use crate::app::actions::ActionName;
    use crate::domain::schedule::{Place, WaitingOn};

    let world = World::new();
    world
        .board
        .add_ticket(task(1, "Blocked by hand", TicketState::Blocked));
    world
        .board
        .add_ticket(task(2, "Ready by hand", TicketState::Ready));
    world
        .board
        .add_ticket(task(3, "Merged, not yet labelled", TicketState::Claimed));
    world
        .board
        .add_ticket(task(4, "Not placed", TicketState::Question));
    let lane = Place::InLane {
        next: ActionName::from("implement"),
        since: at(10),
    };
    let places: Places = [
        (TicketId::from(1), lane),
        (
            TicketId::from(2),
            Place::Waiting([WaitingOn::Ticket(TicketId::from(1))].into()),
        ),
        (TicketId::from(3), Place::Done),
    ]
    .into();

    let state = gather_state(&StateInputs {
        places: Some(&places),
        ..world.inputs(at(1_000))
    });

    let queue: Vec<(u64, TicketState)> = state
        .queue
        .iter()
        .map(|task| (task.number, task.state))
        .collect();
    // #1 is in the lane, #2 waits on #1, #3 is done by its merge, and #4 keeps its label's word.
    assert_eq!(
        queue,
        [
            (1, TicketState::Ready),
            (2, TicketState::Blocked),
            (4, TicketState::Question),
        ]
    );
}

#[test]
fn the_hub_shows_the_scheduler_from_its_lock() {
    let world = World::new();
    let me = Holder {
        pid: 4242,
        start: "123456".to_owned(),
    };
    let (lock, _) = crate::app::schedule::lock::SchedulerLock::take(
        world.dir.path(),
        &me,
        "0.3.1",
        "0.4.0",
        at(1_000),
    )
    .expect("the scheduler's lock");

    // `alive` says every process lives: the lock names a running scheduler.
    let state = world.gather(at(2_000));

    assert_eq!(
        state.watch,
        Some(WatchState {
            running: true,
            pid: Some(4242),
            since: Some("1970-01-01T00:16:40Z".to_owned()),
            zaf: Some("0.3.1".to_owned()),
            version: Some("0.4.0".to_owned()),
            last_tick: None,
        })
    );

    // Stopped, it clears its line, and the hub shows it down.
    lock.release();
    let state = world.gather(at(3_000));
    assert_eq!(state.watch.map(|watch| watch.running), Some(false));
}

// The machine state: every project on a zaf home's list, read apart under one bound.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use super::machine::{Reader, gather_machine_with};
use crate::app::projects::Projects;
use crate::domain::{Budget, Units};

/// A project folder with a manifest, on the list of `home`.
fn listed(parent: &Path, home: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    std::fs::create_dir_all(&root).expect("project folder");
    std::fs::write(root.join(".zaf.toml"), "").expect("manifest");
    let root = std::fs::canonicalize(root).expect("canonical");
    Projects::add(home, &root).expect("add");
    root
}

/// A project read that found `running` instances.
fn read_with(root: &Path, now: SystemTime, running: usize) -> ProjectState {
    let name = root.file_name().expect("name").to_string_lossy();
    let mut state = unreadable(root, now, String::new());
    state.reason = None;
    state.instances = (0..running)
        .map(|number| InstanceState {
            instance: format!("programmer instance {number} on #{EPIC}"),
            role: "programmer".to_owned(),
            task: None,
            number: None,
            title: None,
            phase: None,
            run_seconds: None,
            last_tool_call: None,
        })
        .collect();
    state.epics = vec![EpicState {
        number: EPIC,
        title: format!("The epic of {name}"),
        stage: EpicStage::Challenged,
        url: None,
        done: 0,
        total: 1,
        dollars: 0.0,
        groups: Vec::new(),
        line_state: LineState::NotStarted,
        line_question: None,
        tasks: Vec::new(),
    }];
    state
}

fn machine(home: &Path, bound: Duration, reader: Arc<Reader>) -> MachineState {
    gather_machine_with(
        home,
        at(1_000),
        bound,
        Units::default(),
        Budget::default(),
        reader,
    )
}

#[test]
fn machine_holds_every_project() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let alpha = listed(dir.path(), &home, "alpha");
    let beta = listed(dir.path(), &home, "beta");

    let state = machine(
        &home,
        Duration::from_secs(10),
        Arc::new(|root, now| read_with(root, now, 0)),
    );

    let roots: Vec<_> = state.projects.iter().map(|project| &project.root).collect();
    assert_eq!(roots, [&alpha, &beta], "in the order they joined");
    assert!(
        state
            .projects
            .iter()
            .all(|project| !project.missing && project.reason.is_none())
    );
    assert_eq!(state.projects[0].epics[0].title, "The epic of alpha");
    assert_eq!(state.projects[1].epics[0].title, "The epic of beta");
    assert_eq!(state.machine.projects, 2);
}

#[test]
fn missing_project_kept() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let fine = listed(dir.path(), &home, "fine");
    let gone = listed(dir.path(), &home, "gone");
    std::fs::remove_dir_all(&gone).expect("remove the folder");
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);

    let state = machine(
        &home,
        Duration::from_secs(10),
        Arc::new(move |root, now| {
            counted.fetch_add(1, Ordering::SeqCst);
            read_with(root, now, 0)
        }),
    );

    assert_eq!(state.projects.len(), 2);
    assert_eq!(state.projects[0].root, fine);
    assert!(!state.projects[0].missing);
    let second = &state.projects[1];
    assert!(second.missing);
    assert_eq!(second.root, gone);
    assert_eq!(second.project, "gone");
    assert_eq!(second.reason.as_deref(), Some("its folder is gone"));
    assert!(second.epics.is_empty() && second.queue.is_empty());
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "no read for the missing one"
    );
    let json = serde_json::to_value(second).expect("serializes");
    assert_eq!(json["missing"], true);
}

#[test]
fn slow_board_does_not_hold_others() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let fine = listed(dir.path(), &home, "fine");
    let slow = listed(dir.path(), &home, "slow");
    // The slow read waits on a channel nobody feeds until the test ends.
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let slow_root = slow.clone();

    let started = std::time::Instant::now();
    let state = machine(
        &home,
        Duration::from_millis(300),
        Arc::new(move |root, now| {
            if root == slow_root {
                let _ = held.lock().expect("held").recv();
            }
            read_with(root, now, 1)
        }),
    );
    let took = started.elapsed();
    drop(release);

    assert!(took < Duration::from_secs(5), "the bound held: {took:?}");
    let [first, second] = &state.projects[..] else {
        panic!("both projects: {:?}", state.projects);
    };
    assert_eq!(first.root, fine);
    assert_eq!(first.reason, None);
    assert_eq!(first.epics.len(), 1, "the other project is complete");
    assert_eq!(first.instances.len(), 1);
    assert_eq!(second.root, slow);
    assert!(!second.missing);
    let reason = second.reason.as_deref().expect("its reason");
    assert!(reason.contains("did not answer within"), "{reason}");
    assert!(second.epics.is_empty());
}

#[test]
fn machine_line_sums() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    listed(dir.path(), &home, "alpha");
    listed(dir.path(), &home, "beta");
    let gone = listed(dir.path(), &home, "gone");
    std::fs::remove_dir_all(gone).expect("remove the folder");

    let state = gather_machine_with(
        &home,
        at(1_000),
        Duration::from_secs(10),
        Units::whole(2),
        Budget {
            units: Some(24),
            ..Budget::default()
        },
        Arc::new(|root, now| {
            let running = if root.ends_with("alpha") { 2 } else { 1 };
            read_with(root, now, running)
        }),
    );

    assert_eq!(state.machine.instances, 3, "the running instances of both");
    assert_eq!(state.machine.projects, 3, "the missing one is listed");
    assert_eq!(state.machine.in_use.as_f64(), 2.0);
    assert_eq!(state.machine.budget.units, Some(24));
    let json = serde_json::to_value(&state).expect("serializes");
    assert_eq!(json["machine"]["instances"], 3);
    assert_eq!(json["machine"]["in_use_units"], 2.0);
    assert_eq!(json["machine"]["budget"]["units"], 24);
    assert_eq!(json["projects"].as_array().map(Vec::len), Some(3));
}

#[test]
fn a_read_that_hangs_holds_one_thread() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    listed(dir.path(), &home, "stuck");
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let reader: Arc<Reader> = Arc::new(move |root, now| {
        counted.fetch_add(1, Ordering::SeqCst);
        let _ = held.lock().expect("held").recv();
        read_with(root, now, 0)
    });

    let bound = Duration::from_millis(100);
    let first = machine(&home, bound, Arc::clone(&reader));
    let reason = first.projects[0].reason.as_deref().expect("its reason");
    assert!(reason.contains("did not answer within"), "{reason}");
    // The first read is still on its thread: the next refreshes start no other.
    for _ in 0..3 {
        let again = machine(&home, bound, Arc::clone(&reader));
        let reason = again.projects[0].reason.as_deref().expect("its reason");
        assert!(reason.contains("has not ended"), "{reason}");
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);

    // Once it ends, the project is read again.
    drop(release);
    let mut read = None;
    for _ in 0..100 {
        let state = machine(&home, Duration::from_secs(10), Arc::clone(&reader));
        if state.projects[0].reason.is_none() {
            read = Some(state);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(read.is_some(), "read again after the hung read ended");
}
