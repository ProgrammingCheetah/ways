use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::app::schedule::gather::gather_listing as gather;
use crate::app::spawn::outcome::{Launched, Ran, Started};
use crate::app::spawn::phase::{self, PhaseError, Ports};
use crate::domain::{Instance, RoleName, Ticket, TicketState};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
use crate::manifest::Manifest;
use crate::ports::{
    BaseName, Check, CheckName, CheckState, Holder, NewQuestion, PrNumber, SliceName,
};

/// The pack's list: implement (a model ending at an open pull request), ci (a wait ending when
/// CI is done) and merge (zaf, ending at the merge).
const LIST: &str = r#"["implement", "ci", "merge"]"#;

const EPIC: u64 = 2;

/// The process id every live run in these tests records; any other reads as dead.
const LIVE: u32 = 4242;

fn manifest(list: &str) -> Manifest {
    format!(
        "[project]\nname = \"bookshelf\"\n\n[board]\nkind = \"github-issues\"\n\
         repo = \"example/bookshelf-board\"\n\n[spawn]\nslots = 2\n\n[phases]\nTask = {list}\n"
    )
    .parse()
    .expect("manifest parses")
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// The time every tick here runs at.
fn now() -> SystemTime {
    time(100)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

/// Whether `holder` is alive: only [`LIVE`] is.
fn live(holder: &Holder) -> bool {
    holder.pid == LIVE
}

/// A task body that follows the template, of size `size`, blocked by `blocked_by`.
fn task_body(size: &str, blocked_by: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
         Model: sonnet\nSize: {size}\nBlocked by: {blocked_by}\n"
    )
}

/// A task of size S under `epic`, labelled `state`, blocked by `blocked_by`.
fn ticket(number: u64, epic: Option<u64>, state: TicketState, blocked_by: &str) -> Ticket {
    Ticket {
        id: id(number),
        epic: epic.map(id),
        title: format!("Issue {number}").into(),
        body: task_body("S", blocked_by).into(),
        state: Some(state),
        url: "".into(),
        kind: None,
    }
}

/// A project on disk with the phase actions of its list, its board with a challenged epic #2 and
/// its forge.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state_dir: PathBuf,
    board: FakeBoard,
    forge: FakeForge,
    vcs: FakeVcs,
    manifest: Manifest,
}

impl Scene {
    /// The pack's list ([`LIST`]).
    fn new() -> Self {
        let scene = Self::with_list(LIST);
        scene.action("implement", "model", "pr-open");
        scene.action("ci", "wait", "ci-done");
        scene.action("merge", "zaf", "merged");
        scene
    }

    /// A scene whose `Task` list is `list`, with no action written yet.
    fn with_list(list: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path())
            .expect("real path")
            .join("project");
        let home = root.parent().expect("parent").join("home");
        fs::create_dir_all(&home).expect("home");
        let state_dir = root.join(".zaf");
        let board = FakeBoard::default();
        board.add_challenged_epic(id(EPIC), "Issue 2");
        Self {
            _dir: dir,
            root,
            home,
            state_dir,
            board,
            forge: FakeForge::default(),
            vcs: FakeVcs::default(),
            manifest: manifest(list),
        }
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
            body: task_body(size, "—").into(),
            ..ticket(number, Some(EPIC), TicketState::Ready, "—")
        });
    }

    /// A ready task of size S under `epic`.
    fn task_under(&self, epic: u64, number: u64) {
        self.board
            .add_ticket(ticket(number, Some(epic), TicketState::Ready, "—"));
    }

    /// Makes `ci` fail to `fix`, a model action ending at its exit.
    fn fails_to_fix(&self) {
        let ci = self.root.join(".zaf/actions/ci");
        write(
            &ci,
            "#!/bin/sh\n# zaf description: ci\n# zaf 1: failed\n# zaf kind: wait\n\
             # zaf ends: ci-done\n# zaf on-fail: fix\nexit 0\n",
        );
        fs::set_permissions(&ci, fs::Permissions::from_mode(0o755)).expect("chmod");
        self.action("fix", "model", "exit");
    }

    /// `task`'s pull request, opened at 20 at head `h1` changing a file, and recorded, its CI
    /// `state`.
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
        self.forge
            .set_changed_files(pr, &["src/lib.rs"])
            .expect("files");
        if let Some(state) = state {
            self.checks(pr, state);
        }
        self.record_pr(task, &pr.get().to_string());
        pr
    }

    /// Sets `pr`'s one check, `ci`, to `state`; a finished one finished at 30.
    fn checks(&self, pr: PrNumber, state: CheckState) {
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

    fn record_pr(&self, task: u64, text: &str) {
        write(&self.root.join(format!(".zaf/slices/{task}.pr")), text);
    }

    fn ports(&self) -> Ports<'_> {
        Ports {
            board: &self.board,
            forge: &self.forge,
            ci_filter: None,
            manifest: &self.manifest,
            root: &self.root,
            home: &self.home,
            state_dir: &self.state_dir,
            alive: &live,
            version: None,
            now: SystemTime::now(),
            vcs: Some(&self.vcs),
        }
    }

    fn gather(&self) -> Gathered {
        gather(&self.ports()).expect("the board reads")
    }

    /// `zaf spawn phase <task> <action>`, with a launcher that must never be reached here: a
    /// start that gets that far fails with `launched`.
    fn spawn_phase(&self, task: u64, action: &str) -> Result<phase::Started, PhaseError> {
        phase::start(&self.ports(), id(task), action, &|_: &phase::Started| {
            Err("launched".to_owned())
        })
    }

    /// The tick's step 6 start as the binary makes it, `zaf spawn phase` on the planned phase,
    /// through its own code: a launcher that starts nothing and hands back a live process, so the
    /// run folder is written as a started run's.
    fn start_phase(&self, planned: &PlannedModel) -> Result<(), String> {
        let PlannedModel::Phase { task, action, .. } = planned else {
            return Err("an epic launch is not started by zaf spawn phase".to_owned());
        };
        phase::start(
            &self.ports(),
            *task,
            action.as_str(),
            &|_: &phase::Started| {
                Ok(Holder {
                    pid: LIVE,
                    start: "1000".to_owned(),
                })
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// The phase run folders on disk, by name.
    fn run_folders(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.state_dir.join("phases")) else {
            return Vec::new();
        };
        let mut folders: Vec<PathBuf> = entries.map(|entry| entry.expect("entry").path()).collect();
        folders.sort();
        folders
    }
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

/// What the steps were asked to do, in order.
#[derive(Debug, Default)]
struct Calls {
    run: Vec<(u64, String)>,
    launch: Vec<(u64, String)>,
    start: Vec<PlannedModel>,
}

impl Calls {
    fn is_empty(&self) -> bool {
        self.run.is_empty() && self.launch.is_empty() && self.start.is_empty()
    }
}

/// Runs the tick's steps on `gathered` with two slots, each act recorded and `start` answering
/// as `start_result` says.
fn run(
    board: &dyn Board,
    gathered: &Gathered,
    start_result: Result<(), String>,
) -> (PhaseTick, Calls) {
    run_with(board, gathered, 2, start_result)
}

/// [`run`] with `slots` slots.
fn run_with(
    board: &dyn Board,
    gathered: &Gathered,
    slots: usize,
    start_result: Result<(), String>,
) -> (PhaseTick, Calls) {
    run_on(
        board,
        gathered,
        &idle(slots),
        &Budget::default(),
        &mut |_: &PlannedModel| start_result.clone(),
    )
}

/// An idle machine whose project has `slots` slots and runs no planning or challenger launch.
fn idle(slots: usize) -> Machine {
    Machine {
        units: Units::default(),
        e_live: false,
        launches_here: 0,
        slots,
        merge_per_tick: 1,
    }
}

/// Runs the tick's steps on `gathered` against `machine` within `budget`, each act recorded and
/// the planned model handed to `start`.
fn run_on(
    board: &dyn Board,
    gathered: &Gathered,
    machine: &Machine,
    budget: &Budget,
    start: &mut dyn FnMut(&PlannedModel) -> Result<(), String>,
) -> (PhaseTick, Calls) {
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
    let mut start = |planned: &PlannedModel| {
        starts.push(planned.clone());
        start(planned)
    };
    let mut log = |_: &str| {};
    // No other tick meets this one: its questions are raised under a lock of its own.
    let state_dir = tempfile::tempdir().expect("tempdir");
    let tick = tick(
        board,
        state_dir.path(),
        gathered,
        now(),
        machine,
        budget,
        &mut Steps {
            run: &mut run,
            launch: &mut launch,
            start: &mut start,
            pr_of: &mut |task: TicketId| task.get().to_string().parse().ok(),
            merged: &mut |_: crate::ports::PrNumber| true,
            branch_head: &mut || None,
            group: &mut |_| {},
            log: &mut log,
        },
    );
    let calls = Calls {
        run: runs,
        launch: launches,
        start: starts,
    };
    (tick, calls)
}

/// The lines the tick's block names the held tickets by.
fn lines(held: &[Held]) -> Vec<String> {
    held.iter().map(Held::line).collect()
}

/// The ticket numbers gathered.
fn numbers(gathered: &Gathered) -> Vec<u64> {
    gathered
        .tickets
        .iter()
        .map(|facts| facts.ticket.get())
        .collect()
}

/// Where `task` sits in `gathered` at [`now`].
fn place_of(gathered: &Gathered, task: u64) -> Place {
    let facts = gathered
        .tickets
        .iter()
        .find(|facts| facts.ticket == id(task))
        .expect("the task is gathered");
    place(facts, now())
}

fn refused(result: Result<phase::Started, PhaseError>) -> String {
    match result {
        Err(PhaseError::Refused(words)) => words,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn the_tick_derives_and_merges() {
    let scene = Scene::new();
    scene.task(3, "S");
    // Its pull request is open and CI passed: merge, a zaf action, is next.
    scene.open_pr(3, Some(CheckState::Passed));
    let gathered = scene.gather();
    assert!(gathered.unreadable.is_empty(), "{gathered:?}");
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
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
    // The pull request is open and CI has not finished.
    scene.open_pr(3, Some(CheckState::Pending));
    // A phase ending at `ci-done` is never next, and nothing is while a check at the head has not
    // ended: the ci wait is not launched.
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.launch.is_empty(), "{calls:?}");
    assert!(tick.outcome.waits.is_empty(), "{tick:?}");
    // A wait that ends at its own exit is in the lane, and launched.
    scene.action("ci", "wait", "exit");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
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
    // Both are in the lane at implement since the same time: the lower task goes first (#497).
    scene.task(5, "S");
    scene.task(3, "S");
    let gathered = scene.gather();
    assert_eq!(numbers(&gathered), [3, 5], "in task order");
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert_eq!(
        calls.start,
        [PlannedModel::Phase {
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
    let (tick, calls) = run(&scene.board, &scene.gather(), Err("exit 2".to_owned()));
    assert_eq!(calls.start.len(), 1, "no other entry is tried: {calls:?}");
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
    assert_eq!(gathered.unreadable[0].ticket, id(3));
    assert!(
        gathered.unreadable[0].reason.contains("pull request #99"),
        "{gathered:?}"
    );
    let (tick, _) = run(&scene.board, &gathered, Ok(()));
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
    assert_eq!(numbers(&gathered), [3]);
    assert!(gathered.unreadable.is_empty());
    let (tick, _) = run(&scene.board, &gathered, Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(3))
    );
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
fn the_tick_stores_no_placement() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(4, "S");
    scene.task(5, "S");
    scene.open_pr(3, Some(CheckState::Passed));
    scene.open_pr(4, Some(CheckState::Pending));
    let before = files(&scene.root);
    let home_before = files(&scene.home);
    let gathered = scene.gather();
    let (tick, _) = run(&scene.board, &gathered, Ok(()));
    assert!(!tick.outcome.is_idle(), "the steps acted: {tick:?}");
    assert_eq!(
        files(&scene.root),
        before,
        "nothing about a placement is written"
    );
    assert_eq!(files(&scene.home), home_before);
    // Read again, the same facts come back: the placement is derived, not kept.
    assert_eq!(scene.gather(), gathered);
}

impl Scene {
    /// One finished run of `action` on `task` with no pull request: started at `started`, its
    /// process exited `exit` and printed `output`. A run with no exit is a dead process that
    /// recorded no code.
    fn phase_run(&self, task: u64, action: &str, started: u64, exit: Option<i32>, output: &str) {
        self.phase_run_at(task, action, started, exit, output, "");
    }

    /// One finished run of `action` on `task`, started at `head` (empty for no pull request) and,
    /// once its process exited `exit`, leaving the head where it was: its launcher records the head
    /// it left before the exit code. A run with no exit recorded neither.
    fn phase_run_at(
        &self,
        task: u64,
        action: &str,
        started: u64,
        exit: Option<i32>,
        output: &str,
        head: &str,
    ) {
        let dir = self.run_dir(task, action, started);
        write(&dir.join("action"), action);
        write(&dir.join("head"), head);
        write(&dir.join("output"), output);
        if let Some(code) = exit {
            write(&dir.join("head-end"), head);
            write(&dir.join("exit"), &code.to_string());
        }
        self.started_at(task, action, started);
    }

    /// A run of `action` on `task` started at `started` at `head`, whose process `pid` has not
    /// recorded an exit: live when `pid` is [`LIVE`], dead otherwise.
    fn process_run(&self, task: u64, action: &str, started: u64, head: &str, pid: u32) {
        let dir = self.run_dir(task, action, started);
        write(&dir.join("action"), action);
        write(&dir.join("head"), head);
        write(&dir.join("pid"), &format!("{pid} 1000"));
        self.started_at(task, action, started);
    }

    fn run_dir(&self, task: u64, action: &str, started: u64) -> PathBuf {
        self.state_dir
            .join(format!("phases/{action}-{started}-task-{task}"))
    }

    /// Writes the run's `task` file last: its time is when the run started.
    fn started_at(&self, task: u64, action: &str, started: u64) {
        let file = self.run_dir(task, action, started).join("task");
        write(&file, &task.to_string());
        fs::File::options()
            .write(true)
            .open(&file)
            .expect("opens")
            .set_modified(time(started))
            .expect("sets the start");
    }

    /// Records that the run of `action` on `task` started at `started` left `head`.
    fn moved(&self, task: u64, action: &str, started: u64, head: &str) {
        write(&self.run_dir(task, action, started).join("head-end"), head);
    }

    /// The questions on the board as (task, text).
    fn asked(&self) -> Vec<(Option<TicketId>, String)> {
        self.board
            .questions()
            .expect("lists")
            .iter()
            .map(|ticket| {
                (
                    NewQuestion::raised_on(ticket.body.as_str()),
                    NewQuestion::text_of(ticket).to_owned(),
                )
            })
            .collect()
    }

    /// Opens a question on `on` and gives its number.
    fn question_on(&self, on: u64) -> TicketId {
        self.board
            .open_question(&NewQuestion {
                on: id(on),
                text: "Which way?".to_owned(),
                door: false,
            })
            .expect("question")
    }
}

/// Three runs of implement on #3 that each ended non-zero.
fn failed_three_times(scene: &Scene) {
    scene.phase_run(
        3,
        "implement",
        10,
        Some(1),
        "FAIL implement: exit 1\nfirst\n",
    );
    scene.phase_run(
        3,
        "implement",
        20,
        Some(2),
        "FAIL implement: exit 2\nsecond\n",
    );
    scene.phase_run(
        3,
        "implement",
        30,
        Some(3),
        "FAIL implement: exit 3, failed\nstep one\nno such file: spec.md\n\n",
    );
}

#[test]
fn phase_failed_three_times_is_not_started() {
    let scene = Scene::new();
    scene.task(3, "S");
    failed_three_times(&scene);
    let gathered = scene.gather();
    assert!(gathered.unreadable.is_empty(), "{gathered:?}");
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.start.is_empty(), "no fourth run starts: {calls:?}");
    assert!(tick.outcome.started.is_none(), "{tick:?}");
    assert!(tick.failed.is_empty(), "{tick:?}");
}

#[test]
fn a_phase_failed_twice_still_starts() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.phase_run(3, "implement", 10, Some(1), "");
    scene.phase_run(3, "implement", 20, Some(1), "");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "{calls:?}");
    assert!(tick.outcome.questions.is_empty());
    assert!(scene.asked().is_empty());
}

#[test]
fn a_success_between_failures_at_one_head_does_not_start_the_count_again() {
    // D8: a run that exited 0 and left the head where it was got nowhere too. Two failures, a run
    // that exited 0 with no pull request, then two more: five runs got nowhere, not a streak of
    // two.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.phase_run(3, "implement", 10, Some(1), "");
    scene.phase_run(3, "implement", 20, Some(1), "");
    scene.phase_run(3, "implement", 30, Some(0), "");
    scene.phase_run(3, "implement", 40, Some(1), "");
    scene.phase_run(3, "implement", 50, Some(1), "");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert!(
        scene.asked()[0].1.contains("(exit codes 0, 1, 1)"),
        "{:?}",
        scene.asked()
    );
}

#[test]
fn a_run_that_moved_the_head_starts_the_count_again() {
    let scene = Scene::new();
    scene.task(3, "S");
    // Two failures with no pull request, then a run that opened one changing no file (so
    // implement is not done) and two failures at its head: two runs got nowhere at h1.
    scene.phase_run(3, "implement", 10, Some(1), "");
    scene.phase_run(3, "implement", 20, Some(1), "");
    let pr = scene.open_pr(3, Some(CheckState::Passed));
    scene.forge.set_changed_files(pr, &[]).expect("files");
    scene.phase_run(3, "implement", 30, Some(0), "");
    scene.moved(3, "implement", 30, "h1");
    scene.phase_run_at(3, "implement", 40, Some(1), "", "h1");
    scene.phase_run_at(3, "implement", 50, Some(1), "", "h1");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "{calls:?}");
    assert!(tick.outcome.questions.is_empty());
    assert!(scene.asked().is_empty());
}

#[test]
fn phase_failed_three_times_raises_one_question() {
    let scene = Scene::new();
    scene.task(3, "S");
    failed_three_times(&scene);
    let gathered = scene.gather();
    let (tick, _) = run(&scene.board, &gathered, Ok(()));
    assert_eq!(tick.outcome.questions, [id(3)]);
    let asked = scene.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, Some(id(3)));
    let text = &asked[0].1;
    assert!(
        text.starts_with("implement got nowhere three times with no pull request"),
        "{text}"
    );
    assert!(text.contains("exit codes 1, 2, 3"), "{text}");
    assert!(
        text.contains("\"no such file: spec.md\""),
        "the last line of the last run: {text}"
    );
    assert_eq!(
        scene.board.task(id(3)).expect("task").state,
        Some(TicketState::Question),
        "the question makes the task wait"
    );

    // A second tick, on the same facts and on the board as it is now, raises no second one.
    let (again, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(again.outcome.questions.is_empty(), "{again:?}");
    assert!(calls.start.is_empty(), "{calls:?}");
    // Read again, the ticket waits on its question.
    let gathered = scene.gather();
    let question = scene.board.questions().expect("lists")[0].id;
    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Question(question)]))
    );
    let (later, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(later.outcome.questions.is_empty(), "{later:?}");
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(scene.asked().len(), 1);
}

#[test]
fn the_tick_names_each_question_with_its_cause() {
    let scene = Scene::new();
    scene.task(3, "S");
    failed_three_times(&scene);
    let (tick, _) = run(&scene.board, &scene.gather(), Ok(()));
    let question = scene.board.questions().expect("lists")[0].id;
    assert_eq!(tick.outcome.asked.len(), 1, "{tick:?}");
    assert_eq!(
        tick.outcome.asked[0].line(),
        format!("asked {question} on #3: implement got nowhere three times with no pull request")
    );
    assert_eq!(
        tick.outcome.render_text(),
        format!(
            "question on #3\nasked {question} on #3: implement got nowhere three times with no \
             pull request\n"
        ),
        "the pinned line stays, and the cause follows it"
    );
}

#[test]
fn a_dead_dependency_is_asked_by_the_tick() {
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(7, Some(EPIC), TicketState::Ready, "#9"));
    scene.task(9, "S");
    scene.board.close_not_planned(id(9));
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(7)]);
    assert_eq!(
        tick.outcome.asked[0].cause,
        "its dependency #9 is closed as not planned"
    );
    // Gathered again, it waits on the question too; nothing more is asked.
    let (again, _) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(again.outcome.asked.is_empty(), "{again:?}");
}

#[test]
fn a_ruled_question_lets_the_phase_start_again() {
    let scene = Scene::new();
    scene.task(3, "S");
    failed_three_times(&scene);
    run(&scene.board, &scene.gather(), Ok(()));
    let question = scene.board.questions().expect("lists")[0].id;
    scene
        .board
        .record_ruling(question, "Fix the spec path, then go on.")
        .expect("rules");

    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "the ruling lets it start: {calls:?}");
    assert!(tick.outcome.questions.is_empty());

    // It fails once more: that is one failure since the ruling, not three.
    scene.phase_run(3, "implement", 40, Some(1), "boom\n");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "{calls:?}");
    assert!(tick.outcome.questions.is_empty());

    // Three since the ruling hold it again, with a question of its own.
    scene.phase_run(3, "implement", 50, Some(1), "boom\n");
    scene.phase_run(3, "implement", 60, Some(4), "boom again\n");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert!(
        scene.asked()[0].1.contains("exit codes 1, 1, 4"),
        "{:?}",
        scene.asked()
    );
}

#[test]
fn a_failing_phase_does_not_hold_another_task() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    failed_three_times(&scene);
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5)),
        "{calls:?}"
    );
}

/// A scene whose task #3 has its pull request open at h1, changing a file, its CI failed at the
/// head: `fix`, the ci wait's `on-fail`, is next.
fn ci_failed() -> Scene {
    let scene = Scene::new();
    scene.fails_to_fix();
    scene.task(3, "S");
    scene.open_pr(3, Some(CheckState::Failed));
    scene
}

#[test]
fn a_fix_that_changes_nothing_three_times_raises_a_question() {
    let scene = ci_failed();
    // Each fix exited 0 but pushed nothing: the head stayed at h1.
    scene.phase_run_at(3, "fix", 40, Some(0), "nothing to fix\n", "h1");
    scene.phase_run_at(3, "fix", 50, Some(0), "nothing to fix\n", "h1");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(
        calls.start,
        [PlannedModel::Phase {
            task: id(3),
            action: name("fix"),
            units: 2,
        }],
        "twice is not yet nowhere"
    );
    assert!(tick.outcome.questions.is_empty());

    scene.phase_run_at(3, "fix", 60, Some(0), "still nothing\n", "h1");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "no fourth fix: {calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    let asked = scene.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].1.starts_with(
            "fix got nowhere three times at h1 (exit codes 0, 0, 0); the last line of its log: \
             \"still nothing\"."
        ),
        "{asked:?}"
    );
}

/// A failed ci on #3 whose pull request is at `head`, a commit of the scene's VCS (or any text,
/// for a head the VCS does not hold).
fn ci_failed_at(scene: &Scene, head: &str) {
    scene.fails_to_fix();
    scene.task(3, "S");
    let pr = scene.open_pr(3, Some(CheckState::Failed));
    scene.forge.set_head(pr, Some(head)).expect("head");
}

/// A finished run of `fix` on #3 that began at `from` and left `to`.
fn fix_between(scene: &Scene, started: u64, code: i32, from: &str, to: &str) {
    scene.phase_run_at(3, "fix", started, Some(code), "no change\n", from);
    write(&scene.run_dir(3, "fix", started).join("head-end"), to);
}

/// The scene's first commit, and what the next empty commits stand on.
fn first_commit(scene: &Scene) -> crate::domain::CommitId {
    let (slice, base) = (SliceName::from("slice"), BaseName::from("base"));
    scene.vcs.open(&slice, &base);
    crate::domain::CommitId::from("fake-base-base")
}

#[test]
fn three_rounds_at_one_tree_raise_r18() {
    let scene = Scene::new();
    let base = first_commit(&scene);
    ci_failed_at(&scene, base.as_str());
    for started in [40, 50, 60] {
        fix_between(&scene, started, 0, base.as_str(), base.as_str());
    }
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "no fourth fix: {calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert!(
        scene.asked()[0]
            .1
            .starts_with("fix got nowhere three times at fake-base-base"),
        "{:?}",
        scene.asked()
    );
}

#[test]
fn rounds_with_empty_commits_still_raise_r18() {
    let scene = Scene::new();
    let base = first_commit(&scene);
    // Each round added an empty commit: a new head every time, the tree never moved.
    let one = scene.vcs.empty_commit(&base).expect("empty");
    let two = scene.vcs.empty_commit(&one).expect("empty");
    let three = scene.vcs.empty_commit(&two).expect("empty");
    ci_failed_at(&scene, three.as_str());
    fix_between(&scene, 40, 0, base.as_str(), one.as_str());
    fix_between(&scene, 50, 0, one.as_str(), two.as_str());
    let (_, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "two rounds are not yet nowhere");
    fix_between(&scene, 60, 0, two.as_str(), three.as_str());
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "no fourth fix: {calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    // The question still names the current head.
    assert!(
        scene.asked()[0]
            .1
            .starts_with(&format!("fix got nowhere three times at {three} ")),
        "{:?}",
        scene.asked()
    );
}

#[test]
fn rounds_that_each_change_the_tree_raise_no_question() {
    let scene = Scene::new();
    let base = first_commit(&scene);
    let one = scene.vcs.changed_commit(&base).expect("changed");
    let two = scene.vcs.changed_commit(&one).expect("changed");
    let three = scene.vcs.changed_commit(&two).expect("changed");
    ci_failed_at(&scene, three.as_str());
    fix_between(&scene, 40, 0, base.as_str(), one.as_str());
    fix_between(&scene, 50, 0, one.as_str(), two.as_str());
    fix_between(&scene, 60, 0, two.as_str(), three.as_str());
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "fix starts again: {calls:?}");
    assert!(tick.outcome.questions.is_empty());
    assert!(scene.asked().is_empty());
}

#[test]
fn runs_at_an_unreadable_tree_count_by_commit_id() {
    let scene = Scene::new();
    // The VCS holds none of these heads: the same id counts, a different one does not.
    ci_failed_at(&scene, "h3");
    fix_between(&scene, 40, 0, "h1", "h2");
    fix_between(&scene, 50, 0, "h3", "h3");
    fix_between(&scene, 60, 0, "h3", "h3");
    let (_, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "two at h3, not three: {calls:?}");
    fix_between(&scene, 70, 0, "h3", "h3");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
}

#[test]
fn done_failed_done_failed_at_one_head_is_getting_nowhere() {
    let scene = ci_failed();
    scene.phase_run_at(3, "fix", 40, Some(0), "", "h1");
    scene.phase_run_at(3, "fix", 50, Some(1), "", "h1");
    scene.phase_run_at(3, "fix", 60, Some(0), "", "h1");
    scene.phase_run_at(3, "fix", 70, Some(1), "", "h1");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert!(
        scene.asked()[0].1.contains("(exit codes 1, 0, 1)"),
        "the last three: {:?}",
        scene.asked()
    );
}

#[test]
fn a_machinery_failure_never_counts() {
    let scene = ci_failed();
    // Two runs got nowhere; the rest failed in the machinery: one never ran its body (125), one
    // died with no exit code, one was stopped by a signal (143), one left an unknown head.
    scene.phase_run_at(3, "fix", 40, Some(1), "", "h1");
    scene.phase_run_at(3, "fix", 50, Some(125), "", "h1");
    scene.phase_run_at(3, "fix", 60, None, "", "h1");
    scene.phase_run_at(3, "fix", 70, Some(143), "", "h1");
    scene.phase_run_at(3, "fix", 80, Some(1), "", "h1");
    scene.phase_run_at(3, "fix", 90, Some(1), "", "h1");
    fs::remove_file(scene.state_dir.join("phases/fix-90-task-3/head-end")).expect("unknown head");
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "two count, not six: {calls:?}");
    assert!(tick.outcome.questions.is_empty());
    assert!(scene.asked().is_empty());
}

#[test]
fn a_ruling_lets_it_try_again() {
    let scene = ci_failed();
    for started in [40, 50, 60] {
        scene.phase_run_at(3, "fix", started, Some(1), "red\n", "h1");
    }
    let (tick, _) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(tick.outcome.questions, [id(3)]);
    let question = scene.board.questions().expect("lists")[0].id;
    scene
        .board
        .record_ruling(question, "The test is flaky; try once more.")
        .expect("rules");

    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "the ruling lets fix start: {calls:?}");
    assert!(tick.outcome.questions.is_empty());

    // The runs before the ruling's marker no longer count: two since, still not three.
    scene.phase_run_at(3, "fix", 70, Some(1), "red\n", "h1");
    scene.phase_run_at(3, "fix", 80, Some(0), "\n", "h1");
    let (_, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.start.len(), 1, "{calls:?}");
}

#[test]
fn a_merge_refused_three_times_at_one_head_raises_a_question() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.open_pr(3, Some(CheckState::Passed));
    // The forge refused the merge three times; the head stayed at h1.
    for started in [40, 50, 60] {
        scene.phase_run_at(3, "merge", started, Some(1), "not mergeable\n", "h1");
    }
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.run.is_empty(), "no fourth merge: {calls:?}");
    assert!(tick.outcome.ran.is_empty(), "{tick:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert!(
        scene.asked()[0]
            .1
            .starts_with("merge got nowhere three times at h1 (exit codes 1, 1, 1)"),
        "{:?}",
        scene.asked()
    );
    // While it is open, the next tick neither merges nor asks again.
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.run.is_empty() && tick.outcome.questions.is_empty());
}

#[test]
fn a_task_of_an_unsigned_epic_is_held_back() {
    let scene = Scene::new();
    scene.board.add_epic(id(7), "Unsigned");
    scene.task_under(7, 3);

    let gathered = scene.gather();

    assert!(gathered.unreadable.is_empty(), "held is no error");
    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Epic(id(7))]))
    );
    assert_eq!(
        lines(&held(&gathered, now())),
        ["#3 skipped: its epic #7 is not signed"]
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.started.is_none());
    assert_eq!(lines(&tick.held), ["#3 skipped: its epic #7 is not signed"]);
}

#[test]
fn a_task_of_an_epic_with_an_open_question_is_held_back() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene
        .board
        .open_question(&NewQuestion {
            on: id(EPIC),
            text: "Which door?".to_owned(),
            door: true,
        })
        .expect("question");

    let gathered = scene.gather();

    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(
        lines(&tick.held),
        ["#3 skipped: a question is open on its epic #2"]
    );
}

#[test]
fn a_task_with_a_question_of_its_own_is_held_back() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.question_on(3);

    let gathered = scene.gather();

    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(lines(&tick.held), ["#3 skipped: a question is open on it"]);
}

#[test]
fn a_task_of_an_epic_not_challenged_yet_is_held_back() {
    // Also what the JSON dry run names (`held`, the same function) for a signed epic not
    // challenged yet.
    let scene = Scene::new();
    scene.board.add_signed_epic(id(7), "Signed, not challenged");
    scene.task_under(7, 3);

    let gathered = scene.gather();

    let expected = ["#3 skipped: its epic #7 is not challenged yet"];
    assert_eq!(lines(&held(&gathered, now())), expected);
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(lines(&tick.held), expected);
}

#[test]
fn a_started_ticket_waits_on_its_epic_too() {
    // The old path let a claimed task go on whatever its epic said; the placement holds it too.
    let scene = Scene::new();
    scene.board.add_epic(id(7), "Unsigned");
    scene.task_under(7, 3);
    scene.task_under(7, 4);
    scene
        .board
        .claim(
            id(3),
            &Instance {
                role: RoleName::from("team"),
                epic: id(7),
                number: 1.into(),
            },
        )
        .expect("claims");
    // #3 is well on: its pull request is open and green, so merge would be next.
    scene.open_pr(3, Some(CheckState::Passed));

    let gathered = scene.gather();

    assert_eq!(numbers(&gathered), [3, 4]);
    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Epic(id(7))]))
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "nothing of #3 or #4 runs: {calls:?}");
    assert_eq!(
        lines(&tick.held),
        [
            "#3 skipped: its epic #7 is not signed",
            "#4 skipped: its epic #7 is not signed"
        ]
    );
    assert_eq!(
        refused(scene.spawn_phase(3, "merge")),
        "merge is not #3's current phase: merge waits: its epic #7 is not signed"
    );
}

#[test]
fn a_running_phase_is_not_cut_off_by_a_question_on_its_epic() {
    let scene = Scene::new();
    scene.task(3, "S");
    scene.process_run(3, "implement", 50, "", LIVE);
    scene.question_on(EPIC);
    // #5 sits under another epic, cleared, so only the slots hold it.
    scene.board.add_challenged_epic(id(8), "Issue 8");
    scene.task_under(8, 5);

    let gathered = scene.gather();

    assert!(
        matches!(place_of(&gathered, 3), Place::Running(run) if run.phase == name("implement")),
        "{:?}",
        place_of(&gathered, 3)
    );
    let (tick, calls) = run_with(&scene.board, &gathered, 1, Ok(()));
    assert!(
        tick.held.is_empty(),
        "a running ticket is not held: {tick:?}"
    );
    assert!(
        calls.is_empty(),
        "#3's run keeps its one slot, so #5 does not start: {calls:?}"
    );
    assert_eq!(
        refused(scene.spawn_phase(3, "implement")),
        "#3's phase implement already runs"
    );
}

#[test]
fn review_is_not_started_before_a_check_ends_at_the_new_head() {
    let scene = Scene::with_list(r#"["implement", "ci", "review", "merge"]"#);
    scene.action("implement", "model", "pr-open");
    scene.action("ci", "wait", "ci-done");
    scene.action("review", "model", "exit");
    scene.action("merge", "zaf", "merged");
    scene.task(3, "S");
    let pr = scene.open_pr(3, Some(CheckState::Passed));
    // Reviewed at h1: merge is next there.
    scene.phase_run_at(3, "review", 40, Some(0), "approved\n", "h1");
    let (_, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(calls.run, [(3, "merge".to_owned())]);

    // A push: the head is h2 and its check has not ended. The review at h1 no longer counts, and
    // nothing starts until the check at h2 ends.
    scene.forge.set_head(pr, Some("h2")).expect("head");
    scene.checks(pr, CheckState::Pending);
    let gathered = scene.gather();
    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Check(
            id(3),
            crate::domain::CommitId::from("h2")
        )]))
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert!(tick.outcome.is_idle(), "{tick:?}");
    assert_eq!(
        refused(scene.spawn_phase(3, "review")),
        "review is not #3's current phase: ci waits: the checks at h2 have not ended"
    );

    // Once it ends, review starts at h2.
    scene.checks(pr, CheckState::Passed);
    let (_, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(
        calls.start,
        [PlannedModel::Phase {
            task: id(3),
            action: name("review"),
            units: 2,
        }]
    );
}

#[test]
fn a_dead_process_holds_no_slot() {
    // One slot. #3's implement run died with no exit recorded: it is no run, holds no slot, and
    // implement is in the lane again.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    scene.process_run(3, "implement", 50, "", 9999);
    let gathered = scene.gather();
    assert!(
        Placement::of(&gathered.tickets, now()).running.is_empty(),
        "{gathered:?}"
    );
    let (tick, _) = run_with(&scene.board, &gathered, 1, Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(3))
    );

    // A live one holds the slot: nothing starts.
    scene.process_run(3, "implement", 60, "", LIVE);
    let (tick, calls) = run_with(&scene.board, &scene.gather(), 1, Ok(()));
    assert!(tick.outcome.started.is_none(), "{calls:?}");
    assert!(calls.start.is_empty(), "{calls:?}");
}

#[test]
fn a_live_wait_holds_no_slot() {
    // One slot. #3's ci wait runs; a wait costs nothing, so #5's implement starts.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    scene.open_pr(3, Some(CheckState::Pending));
    scene.process_run(3, "ci", 50, "h1", LIVE);
    let gathered = scene.gather();
    assert!(matches!(place_of(&gathered, 3), Place::Running(_)));
    let (tick, _) = run_with(&scene.board, &gathered, 1, Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5))
    );
}

/// Runs the tick on `scene` and asks `zaf spawn phase` to start `action` on #3: neither starts
/// anything of #3, the tick holds nothing else back, no run folder is written, and the refusal is
/// `words`.
fn held_alike(scene: &Scene, action: &str, words: &str) {
    let folders = scene.run_folders();
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.is_empty(), "the tick starts nothing of #3: {calls:?}");
    assert!(tick.outcome.is_idle(), "{tick:?}");
    assert_eq!(refused(scene.spawn_phase(3, action)), words);
    assert_eq!(scene.run_folders(), folders, "no run folder is written");
}

#[test]
fn the_tick_and_spawn_phase_hold_a_ticket_to_the_same_gate() {
    // Waiting on its epic.
    let scene = Scene::new();
    scene.board.add_epic(id(7), "Unsigned");
    scene.task_under(7, 3);
    held_alike(
        &scene,
        "implement",
        "implement is not #3's current phase: implement waits: its epic #7 is not signed",
    );

    // Waiting on a question.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.question_on(3);
    held_alike(
        &scene,
        "implement",
        "implement is not #3's current phase: implement waits: a question is open on it",
    );

    // Waiting on a ticket it depends on, #4, open (of a type with no list, so not in the lane).
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(3, Some(EPIC), TicketState::Ready, "#4"));
    scene
        .board
        .add_ticket(ticket(4, Some(EPIC), TicketState::Ready, "—"));
    scene.board.set_type(id(4), "Bug");
    held_alike(
        &scene,
        "implement",
        "implement is not #3's current phase: implement waits: #4 is not done",
    );

    // Waiting on the checks at its head.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.open_pr(3, Some(CheckState::Pending));
    held_alike(
        &scene,
        "ci",
        "ci is not #3's current phase: ci waits: the checks at h1 have not ended",
    );
    assert_eq!(
        refused(scene.spawn_phase(3, "merge")),
        "merge is not #3's current phase: ci waits: the checks at h1 have not ended"
    );

    // Getting nowhere: neither starts a fourth run. The tick raises the question, and from then
    // on the ticket waits on it, in both.
    let scene = Scene::new();
    scene.task(3, "S");
    failed_three_times(&scene);
    let folders = scene.run_folders();
    assert_eq!(
        refused(scene.spawn_phase(3, "implement")),
        "implement is not started on #3: it got nowhere 3 times with no pull request; it starts \
         again once the question on it is ruled"
    );
    let (tick, calls) = run(&scene.board, &scene.gather(), Ok(()));
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(tick.outcome.questions, [id(3)]);
    assert_eq!(scene.run_folders(), folders);
    held_alike(
        &scene,
        "implement",
        "implement is not #3's current phase: implement waits: a question is open on it",
    );
}

#[test]
fn a_done_ticket_is_never_placed_or_started_again() {
    // Labelled done, or closed: not among the open tickets, so never gathered or placed.
    let finishes: [fn(&Scene); 2] = [
        |scene: &Scene| {
            scene
                .board
                .set_state(id(3), TicketState::Done)
                .expect("labels");
        },
        |scene: &Scene| scene.board.close(id(3)),
    ];
    for finish in finishes {
        let scene = Scene::new();
        scene.task(3, "S");
        finish(&scene);
        let gathered = scene.gather();
        assert!(numbers(&gathered).is_empty(), "{gathered:?}");
        let (tick, calls) = run(&scene.board, &gathered, Ok(()));
        assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
        assert_eq!(
            refused(scene.spawn_phase(3, "implement")),
            "implement is not #3's current phase: #3 is not open, so it is done or closed"
        );
    }

    // Its pull request merged, the label not yet moved: placed done, and nothing starts.
    let scene = Scene::new();
    scene.task(3, "S");
    let pr = scene.open_pr(3, Some(CheckState::Passed));
    scene.forge.set_merged_at(pr, time(40)).expect("merged at");
    scene.forge.merge(pr).expect("merges");
    let gathered = scene.gather();
    assert_eq!(place_of(&gathered, 3), Place::Done);
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
    assert_eq!(
        refused(scene.spawn_phase(3, "merge")),
        "merge is not #3's current phase: every phase is done"
    );
}

#[test]
fn a_ticket_labelled_blocked_whose_dependency_is_done_is_started() {
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(3, Some(EPIC), TicketState::Blocked, "#4"));
    scene
        .board
        .add_ticket(ticket(4, Some(EPIC), TicketState::Done, "—"));
    let (tick, _) = run(&scene.board, &scene.gather(), Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(3)),
        "the label is not read: the dependency is done"
    );
}

#[test]
fn a_ticket_labelled_ready_whose_dependency_is_open_is_not() {
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(3, Some(EPIC), TicketState::Ready, "#4"));
    scene
        .board
        .add_ticket(ticket(4, Some(EPIC), TicketState::Blocked, "—"));
    scene.board.set_type(id(4), "Bug");
    let gathered = scene.gather();
    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Ticket(id(4))]))
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
    assert_eq!(
        refused(scene.spawn_phase(3, "implement")),
        "implement is not #3's current phase: implement waits: #4 is not done"
    );
}

#[test]
fn a_run_that_opened_a_pull_request_and_asked_waits_on_its_question() {
    // implement opened a pull request changing a file and asked a question on the way. A run that
    // raised a question does not end its phase, so implement stays current, though CI is green;
    // the question holds the ticket.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.open_pr(3, Some(CheckState::Passed));
    let question = scene.question_on(3);
    scene.phase_run(3, "implement", 10, Some(0), "opened the pull request\n");
    scene.moved(3, "implement", 10, "h1");
    write(
        &scene.run_dir(3, "implement", 10).join("questions"),
        &format!("{}\n", question.get()),
    );

    let gathered = scene.gather();

    assert_eq!(
        place_of(&gathered, 3),
        Place::Waiting(BTreeSet::from([WaitingOn::Question(question)]))
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty(), "no merge: {calls:?}");
    assert_eq!(lines(&tick.held), ["#3 skipped: a question is open on it"]);
    assert_eq!(
        refused(scene.spawn_phase(3, "merge")),
        "merge is not #3's current phase: implement waits: a question is open on it"
    );
}

#[test]
fn a_cleared_epics_ready_task_starts() {
    let scene = Scene::new();
    scene.task(3, "S");

    let gathered = scene.gather();

    assert!(held(&gathered, now()).is_empty(), "{gathered:?}");
    let (tick, _) = run(&scene.board, &gathered, Ok(()));
    assert!(tick.held.is_empty(), "{tick:?}");
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(3))
    );
}

/// The tickets `gathered` skipped, each with why.
fn skipped(gathered: &Gathered) -> Vec<(u64, String)> {
    gathered
        .unreadable
        .iter()
        .map(|unreadable| (unreadable.ticket.get(), unreadable.reason.clone()))
        .collect()
}

#[test]
fn a_ticket_with_no_epic_is_named_and_never_started() {
    // A hand-filed task that names no epic has no signature to run under, so it does not run: the
    // tick skips it, named, and `zaf spawn phase` refuses it in the same words, at any action.
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(3, None, TicketState::Ready, "—"));

    let gathered = scene.gather();

    assert!(numbers(&gathered).is_empty(), "{gathered:?}");
    assert_eq!(
        skipped(&gathered),
        [(3, "it is not a task of an epic".to_owned())]
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
    assert!(tick.failed.is_empty(), "{tick:?}");
    for action in ["implement", "ci", "merge"] {
        assert_eq!(
            refused(scene.spawn_phase(3, action)),
            "#3 cannot start: it is not a task of an epic"
        );
    }

    // Nor does its `zaf`-kind action run inline, nor its wait launch, once a pull request is open.
    scene.open_pr(3, Some(CheckState::Passed));
    let gathered = scene.gather();
    assert_eq!(skipped(&gathered).len(), 1, "{gathered:?}");
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
    assert!(scene.run_folders().is_empty(), "no run folder is written");
}

#[test]
fn a_ticket_that_cannot_start_does_not_stop_the_others() {
    // #3 names no epic and is numbered below #5, a healthy task; both would be in the lane at
    // implement since the same time, #3 first. The tick's start is `zaf spawn phase`'s own code.
    let scene = Scene::new();
    scene
        .board
        .add_ticket(ticket(3, None, TicketState::Ready, "—"));
    scene.task(5, "S");
    let gathered = scene.gather();

    let mut start = |planned: &PlannedModel| scene.start_phase(planned);
    let (tick, calls) = run_on(
        &scene.board,
        &gathered,
        &idle(2),
        &Budget::default(),
        &mut start,
    );

    assert_eq!(
        calls.start,
        [PlannedModel::Phase {
            task: id(5),
            action: name("implement"),
            units: 2,
        }],
        "#3 is never planned"
    );
    assert!(tick.failed.is_empty(), "the tick does not exit 2: {tick:?}");
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5)),
        "the healthy one starts this tick"
    );
    assert_eq!(scene.run_folders().len(), 1, "#5's run alone: {gathered:?}");
}

#[test]
fn a_launcher_that_recorded_the_action_first_keeps_its_record() {
    // The launcher is a process of its own: it may record the action's process before the verb
    // records the launcher. The verb then leaves the record as it is (#223).
    let scene = Scene::new();
    scene.task(5, "S");
    let action = Holder {
        pid: LIVE,
        start: "2000".to_owned(),
    };

    let started = phase::start(
        &scene.ports(),
        id(5),
        "implement",
        &|started: &phase::Started| {
            phase::record_pid(&started.run_dir, &action)?;
            Ok(Holder {
                pid: LIVE,
                start: "1000".to_owned(),
            })
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        fs::read_to_string(started.run_dir.join(phase::PID_FILE)).expect("pid"),
        action.to_string(),
        "the action's process, not the launcher's"
    );
}

#[test]
fn a_live_model_run_holds_its_slot_though_its_ticket_cannot_be_read() {
    // One slot. #3's implement runs, live, but #3 records a pull request the forge does not know,
    // so #3 cannot be read this tick. Its run still holds the slot: #5 does not start.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(5, "S");
    scene.process_run(3, "implement", 50, "", LIVE);
    scene.record_pr(3, "99");

    let gathered = scene.gather();

    assert_eq!(
        skipped(&gathered)
            .iter()
            .map(|(ticket, _)| *ticket)
            .collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(gathered.model_runs, Ok(1), "counted from its run folder");
    let (tick, calls) = run_with(&scene.board, &gathered, 1, Ok(()));
    assert!(calls.start.is_empty(), "one over the slots: {calls:?}");
    assert!(tick.outcome.started.is_none(), "{tick:?}");
    // With a second slot, #5 starts beside it.
    let (tick, _) = run_with(&scene.board, &gathered, 2, Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5))
    );
}

#[test]
fn live_model_runs_that_cannot_be_counted_start_no_model() {
    // The count fails: the steps that cost nothing still run, no model starts, and the tick fails.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.task(4, "S");
    scene.open_pr(4, Some(CheckState::Passed));
    let gathered = Gathered {
        model_runs: Err("phases: permission denied".to_owned()),
        ..scene.gather()
    };

    let (tick, calls) = run(&scene.board, &gathered, Ok(()));

    assert_eq!(calls.run, [(4, "merge".to_owned())], "merge still runs");
    assert!(calls.start.is_empty(), "{calls:?}");
    assert_eq!(
        tick.failed,
        ["no model starts: the live model runs cannot be counted: phases: permission denied"]
    );
}

/// A process that outlived its run: python holding `mebibytes` of memory, carrying the run's
/// `ZAF_RUN` and `ZAF_SIZE` as the run's own processes do. Killed when dropped.
struct Orphan(std::process::Child);

impl Orphan {
    /// Starts it on `run`, of `size`, and returns once the machine read sees its memory.
    fn start(run: &Path, size: &str, mebibytes: u64) -> Self {
        let script = format!("import time\nblock = b'x' * ({mebibytes} << 20)\ntime.sleep(30)");
        let child = std::process::Command::new("python3")
            .args(["-c", &script])
            .env_remove(crate::ports::INSTANCE_VAR)
            .env(crate::ports::RUN_VAR, run)
            .env(crate::ports::SIZE_VAR, size)
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("runs python3");
        let orphan = Self(child);
        for _ in 0..200 {
            if machine_under(run)
                .iter()
                .any(|instance| instance.memory.get() >= mebibytes)
            {
                return orphan;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the orphan never held {mebibytes} MiB");
    }
}

impl Drop for Orphan {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What the budget reads of the machine: every live zaf process whose run is under `dir`.
fn machine_under(dir: &Path) -> Vec<crate::ports::MachineInstance> {
    crate::adapters::process::machine()
        .into_iter()
        .filter(|instance| instance.run.starts_with(dir))
        .collect()
}

#[test]
fn an_orphan_still_counts_toward_memory_and_the_e_rule_but_holds_no_slot() {
    use crate::ports::{e_is_live, live_use};
    // #9's implement run has ended (its exit is recorded, and #9 is long closed), but a grandchild
    // of size E outlived it, still carrying its `ZAF_RUN` and holding 64 MiB. #4 (S) and #5 (E)
    // are in the lane at implement.
    let scene = Scene::new();
    scene.task(4, "S");
    scene.task(5, "E");
    scene.phase_run(9, "implement", 10, Some(1), "FAIL implement: exit 1\n");
    let run = scene.run_dir(9, "implement", 10);
    let _orphan = Orphan::start(&run, "E", 64);

    let read = machine_under(&scene.root);

    assert_eq!(read.len(), 1, "{read:?}");
    assert!(read[0].memory.get() >= 64, "its memory is read: {read:?}");
    assert!(e_is_live(&read), "it holds the one E");
    assert!(
        crate::adapters::process::displayed()
            .iter()
            .all(|instance| !instance.run.starts_with(&scene.root)),
        "zaf status does not show it as an instance"
    );
    let gathered = scene.gather();
    assert_eq!(gathered.model_runs, Ok(0), "it holds no slot");
    let machine = Machine {
        units: live_use(&read),
        e_live: e_is_live(&read),
        ..idle(1)
    };

    // Its memory counts: an S costs the whole budget of 2 units, which the orphan leaves short.
    // On an idle machine, the S would start.
    let tight = Budget {
        units: Some(2),
        ..Budget::default()
    };
    let (tick, calls) = run_on(&scene.board, &gathered, &machine, &tight, &mut |_| Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert!(tick.outcome.started.is_none(), "{tick:?}");
    let (tick, _) = run_on(&scene.board, &gathered, &idle(1), &tight, &mut |_| Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(4))
    );

    // It holds the one E: with #4 done, the E does not start beside it, whatever budget is left.
    // With no E live, it would.
    scene
        .board
        .set_state(id(4), TicketState::Done)
        .expect("labels");
    let gathered = scene.gather();
    assert_eq!(numbers(&gathered), [5]);
    let wide = Budget {
        units: Some(100),
        ..Budget::default()
    };
    let (tick, calls) = run_on(&scene.board, &gathered, &machine, &wide, &mut |_| Ok(()));
    assert!(calls.start.is_empty(), "{calls:?}");
    assert!(tick.outcome.started.is_none(), "{tick:?}");
    let no_e = Machine {
        e_live: false,
        ..machine
    };
    let (tick, _) = run_on(&scene.board, &gathered, &no_e, &wide, &mut |_| Ok(()));
    assert_eq!(
        tick.outcome.started.map(|started| started.task),
        Some(id(5))
    );
}

#[test]
fn a_garbled_pull_request_record_makes_the_ticket_unreadable_and_starts_nothing() {
    // #3's pull request is open, but its record holds no number. Read as "no pull request", it
    // would put #3 back at implement over the open pull request; it is skipped instead, named
    // with why, and `zaf spawn phase` fails on it in the same words.
    let scene = Scene::new();
    scene.task(3, "S");
    scene.open_pr(3, Some(CheckState::Passed));
    scene.record_pr(3, "#1 opened");

    let gathered = scene.gather();

    assert!(numbers(&gathered).is_empty(), "{gathered:?}");
    let skipped = skipped(&gathered);
    assert_eq!(skipped.len(), 1, "{gathered:?}");
    let (ticket, reason) = &skipped[0];
    assert_eq!(*ticket, 3);
    assert!(
        reason.starts_with("its pull request record ")
            && reason.ends_with("3.pr: holds \"#1 opened\", not a pull request number"),
        "{reason}"
    );
    let (tick, calls) = run(&scene.board, &gathered, Ok(()));
    assert!(calls.is_empty() && tick.outcome.is_idle(), "{calls:?}");
    match scene.spawn_phase(3, "implement") {
        Err(PhaseError::Failed(words)) => assert_eq!(words, format!("#3 cannot be read: {reason}")),
        other => panic!("not failed: {other:?}"),
    }
    assert!(scene.run_folders().is_empty(), "no run folder is written");
}

mod fixing {
    //! A merge whose base conflicts, and the failures no `on-fail` answers (slice 8).

    use super::*;
    use crate::domain::schedule::{
        CheckFact, CheckName, CheckResult, ExitCode, HeadEnd, PhaseEnd, PullFacts, RunExit,
    };
    use crate::domain::{CommitId, TaskSize};

    /// The pack's list with merge failing to `fix`: implement, ci and review fail to `fix`, merge.
    fn fixing_list() -> Vec<ListedPhase> {
        let listed = |action: &str, kind, ends, on_fail: Option<&str>| ListedPhase {
            action: name(action),
            kind,
            ends,
            on_fail: on_fail.map(name),
        };
        vec![
            listed("implement", ActionKind::Model, PhaseEnd::PrOpen, None),
            listed("ci", ActionKind::Wait, PhaseEnd::CiDone, Some("fix")),
            listed("review", ActionKind::Wait, PhaseEnd::Exit, Some("fix")),
            listed("merge", ActionKind::Zaf, PhaseEnd::Merged, Some("fix")),
        ]
    }

    /// A record of `action` at `h1`, started at `started`, that exited `code` and left `h1`.
    fn ended_at_h1(action: &str, started: u64, code: i32) -> RunRecord {
        RunRecord {
            action: name(action),
            head: Some(CommitId::from("h1")),
            head_end: HeadEnd::Left(Some(CommitId::from("h1"))),
            started_at: time(started),
            alive: false,
            exit: Some(RunExit {
                code: ExitCode::from(code),
                at: time(started + 1),
            }),
            questions: BTreeSet::new(),
            last_line: None,
        }
    }

    /// Pull request 7 at `h1`, opened at 10, with a green check.
    fn green_at_h1() -> PullFacts {
        PullFacts {
            opened_at: time(10),
            merged_at: None,
            head: Some(CommitId::from("h1")),
            changed_files: crate::domain::ChangedFiles::of(&["src/lib.rs"]),
            checks: vec![CheckFact {
                name: CheckName::from("ci"),
                result: CheckResult::Passed(time(12)),
            }],
            reviews: Vec::new(),
            mergeable: None,
            checks_expected: true,
        }
    }

    #[test]
    fn a_merge_whose_base_conflicts_sorts_to_the_fix_model() {
        let list = fixing_list();
        let runs = vec![ended_at_h1("review", 20, 0), ended_at_h1("merge", 30, 1)];
        let facts = support::facts(7, &list, TaskSize::S, time(1), Some(green_at_h1()), runs);
        let sorted = support::sorted(&support::gathered(vec![(facts, list)]), now());
        assert_eq!(
            sorted
                .models
                .iter()
                .map(|entry| entry.next.clone())
                .collect::<Vec<_>>(),
            [name("fix")]
        );
        assert!(sorted.zaf.is_empty(), "the merge is not run inline again");
    }

    #[test]
    fn a_merge_the_forge_refused_runs_again_until_it_gets_nowhere() {
        let list = fixing_list();
        let mut runs = vec![ended_at_h1("review", 20, 0), ended_at_h1("merge", 30, 4)];
        let facts = support::facts(
            7,
            &list,
            TaskSize::S,
            time(1),
            Some(green_at_h1()),
            runs.clone(),
        );
        let sorted = support::sorted(&support::gathered(vec![(facts, list.clone())]), now());
        assert_eq!(sorted.zaf.len(), 1, "{sorted:?}");
        assert_eq!(sorted.zaf[0].next, name("merge"));
        assert!(sorted.models.is_empty(), "no fix for a refusal");

        runs.push(ended_at_h1("merge", 40, 4));
        runs.push(ended_at_h1("merge", 50, 4));
        let facts = support::facts(7, &list, TaskSize::S, time(1), Some(green_at_h1()), runs);
        let sorted = support::sorted(&support::gathered(vec![(facts, list)]), now());
        assert!(sorted.zaf.is_empty());
        assert_eq!(sorted.nowhere.len(), 1, "three refusals at one head ask");
    }

    #[test]
    fn a_wait_that_failed_on_a_backend_is_asked_not_fixed() {
        let list = fixing_list();
        let runs = vec![ended_at_h1("review", 20, 2)];
        let facts = support::facts(7, &list, TaskSize::S, time(1), Some(green_at_h1()), runs);
        let sorted = support::sorted(
            &support::gathered(vec![(facts.clone(), list.clone())]),
            now(),
        );
        assert!(sorted.models.is_empty(), "{sorted:?}");
        assert_eq!(sorted.failed_waits.len(), 1, "{sorted:?}");
        let beside = Beside {
            list,
            epic: id(1),
            on_epic: BTreeSet::new(),
        };
        let refusal = may_start(&facts, &beside, &name("review"), now()).expect_err("refused");
        assert_eq!(
            refusal.reason(id(7), "review"),
            "review is not #7's current phase: review failed on something no on-fail action answers"
        );
    }

    #[test]
    fn the_failed_phase_names_the_run_that_failed_it() {
        let list = fixing_list();
        let runs = vec![ended_at_h1("review", 20, 0), ended_at_h1("merge", 30, 1)];
        let facts = support::facts(7, &list, TaskSize::S, time(1), Some(green_at_h1()), runs);
        let failed = failed_phase(&facts).expect("the merge failed");
        assert_eq!(failed.phase.name, name("merge"));
        assert_eq!(failed.failure.exit, Some(ExitCode::from(1)));
        assert_eq!(failed.run.map(|run| run.started_at), Some(time(30)));
        let open = support::facts(
            7,
            &list,
            TaskSize::S,
            time(1),
            Some(green_at_h1()),
            Vec::new(),
        );
        assert_eq!(failed_phase(&open), None);
    }
}

mod one_merge_a_tick {
    //! One merge a tick, and the words of a merge's waits (R29, R30).

    use super::*;
    use crate::app::schedule::gather::Base;
    use crate::domain::CommitId;
    use crate::domain::schedule::{BaseFacts, CheckFact, CheckResult};
    use crate::ports::BranchName;

    /// Three tasks, each with its pull request open and CI green: `merge` is next for each.
    fn three_at_their_merge() -> (Scene, Gathered) {
        let scene = Scene::new();
        for task in [3, 4, 5] {
            scene.task(task, "S");
            scene.open_pr(task, Some(CheckState::Passed));
        }
        let gathered = scene.gather();
        (scene, gathered)
    }

    /// The tick's steps on `gathered`, `zaf do` exiting as `exit` says for each task, a merge that
    /// exits 0 merged on the forge, and `per_tick` merges allowed.
    fn run_exiting(
        board: &dyn Board,
        gathered: &Gathered,
        exit: impl Fn(u64) -> u8,
    ) -> (PhaseTick, Vec<u64>) {
        run_group(board, gathered, 1, &exit, &|task| exit(task) == 0).0
    }

    /// The tick's steps with `per_tick` merges allowed, `zaf do` exiting as `exit` says and the
    /// forge saying `merged` of each task's pull request; the log's lines come with it.
    fn run_group(
        board: &dyn Board,
        gathered: &Gathered,
        per_tick: u32,
        exit: &dyn Fn(u64) -> u8,
        merged: &dyn Fn(u64) -> bool,
    ) -> ((PhaseTick, Vec<u64>), Vec<String>) {
        let mut ran = Vec::new();
        let mut logged = Vec::new();
        let mut run = |task: TicketId, _: &ActionName| {
            ran.push(task.get());
            exit(task.get())
        };
        let state_dir = tempfile::tempdir().expect("tempdir");
        let tick = tick(
            board,
            state_dir.path(),
            gathered,
            now(),
            &Machine {
                merge_per_tick: per_tick,
                ..idle(2)
            },
            &Budget::default(),
            &mut Steps {
                run: &mut run,
                launch: &mut |_: TicketId, _: &ActionName| Ok(()),
                start: &mut |_: &PlannedModel| Ok(()),
                pr_of: &mut |task: TicketId| task.get().to_string().parse().ok(),
                merged: &mut |pr: crate::ports::PrNumber| merged(pr.get()),
                branch_head: &mut || None,
                group: &mut |_| {},
                log: &mut |line: &str| logged.push(line.to_owned()),
            },
        );
        ((tick, ran), logged)
    }

    /// `gathered` over an integration branch at `b1` with `checks` at its head.
    fn over_branch(mut gathered: Gathered, checks: Vec<CheckFact>) -> Gathered {
        gathered.base = Some(Base {
            branch: BranchName::from("feature/v1"),
            facts: BaseFacts {
                head: Some(CommitId::from("b1")),
                checks,
                reached_at: None,
            },
        });
        gathered
    }

    #[test]
    fn a_red_head_lands_one_merge() {
        let (scene, gathered) = three_at_their_merge();
        let failed = CheckFact {
            name: CheckName::from("ci"),
            result: CheckResult::Failed(now()),
        };
        let gathered = over_branch(gathered, vec![failed]);
        let ((tick, ran), _) = run_group(&scene.board, &gathered, 8, &|_| 0, &|_| true);
        assert_eq!(ran, [3], "red: one merge, whatever per_tick says");
        assert_eq!(
            lines(&tick.held),
            [
                "#4 skipped: #3 merged in this tick, so its merge waits for the next one",
                "#5 skipped: #3 merged in this tick, so its merge waits for the next one",
            ]
        );
    }

    #[test]
    fn a_head_with_no_check_is_not_red() {
        let (scene, gathered) = three_at_their_merge();
        let gathered = over_branch(gathered, Vec::new());
        let ((tick, ran), _) = run_group(&scene.board, &gathered, 8, &|_| 0, &|_| true);
        assert_eq!(ran, [3, 4, 5]);
        assert!(tick.held.is_empty(), "{:?}", tick.held);
    }

    #[test]
    fn the_group_keeps_its_last_commit_with_the_start_base_after_each_merge() {
        use crate::domain::CommitId;
        use crate::domain::schedule::{BaseFacts, GroupBase};
        let (scene, mut gathered) = three_at_their_merge();
        let at_start = BaseFacts {
            head: Some(CommitId::from("b0")),
            ..BaseFacts::default()
        };
        gathered.base = Some(crate::app::schedule::gather::Base {
            branch: "feature/v1".into(),
            facts: at_start.clone(),
        });
        // The branch moves with each merge: b1 after #3, b2 after #4.
        let heads = std::cell::RefCell::new(vec!["b2", "b1"]);
        let kept = std::cell::RefCell::new(Vec::new());
        let state_dir = tempfile::tempdir().expect("tempdir");
        tick(
            &scene.board,
            state_dir.path(),
            &gathered,
            now(),
            &Machine {
                merge_per_tick: 2,
                ..idle(2)
            },
            &Budget::default(),
            &mut Steps {
                run: &mut |_: TicketId, _: &ActionName| 0,
                launch: &mut |_: TicketId, _: &ActionName| Ok(()),
                start: &mut |_: &PlannedModel| Ok(()),
                pr_of: &mut |task: TicketId| task.get().to_string().parse().ok(),
                merged: &mut |_: crate::ports::PrNumber| true,
                branch_head: &mut || heads.borrow_mut().pop().map(CommitId::from),
                group: &mut |own: Option<GroupBase>| kept.borrow_mut().push(own),
                log: &mut |_: &str| {},
            },
        );
        let own = |head: &str| {
            Some(GroupBase {
                head: CommitId::from(head),
                base: at_start.clone(),
            })
        };
        // Each landed merge keeps the head then read with the base as the tick began; the group
        // clears it when it ends, so nothing outlives the tick.
        assert_eq!(*kept.borrow(), [own("b1"), own("b2"), None]);
    }

    #[test]
    fn a_group_lands_every_ready_merge() {
        let (scene, gathered) = three_at_their_merge();
        let ((tick, ran), logged) = run_group(&scene.board, &gathered, 8, &|_| 0, &|_| true);
        assert_eq!(ran, [3, 4, 5]);
        assert_eq!(tick.outcome.ran.len(), 3);
        assert!(tick.held.is_empty(), "{:?}", tick.held);
        assert!(tick.failed.is_empty(), "{tick:?}");
        assert!(
            logged
                .iter()
                .any(|line| line.starts_with("merge #4 of merge: exit 0, merged, ")),
            "each merge is logged: {logged:?}"
        );
        assert!(
            logged
                .iter()
                .any(|line| line.starts_with("merges: 3 landed, 0 left for the next tick, in ")),
            "the tick's length is logged: {logged:?}"
        );
    }

    #[test]
    fn tick_logs_merged_for_each_landed_merge() {
        use crate::ports::PrNumber;
        use std::cell::RefCell;
        let (scene, gathered) = three_at_their_merge();
        // The record is read before the merge and gone after it, as a landed `slice-merge` leaves
        // it: only a number read first can be asked about.
        let records: RefCell<Vec<u64>> = RefCell::new(vec![3, 4, 5]);
        let asked = RefCell::new(Vec::new());
        let mut logged = Vec::new();
        let state_dir = tempfile::tempdir().expect("tempdir");
        tick(
            &scene.board,
            state_dir.path(),
            &gathered,
            now(),
            &Machine {
                merge_per_tick: 3,
                ..idle(2)
            },
            &Budget::default(),
            &mut Steps {
                run: &mut |task: TicketId, _: &ActionName| {
                    records.borrow_mut().retain(|kept| *kept != task.get());
                    0
                },
                launch: &mut |_: TicketId, _: &ActionName| Ok(()),
                start: &mut |_: &PlannedModel| Ok(()),
                pr_of: &mut |task: TicketId| {
                    records
                        .borrow()
                        .contains(&task.get())
                        .then(|| task.get().to_string().parse().expect("a number"))
                },
                merged: &mut |pr: PrNumber| {
                    asked.borrow_mut().push(pr.get());
                    true
                },
                branch_head: &mut || None,
                group: &mut |_| {},
                log: &mut |line: &str| logged.push(line.to_owned()),
            },
        );
        assert_eq!(
            *asked.borrow(),
            [3, 4, 5],
            "the forge is asked of each number"
        );
        for task in [3, 4, 5] {
            let prefix = format!("merge #{task} of merge: exit 0, merged, ");
            assert!(
                logged.iter().any(|line| line.starts_with(&prefix)),
                "#{task} is logged as merged: {logged:?}"
            );
        }
    }

    #[test]
    fn a_merge_whose_record_cannot_be_read_is_not_merged() {
        let (scene, gathered) = three_at_their_merge();
        let mut logged = Vec::new();
        let state_dir = tempfile::tempdir().expect("tempdir");
        tick(
            &scene.board,
            state_dir.path(),
            &gathered,
            now(),
            &Machine {
                merge_per_tick: 1,
                ..idle(2)
            },
            &Budget::default(),
            &mut Steps {
                run: &mut |_: TicketId, _: &ActionName| 0,
                launch: &mut |_: TicketId, _: &ActionName| Ok(()),
                start: &mut |_: &PlannedModel| Ok(()),
                pr_of: &mut |_: TicketId| None,
                merged: &mut |_: crate::ports::PrNumber| panic!("no number, no question"),
                branch_head: &mut || None,
                group: &mut |_| {},
                log: &mut |line: &str| logged.push(line.to_owned()),
            },
        );
        assert!(
            logged
                .iter()
                .any(|line| line.contains("exit 0, not merged, ")),
            "{logged:?}"
        );
    }

    #[test]
    fn per_tick_two_skips_the_third() {
        let (scene, gathered) = three_at_their_merge();
        let ((tick, ran), _) = run_group(&scene.board, &gathered, 2, &|_| 0, &|_| true);
        assert_eq!(ran, [3, 4]);
        assert_eq!(lines(&tick.held), ["#5 skipped: 2 merged in this tick"]);
    }

    #[test]
    fn a_refusal_ends_the_group() {
        let (scene, gathered) = three_at_their_merge();
        // The forge refuses #4: pr-merge exits 4, which merge passes on as 2.
        let ((tick, ran), logged) = run_group(
            &scene.board,
            &gathered,
            8,
            &|task| if task == 4 { 2 } else { 0 },
            &|task| task != 4,
        );
        assert_eq!(ran, [3, 4], "#5 never runs");
        assert_eq!(
            lines(&tick.held),
            ["#5 skipped: #3 merged in this tick, so its merge waits for the next one"]
        );
        assert!(
            logged
                .iter()
                .any(|line| line.starts_with("merge #4 of merge: exit 2, not merged, ")),
            "{logged:?}"
        );
    }

    #[test]
    fn a_landed_merge_with_board_done_failing_counts() {
        let (scene, gathered) = three_at_their_merge();
        // #3 merged, then `board done` failed: the action exits 2, the pull request is merged.
        let ((tick, ran), _) = run_group(
            &scene.board,
            &gathered,
            2,
            &|task| if task == 3 { 2 } else { 0 },
            &|_| true,
        );
        assert_eq!(ran, [3, 4], "#3 counts toward per_tick, so #5 waits");
        assert_eq!(lines(&tick.held), ["#5 skipped: 2 merged in this tick"]);
    }

    #[test]
    fn a_tick_runs_one_merge_and_leaves_the_others_for_the_next_tick() {
        let (scene, gathered) = three_at_their_merge();
        let (tick, ran) = run_exiting(&scene.board, &gathered, |_| 0);
        assert_eq!(ran, [3], "the base moved under #4 and #5");
        assert_eq!(tick.outcome.ran.len(), 1);
        assert_eq!(
            lines(&tick.held),
            [
                "#4 skipped: #3 merged in this tick, so its merge waits for the next one",
                "#5 skipped: #3 merged in this tick, so its merge waits for the next one",
            ]
        );
        assert!(tick.failed.is_empty(), "{tick:?}");
    }

    #[test]
    fn a_merge_that_failed_moved_nothing_so_the_next_one_runs() {
        let (scene, gathered) = three_at_their_merge();
        let (tick, ran) = run_exiting(&scene.board, &gathered, |task| u8::from(task == 3));
        assert_eq!(ran, [3, 4], "#3 did not merge, #4 did");
        assert_eq!(
            lines(&tick.held),
            ["#5 skipped: #4 merged in this tick, so its merge waits for the next one"]
        );
    }

    #[test]
    fn a_zaf_action_that_is_no_merge_runs_whatever_merged_before_it() {
        // `tidy` is a zaf action ending at its exit, listed before the merge.
        let scene = Scene::with_list(r#"["implement", "ci", "tidy", "merge"]"#);
        scene.action("implement", "model", "pr-open");
        scene.action("ci", "wait", "ci-done");
        scene.action("tidy", "zaf", "exit");
        scene.action("merge", "zaf", "merged");
        for task in [3, 4] {
            scene.task(task, "S");
            scene.open_pr(task, Some(CheckState::Passed));
        }
        // #3 has tidied at its head, so its merge is next; #4 has not.
        scene.phase_run_at(3, "tidy", 40, Some(0), "", "h1");
        let gathered = scene.gather();
        assert!(gathered.unreadable.is_empty(), "{:?}", gathered.unreadable);
        let (tick, ran) = run_exiting(&scene.board, &gathered, |_| 0);
        assert_eq!(ran, [3, 4]);
        assert_eq!(
            tick.outcome
                .ran
                .iter()
                .map(|ran| ran.action.as_str())
                .collect::<Vec<_>>(),
            ["merge", "tidy"]
        );
        assert!(tick.held.is_empty(), "{:?}", tick.held);
    }

    #[test]
    fn a_held_task_follows_the_fresh_one_in_the_same_tick() {
        let (scene, mut gathered) = three_at_their_merge();
        // #5 is fresh from a fix: #3 and #4 wait on its merge, and nothing else (F31).
        for facts in &mut gathered.tickets {
            facts.merging.first = (facts.ticket != id(5)).then(|| id(5));
        }
        let ((tick, ran), logged) = run_group(&scene.board, &gathered, 8, &|_| 0, &|_| true);
        assert_eq!(
            ran,
            [5, 3, 4],
            "#5 lands first, the two held behind it after"
        );
        assert!(tick.failed.is_empty(), "{tick:?}");
        assert!(
            logged
                .iter()
                .any(|line| line.starts_with("merges: 3 landed, 0 left for the next tick, in ")),
            "{logged:?}"
        );
        // The fresh one did not land: the hold stands, so nobody follows it.
        let ((_, ran), _) = run_group(&scene.board, &gathered, 8, &|_| 1, &|_| false);
        assert_eq!(ran, [5], "#3 and #4 wait while #5 stands");
    }

    #[test]
    fn a_task_with_another_wait_does_not_follow() {
        let (scene, mut gathered) = three_at_their_merge();
        let head = CommitId::from("b1");
        let pending = BaseFacts {
            head: Some(head.clone()),
            checks: vec![crate::domain::schedule::CheckFact {
                name: CheckName::from("ci"),
                result: crate::domain::schedule::CheckResult::Pending,
            }],
            reached_at: None,
        };
        // The base has a check pending: #3, the fresh one, was ruled through at that head; #4 waits
        // on #3's merge and on the base; #5 waits on the base alone.
        for facts in &mut gathered.tickets {
            facts.merging.base = pending.clone();
            facts.merging.first = (facts.ticket == id(4)).then(|| id(3));
            if facts.ticket == id(3) {
                facts.merging.ruled_at = Some(head.clone());
            }
        }
        let ((_, ran), _) = run_group(&scene.board, &gathered, 8, &|_| 0, &|_| true);
        assert_eq!(
            ran,
            [3],
            "#4 is held by R30 and by the base, so it is not added"
        );
    }

    #[test]
    fn the_gate_names_what_a_merge_waits_on() {
        let (_scene, mut gathered) = three_at_their_merge();
        let facts = &mut gathered.tickets[0];
        facts.merging.first = Some(id(12));
        facts.merging.base = BaseFacts {
            head: Some(CommitId::from("b1")),
            checks: vec![crate::domain::schedule::CheckFact {
                name: CheckName::from("ci"),
                result: crate::domain::schedule::CheckResult::Pending,
            }],
            reached_at: None,
        };
        let refusal = may_start(facts, &gathered.beside[&id(3)], &name("merge"), now())
            .expect_err("it waits");
        assert_eq!(
            refusal.reason(id(3), "merge"),
            "merge is not #3's current phase: merge waits: #12 is fresh from a fix and merges \
             first; the integration branch's checks at b1 have not ended"
        );
        // Neither is the epic's or a question's: the tick's block does not name it as skipped.
        assert!(held(&gathered, now()).is_empty());
    }
}
