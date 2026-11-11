use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::app::spawn::outcome::{Launched, Ran, Started};
use crate::domain::{TicketState, Units};
use crate::fakes::{FakeBoard, FakeForge, PrRequest};
use crate::ports::{BaseName, Check, CheckName, CheckState, Holder, PrNumber, SliceName};

const MANIFEST: &str = r#"
[project]
name = "bookshelf"

[board]
kind = "github-issues"
repo = "example/bookshelf-board"

[spawn]
slots = 2

[phases]
Task = ["implement", "ci", "merge"]
"#;

const EPIC: u64 = 2;

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

/// A task body that follows the template, of size `size`.
fn task_body(size: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
         Model: sonnet\nSize: {size}\nBlocked by: —\n"
    )
}

/// A project on disk with the phase actions implement (a model ending at an open pull request),
/// ci (a wait ending when CI is done) and merge (zaf, ending at the merge), its board and forge.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state_dir: PathBuf,
    board: FakeBoard,
    forge: FakeForge,
    manifest: Manifest,
}

impl Scene {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path())
            .expect("real path")
            .join("project");
        let home = root.parent().expect("parent").join("home");
        fs::create_dir_all(&home).expect("home");
        let state_dir = root.join(".zaf");
        let board = FakeBoard::default();
        board.add_challenged_epic(id(EPIC), "Issue 2");
        let scene = Self {
            _dir: dir,
            root,
            home,
            state_dir,
            board,
            forge: FakeForge::default(),
            manifest: MANIFEST.parse().expect("manifest parses"),
        };
        scene.action("implement", "model", "pr-open");
        scene.action("ci", "wait", "ci-done");
        scene.action("merge", "zaf", "merged");
        scene
    }

    fn action(&self, action: &str, kind: &str, ends: &str) {
        let file = self.root.join(".zaf/actions").join(action);
        write(
            &file,
            &format!(
                "#!/bin/sh\n# zaf description: {action}\n# zaf 1: failed\n\
                 # zaf kind: {kind}\n# zaf ends: {ends}\nexit 0\n"
            ),
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    /// A ready task of size `size` under the epic.
    fn task(&self, number: u64, size: &str) {
        self.board.add_ticket(Ticket {
            id: id(number),
            epic: Some(id(EPIC)),
            title: format!("Issue {number}").into(),
            body: task_body(size).into(),
            state: Some(TicketState::Ready),
            url: "".into(),
            kind: None,
        });
    }

    /// `task`'s pull request, opened at 20 at head `h1` and recorded, its CI `state`.
    fn open_pr(&self, task: u64, state: Option<CheckState>) -> PrNumber {
        let pr = self
            .forge
            .open_pr(&PrRequest {
                slice: SliceName::from("slice"),
                base: BaseName::from("base"),
                title: "Title".to_owned(),
                body: String::new(),
            })
            .expect("open pr");
        self.forge.set_head(pr, Some("h1")).expect("head");
        self.forge.set_opened_at(pr, time(20)).expect("opened");
        if let Some(state) = state {
            let finished_at = (state != CheckState::Pending).then(|| time(30));
            self.forge
                .set_checks(
                    pr,
                    vec![Check {
                        name: CheckName::from("ci"),
                        state,
                        finished_at,
                    }],
                )
                .expect("checks");
        }
        self.record_pr(task, &pr.get().to_string());
        pr
    }

    fn record_pr(&self, task: u64, text: &str) {
        write(&self.root.join(format!(".zaf/slices/{task}.pr")), text);
    }

    fn gather(&self) -> Gathered {
        let ports = Ports {
            board: &self.board,
            forge: &self.forge,
            manifest: &self.manifest,
            root: &self.root,
            home: &self.home,
            state_dir: &self.state_dir,
            alive: &|_: &Holder| false,
        };
        gather(&ports, time(1)).expect("the board reads")
    }
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

fn in_use() -> InUse {
    InUse {
        now: time(100),
        units: Units::default(),
        e_live: false,
        running_here: 0,
        slots: 2,
    }
}

/// What the steps were asked to do, in order.
#[derive(Debug, Default)]
struct Calls {
    run: Vec<(u64, String)>,
    launch: Vec<(u64, String)>,
    start: Vec<PlannedPhase>,
}

/// Runs the steps on `tasks`, each act recorded and `start` answering as `start_result` says.
fn run(
    board: &dyn Board,
    tasks: &[PhaseTask],
    start_result: Result<(), String>,
) -> (PhaseTick, Calls) {
    let mut calls = Calls::default();
    let mut runs = Vec::new();
    let mut launches = Vec::new();
    let mut starts = Vec::new();
    let mut run = |task: TicketId, action: &ActionName| {
        runs.push((task.get(), action.as_str().to_owned()));
        0
    };
    let mut launch = |task: TicketId, action: &ActionName| {
        launches.push((task.get(), action.as_str().to_owned()));
        Ok(())
    };
    let mut start = |planned: &PlannedPhase| {
        starts.push(planned.clone());
        start_result.clone()
    };
    let mut log = |_: &str| {};
    let tick = run_steps(
        board,
        tasks,
        &in_use(),
        &Budget::default(),
        &mut Steps {
            run: &mut run,
            launch: &mut launch,
            start: &mut start,
            log: &mut log,
        },
    );
    calls.run = runs;
    calls.launch = launches;
    calls.start = starts;
    (tick, calls)
}

#[test]
fn the_tick_derives_and_merges() {
    let scene = Scene::new();
    scene.task(3, "S");
    // Its pull request is open and CI passed: merge, a zaf action, is ready.
    scene.open_pr(3, Some(CheckState::Passed));
    let gathered = scene.gather();
    assert!(gathered.unreadable.is_empty(), "{gathered:?}");
    let (tick, calls) = run(&scene.board, &gathered.tasks, Ok(()));
    assert_eq!(calls.run, [(3, "merge".to_owned())]);
    assert_eq!(
        tick.outcome.ran,
        [Ran {
            task: id(3),
            action: name("merge"),
            exit: 0,
        }]
    );
    assert!(
        calls.launch.is_empty() && calls.start.is_empty(),
        "{calls:?}"
    );
    assert!(tick.failed.is_empty(), "{tick:?}");
}

#[test]
fn the_tick_launches_a_ready_wait() {
    let scene = Scene::new();
    scene.task(3, "S");
    // The pull request is open and CI has not finished: the ci wait is ready.
    scene.open_pr(3, Some(CheckState::Pending));
    let (tick, calls) = run(&scene.board, &scene.gather().tasks, Ok(()));
    assert_eq!(calls.launch, [(3, "ci".to_owned())]);
    assert_eq!(
        tick.outcome.waits,
        [Launched {
            task: id(3),
            action: name("ci"),
        }]
    );
    assert!(
        tick.outcome.started.is_none(),
        "a wait is not the model start"
    );
    assert!(calls.run.is_empty() && calls.start.is_empty(), "{calls:?}");
}

#[test]
fn the_tick_starts_one_model_phase() {
    let scene = Scene::new();
    // Both are ready at implement since the same time: the lower task goes first (#497).
    scene.task(5, "S");
    scene.task(3, "S");
    let gathered = scene.gather();
    let tasks: Vec<u64> = gathered
        .tasks
        .iter()
        .map(|task| task.tick.task.get())
        .collect();
    assert_eq!(tasks, [3, 5], "in task order");
    let (tick, calls) = run(&scene.board, &gathered.tasks, Ok(()));
    assert_eq!(
        calls.start,
        [PlannedPhase {
            task: id(3),
            action: name("implement"),
            units: 2,
        }]
    );
    assert_eq!(
        tick.outcome.started,
        Some(Started {
            task: id(3),
            action: name("implement"),
            units: 2,
        })
    );
}

#[test]
fn a_failed_start_records_nothing_and_tries_no_other() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    let (tick, calls) = run(
        &scene.board,
        &scene.gather().tasks,
        Err("exit 2".to_owned()),
    );
    assert_eq!(calls.start.len(), 1, "no other item is tried: {calls:?}");
    assert!(tick.outcome.started.is_none());
    assert_eq!(tick.failed, ["exit 2"]);
}

#[test]
fn an_unreadable_task_is_skipped() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    // #3 records a pull request the forge does not know.
    scene.record_pr(3, "99");
    let gathered = scene.gather();
    assert_eq!(gathered.unreadable.len(), 1, "{gathered:?}");
    assert_eq!(gathered.unreadable[0].task, id(3));
    assert!(
        gathered.unreadable[0].reason.contains("pull request #99"),
        "{gathered:?}"
    );
    let (tick, _) = run(&scene.board, &gathered.tasks, Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5)),
        "the others still go"
    );
}

#[test]
fn a_task_of_a_type_with_no_list_is_left_to_the_team() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(4, "S");
    scene.board.set_type(id(4), "Bug");
    let gathered = scene.gather();
    let tasks: Vec<u64> = gathered
        .tasks
        .iter()
        .map(|task| task.tick.task.get())
        .collect();
    assert_eq!(tasks, [3]);
    assert!(gathered.unreadable.is_empty());
    let ticket = scene.board.task(id(4)).expect("#4");
    assert!(!phased(&scene.manifest, &ticket));
    assert!(phased(
        &scene.manifest,
        &scene.board.task(id(3)).expect("#3")
    ));
}

/// Every file under `dir`, with its contents and modified time.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .expect("modified");
                found.push((path.clone(), fs::read(&path).expect("read"), modified));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_tick_stores_no_standing() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(4, "S");
    scene.task(5, "S");
    scene.open_pr(3, Some(CheckState::Passed));
    scene.open_pr(4, Some(CheckState::Pending));
    let before = files(&scene.root);
    let home_before = files(&scene.home);
    let gathered = scene.gather();
    let (tick, _) = run(&scene.board, &gathered.tasks, Ok(()));
    assert!(!tick.outcome.is_idle(), "the steps acted: {tick:?}");
    assert_eq!(
        files(&scene.root),
        before,
        "nothing about a standing is written"
    );
    assert_eq!(files(&scene.home), home_before);
    // Read again, the same standing comes back: it is derived, not kept.
    assert_eq!(scene.gather(), gathered);
}
