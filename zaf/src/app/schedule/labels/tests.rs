use std::cell::{Cell, RefCell};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::app::actions::ActionName;
use crate::app::schedule::gather::gather_listing as gather;
use crate::app::schedule::tick::{Machine, PhaseTick, Steps, tick};
use crate::app::spawn::PlannedModel;
use crate::app::spawn::phase::Ports;
use crate::domain::{Budget, Epic, Instance, Units};
use crate::fakes::{FakeBoard, FakeForge, PrRequest};
use crate::manifest::Manifest;
use crate::ports::{BaseName, ClaimOutcome, ClaimRecord, Holder, NewQuestion, SliceName};

const EPIC: u64 = 2;

const MANIFEST: &str = "[project]\nname = \"bookshelf\"\n\n[board]\nkind = \"github-issues\"\n\
    repo = \"example/bookshelf-board\"\n\n[spawn]\nslots = 2\n\n\
    [phases]\nTask = [\"implement\", \"merge\"]\n";

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// The time every sync and tick here runs at.
fn now() -> SystemTime {
    time(100)
}

/// A task body that follows the template, of size S, naming `blocked_by` as its blockers.
fn task_body(blocked_by: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
         Model: sonnet\nSize: S\nBlocked by: {blocked_by}\n"
    )
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

/// A project on disk whose `Task` list is implement (a model ending at an open pull request) then
/// merge (zaf, ending at the merge), its board with the challenged epic #2, and its forge.
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

    /// A task of the epic labelled `state` and blocked by `blocked_by`.
    fn task(&self, number: u64, state: TicketState, blocked_by: &str) {
        self.board.add_ticket(Ticket {
            id: id(number),
            epic: Some(id(EPIC)),
            title: format!("Issue {number}").into(),
            body: task_body(blocked_by).into(),
            state: Some(state),
            url: "".into(),
            kind: None,
        });
    }

    /// A ticket of the epic of type `Spike`, which has no phase list, labelled `state` and
    /// blocked by `blocked_by`.
    fn spike(&self, number: u64, state: TicketState, blocked_by: &str) {
        self.task(number, state, blocked_by);
        self.board.set_type(id(number), "Spike");
    }

    /// An open question raised on `on`.
    fn question(&self, number: u64, on: u64) {
        let asked = NewQuestion {
            on: id(on),
            text: "Which way?".to_owned(),
            door: false,
        };
        self.board.add_question(Ticket {
            id: id(number),
            epic: None,
            title: asked.title().into(),
            body: asked.body().into(),
            state: None,
            url: "".into(),
            kind: None,
        });
    }

    /// `task`'s pull request, merged at 40, and recorded.
    fn merged_pr(&self, task: u64) {
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
        self.forge.set_merged_at(pr, time(40)).expect("merged at");
        self.forge.merge(pr).expect("merges");
        write(
            &self.root.join(format!(".zaf/slices/{task}.pr")),
            &pr.get().to_string(),
        );
    }

    fn gather_on(&self, board: &dyn Board) -> Gathered {
        let ports = Ports {
            board,
            forge: &self.forge,
            ci_filter: None,
            manifest: &self.manifest,
            root: &self.root,
            home: &self.home,
            state_dir: &self.state_dir,
            alive: &|_: &Holder| false,
            version: None,
            now: SystemTime::now(),
            vcs: None,
        };
        gather(&ports).expect("the board reads")
    }

    fn gather(&self) -> Gathered {
        self.gather_on(&self.board)
    }

    fn label(&self, number: u64) -> Option<TicketState> {
        self.board.task(id(number)).expect("the task reads").state
    }
}

/// Runs the tick's phase steps on `gathered` on an idle machine, each model start recorded.
fn run_tick(board: &dyn Board, gathered: &Gathered) -> (PhaseTick, Vec<PlannedModel>) {
    let mut starts = Vec::new();
    let mut run = |_: TicketId, _: &ActionName| 0;
    let mut launch = |_: TicketId, _: &ActionName| Ok(());
    let mut start = |planned: &PlannedModel| {
        starts.push(planned.clone());
        Ok(())
    };
    let mut log = |_: &str| {};
    let machine = Machine {
        units: Units::default(),
        e_live: false,
        launches_here: 0,
        slots: 2,
        merge_per_tick: 1,
    };
    // No other tick meets this one: its questions are raised under a lock of its own.
    let state_dir = tempfile::tempdir().expect("tempdir");
    let done = tick(
        board,
        state_dir.path(),
        gathered,
        now(),
        &machine,
        &Budget::default(),
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
    (done, starts)
}

/// The tasks of the model starts.
fn started(starts: &[PlannedModel]) -> Vec<TicketId> {
    starts
        .iter()
        .filter_map(|planned| match planned {
            PlannedModel::Phase { task, .. } => Some(*task),
            _ => None,
        })
        .collect()
}

fn unblocked(numbers: &[(u64, &[u64])]) -> Vec<Unblocked> {
    numbers
        .iter()
        .map(|(task, blockers)| Unblocked {
            task: id(*task),
            blockers: blockers.iter().copied().map(id).collect(),
        })
        .collect()
}

#[test]
fn a_blocked_label_nothing_cleared_is_cleared_by_the_tick() {
    let scene = Scene::new();
    scene.task(5, TicketState::Ready, "—");
    scene
        .board
        .set_state(id(5), TicketState::Done)
        .expect("done");
    // #6 waited on #5, which is done now; nothing set it ready.
    scene.task(6, TicketState::Blocked, "#5");
    // #7 still waits on the open #8.
    scene.task(7, TicketState::Blocked, "#8");
    scene.task(8, TicketState::Claimed, "—");

    let labelling = sync(&scene.board, &scene.gather(), now());

    assert_eq!(labelling.failed, Vec::<String>::new());
    assert_eq!(labelling.unblocked, unblocked(&[(6, &[5])]));
    assert_eq!(labelling.relabelled, []);
    assert_eq!(labelling.lines(), ["#6 unblocked: #5 is done"]);
    assert_eq!(scene.label(6), Some(TicketState::Ready));
    assert_eq!(scene.label(7), Some(TicketState::Blocked));
    // Synced, the next tick finds nothing to write.
    assert_eq!(
        read(&scene.board, &scene.gather(), now()),
        Labelling::default()
    );
}

#[test]
fn a_question_label_with_no_open_question_is_cleared() {
    let scene = Scene::new();
    // #6 carries `question`, and no question is open on it or on its epic.
    scene.task(6, TicketState::Question, "—");
    // #7 carries it and one is open on it: it keeps it.
    scene.task(7, TicketState::Question, "—");
    scene.question(30, 7);
    // #8 waits on a question raised on it, labelled `ready` by hand.
    scene.task(8, TicketState::Ready, "—");
    scene.question(31, 8);

    let labelling = sync(&scene.board, &scene.gather(), now());

    assert_eq!(labelling.failed, Vec::<String>::new());
    assert_eq!(
        labelling.lines(),
        [
            "#6 labelled ready: no question is open on it",
            "#8 labelled question: it waits on question #31",
        ]
    );
    assert_eq!(scene.label(6), Some(TicketState::Ready));
    assert_eq!(scene.label(7), Some(TicketState::Question));
    assert_eq!(scene.label(8), Some(TicketState::Question));
    assert_eq!(
        labelling.relabelled_json(),
        serde_json::json!([
            {"task": 6, "from": "question", "to": "ready"},
            {"task": 8, "from": "ready", "to": "question"},
        ])
    );
}

#[test]
fn a_hand_set_blocked_label_does_not_hold_a_ticket() {
    let scene = Scene::new();
    // Nothing holds #6: it names no blocker and no question is open on it.
    scene.task(6, TicketState::Blocked, "—");
    let gathered = scene.gather();

    let labelling = sync(&scene.board, &gathered, now());
    let (_, starts) = run_tick(&scene.board, &gathered);

    assert_eq!(labelling.lines(), ["#6 unblocked: it names no blocker"]);
    assert_eq!(
        started(&starts),
        [id(6)],
        "the place starts it, not the label"
    );
    assert_eq!(scene.label(6), Some(TicketState::Ready));
}

#[test]
fn a_hand_set_ready_label_does_not_start_a_ticket_that_waits() {
    let scene = Scene::new();
    // #6 waits on the open #7 though it carries `ready`.
    scene.task(6, TicketState::Ready, "#7");
    scene.task(7, TicketState::Claimed, "—");
    let gathered = scene.gather();

    let labelling = sync(&scene.board, &gathered, now());
    let (_, starts) = run_tick(&scene.board, &gathered);

    assert_eq!(labelling.lines(), ["#6 labelled blocked: it waits on #7"]);
    assert!(!started(&starts).contains(&id(6)), "{starts:?}");
    assert_eq!(scene.label(6), Some(TicketState::Blocked));
}

#[test]
fn the_label_sync_never_touches_a_priority_label() {
    let scene = Scene::new();
    // #6 carries `question` with no question open, #8 waits on one and is labelled `ready`: the
    // sync relabels both. Each carries a priority the Engineer set.
    scene.task(6, TicketState::Question, "—");
    scene.task(8, TicketState::Ready, "—");
    scene.question(31, 8);
    scene.board.add_label(id(6), "priority/1");
    scene.board.add_label(id(8), "priority/3");

    let labelling = sync(&scene.board, &scene.gather(), now());

    assert_eq!(labelling.relabelled.len(), 2, "{labelling:?}");
    assert_eq!(scene.board.labels(id(6)), ["priority/1"]);
    assert_eq!(scene.board.labels(id(8)), ["priority/3"]);
    let priorities: Vec<Option<u8>> = scene
        .gather()
        .tickets
        .iter()
        .map(|facts| facts.priority.map(|priority| priority.level()))
        .collect();
    assert_eq!(priorities, [Some(1), Some(3)], "read back as set");
}

#[test]
fn the_sync_never_touches_a_claim() {
    let scene = Scene::new();
    let holder = Instance {
        role: "team".into(),
        epic: id(EPIC),
        number: 1.into(),
    };
    // Claimed: one in the lane, one waiting on the open #9, one waiting on a question.
    for task in [5, 6, 7] {
        let blocked_by = if task == 6 { "#9" } else { "—" };
        scene.task(task, TicketState::Ready, blocked_by);
        assert_eq!(
            scene.board.claim(id(task), &holder),
            Ok(ClaimOutcome::Claimed)
        );
    }
    scene.question(30, 7);
    scene.task(9, TicketState::Ready, "—");
    let claims = scene.board.claims().expect("claims read");

    let labelling = sync(&scene.board, &scene.gather(), now());

    // #9 is in the lane and ready: the sync never claims it either.
    assert_eq!(labelling, Labelling::default());
    for task in [5, 6, 7] {
        assert_eq!(scene.label(task), Some(TicketState::Claimed), "#{task}");
    }
    assert_eq!(scene.label(9), Some(TicketState::Ready));
    assert_eq!(scene.board.claims().expect("claims read"), claims);
}

#[test]
fn a_done_ticket_is_never_relabelled() {
    let scene = Scene::new();
    // #5's pull request merged and its label is not yet moved: the core places it done.
    scene.task(5, TicketState::Blocked, "—");
    scene.merged_pr(5);
    // #6 is labelled done on an issue still open: it is no open ticket.
    scene.task(6, TicketState::Ready, "#5");
    scene
        .board
        .set_state(id(6), TicketState::Done)
        .expect("done");
    scene.board.reopen(id(6));
    let gathered = scene.gather();
    assert!(gathered.tickets.iter().all(|facts| facts.ticket != id(6)));

    let labelling = sync(&scene.board, &gathered, now());

    assert_eq!(labelling, Labelling::default());
    assert_eq!(scene.label(5), Some(TicketState::Blocked));
    assert_eq!(scene.label(6), Some(TicketState::Done));
}

/// The fake board, with the failures and the counting a check needs.
struct Watched<'a> {
    board: &'a FakeBoard,
    /// `standing()` fails for this blocker.
    standing_fails: Option<TicketId>,
    /// `set_state()` fails for this task.
    set_fails: Option<TicketId>,
    /// Every blocker whose standing was read, in order.
    read: RefCell<Vec<TicketId>>,
    /// How many states were set.
    sets: Cell<u32>,
    /// How many times the epics were listed.
    epics_read: Cell<u32>,
}

/// `board`, watched, failing nothing.
fn watched(board: &FakeBoard) -> Watched<'_> {
    Watched {
        board,
        standing_fails: None,
        set_fails: None,
        read: RefCell::new(Vec::new()),
        sets: Cell::new(0),
        epics_read: Cell::new(0),
    }
}

impl Board for Watched<'_> {
    fn standing(&self, number: TicketId) -> Result<Standing, PortError> {
        self.read.borrow_mut().push(number);
        if self.standing_fails == Some(number) {
            return Err(PortError::Unavailable("the board is down".to_owned()));
        }
        self.board.standing(number)
    }
    fn epics(&self) -> Result<Vec<Epic>, PortError> {
        self.epics_read.set(self.epics_read.get() + 1);
        self.board.epics()
    }
    fn open_tickets(&self) -> Result<Vec<crate::domain::OpenTicket>, PortError> {
        self.board.open_tickets()
    }
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        self.board.ready()
    }
    fn task(&self, number: TicketId) -> Result<Ticket, PortError> {
        self.board.task(number)
    }
    fn claim(&self, task: TicketId, instance: &Instance) -> Result<ClaimOutcome, PortError> {
        self.board.claim(task, instance)
    }
    fn heartbeat(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.board.heartbeat(task, instance)
    }
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        self.board.claims()
    }
    fn release(&self, task: TicketId, instance: &Instance) -> Result<(), PortError> {
        self.board.release(task, instance)
    }
    fn set_state(&self, task: TicketId, label: TicketState) -> Result<(), PortError> {
        if self.set_fails == Some(task) {
            return Err(PortError::Rejected(format!("{task} is locked")));
        }
        self.sets.set(self.sets.get() + 1);
        self.board.set_state(task, label)
    }
    fn comment(&self, issue: TicketId, body: &str) -> Result<(), PortError> {
        self.board.comment(issue, body)
    }
    fn report(&self, task: TicketId, instance: &Instance, body: &str) -> Result<(), PortError> {
        self.board.report(task, instance, body)
    }
    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        self.board.questions()
    }
    fn questions_on(&self, task: TicketId) -> Result<Vec<(Ticket, Option<String>)>, PortError> {
        self.board.questions_on(task)
    }
    fn open_question(&self, question: &NewQuestion) -> Result<TicketId, PortError> {
        self.board.open_question(question)
    }
    fn record_ruling(
        &self,
        question: TicketId,
        ruling: &str,
    ) -> Result<Option<TicketId>, PortError> {
        self.board.record_ruling(question, ruling)
    }
}

#[test]
fn a_label_that_cannot_be_written_is_named_and_the_tick_goes_on() {
    let scene = Scene::new();
    scene.task(6, TicketState::Blocked, "—");
    scene.task(7, TicketState::Ready, "#8");
    // A spike has no phase list, so the tick starts nothing for it.
    scene.spike(8, TicketState::Claimed, "—");
    let watched = Watched {
        set_fails: Some(id(6)),
        ..watched(&scene.board)
    };
    let gathered = scene.gather_on(&watched);

    let labelling = sync(&watched, &gathered, now());
    let (_, starts) = run_tick(&watched, &gathered);

    assert_eq!(
        labelling.failed,
        ["cannot unblock #6: rejected: #6 is locked"]
    );
    assert_eq!(labelling.unblocked, []);
    // The other label is still written, and the tick still starts #6, which nothing holds.
    assert_eq!(labelling.lines(), ["#7 labelled blocked: it waits on #8"]);
    assert_eq!(scene.label(7), Some(TicketState::Blocked));
    assert_eq!(started(&starts), [id(6)]);
    assert_eq!(watched.sets.get(), 1, "the one that was written");
}

/// Epic #2 with the done #25 (labelled so) and #24 (closed as completed), the open #23, the
/// dropped #22 (closed as not planned) and these blocked spikes, which have no phase list: #26 by
/// #25, #27 by #24 and #25, #28 by #25 and the open #23, #29 naming none, #30 by the dropped
/// #22, #31 by #99 (no such issue).
fn spikes(scene: &Scene) {
    scene.spike(25, TicketState::Claimed, "—");
    scene
        .board
        .set_state(id(25), TicketState::Done)
        .expect("done");
    scene.spike(24, TicketState::Claimed, "—");
    scene.board.close(id(24));
    scene.spike(23, TicketState::Claimed, "—");
    scene.spike(22, TicketState::Ready, "—");
    scene.board.close_not_planned(id(22));
    scene.spike(26, TicketState::Blocked, "#25");
    scene.spike(27, TicketState::Blocked, "#25, #24");
    scene.spike(28, TicketState::Blocked, "#25 #23");
    scene.spike(29, TicketState::Blocked, "—");
    scene.spike(30, TicketState::Blocked, "#22");
    scene.spike(31, TicketState::Blocked, "#99");
}

#[test]
fn a_ticket_with_no_phase_list_is_labelled_from_its_questions_and_its_blockers() {
    let scene = Scene::new();
    scene.board.add_challenged_epic(id(3), "Issue 3");
    // Under #2, where a question is open on the epic: #32 carries `ready`, #34 `blocked`.
    scene.question(40, EPIC);
    scene.spike(32, TicketState::Ready, "—");
    scene.spike(34, TicketState::Blocked, "—");
    // Under #3: #33 carries `question`, none is open on it or its epic, and it names the open #23.
    scene.spike(23, TicketState::Claimed, "—");
    scene.spike(33, TicketState::Question, "#23");
    for spike in [23, 33] {
        scene.board.add_ticket(Ticket {
            epic: Some(id(3)),
            ..scene.board.task(id(spike)).expect("reads")
        });
    }
    let gathered = scene.gather();
    assert!(gathered.tickets.is_empty(), "a spike has no phase list");

    let labelling = sync(&scene.board, &gathered, now());

    assert_eq!(labelling.failed, Vec::<String>::new());
    assert_eq!(
        labelling.lines(),
        [
            "#32 labelled question: it waits on question #40",
            "#33 labelled blocked: it waits on #23",
        ]
    );
    // A blocked ticket keeps `blocked` while it waits on a question.
    assert_eq!(scene.label(34), Some(TicketState::Blocked));
}

#[test]
fn the_lines_and_the_json_name_each_task_and_its_done_blockers() {
    let scene = Scene::new();
    spikes(&scene);

    let labelling = sync(&scene.board, &scene.gather(), now());

    assert_eq!(
        labelling.lines(),
        [
            "#26 unblocked: #25 is done",
            "#27 unblocked: #24 and #25 are done",
            "#29 unblocked: it names no blocker",
        ]
    );
    assert_eq!(
        labelling.to_json(),
        serde_json::json!([
            { "task": 26, "blockers": [25] },
            { "task": 27, "blockers": [24, 25] },
            { "task": 29, "blockers": [] },
        ])
    );
    // An open or dead blocker keeps its task blocked: #28 waits on the open #23; #30 names a
    // dropped task and #31 no issue at all, neither of which closes.
    for blocked in [28, 30, 31] {
        assert_eq!(
            scene.label(blocked),
            Some(TicketState::Blocked),
            "#{blocked}"
        );
    }
    let four = Unblocked {
        task: id(9),
        blockers: [5, 6, 7, 8].into_iter().map(id).collect(),
    };
    assert_eq!(four.line(), "#9 unblocked: #5, #6, #7 and #8 are done");
}

#[test]
fn the_read_alone_names_the_same_labels_and_writes_nothing() {
    let read_only = Scene::new();
    spikes(&read_only);
    read_only.task(40, TicketState::Ready, "#23");
    let written = Scene::new();
    spikes(&written);
    written.task(40, TicketState::Ready, "#23");

    let found = read(&read_only.board, &read_only.gather(), now());

    assert_eq!(found, sync(&written.board, &written.gather(), now()));
    assert_eq!(found.relabelled.len(), 1, "{found:?}");
    for still in [26, 27, 29] {
        assert_eq!(
            read_only.label(still),
            Some(TicketState::Blocked),
            "#{still}"
        );
    }
    assert_eq!(read_only.label(40), Some(TicketState::Ready));
}

/// The open epic #70 with its done task #71 (closed at 50), and the blocked spike #60 of epic #2
/// naming it; a spike has no phase list, so the labels are what reads it.
fn finished_epic_blocking_a_spike(scene: &Scene) {
    scene.board.add_challenged_epic(id(70), "Other epic");
    scene.board.add_ticket(Ticket {
        id: id(71),
        epic: Some(id(70)),
        title: "Built".into(),
        body: task_body("—").into(),
        state: Some(TicketState::Done),
        url: "".into(),
        kind: None,
    });
    scene.board.close_at(id(71), time(50));
    scene.spike(60, TicketState::Blocked, "#70");
}

#[test]
fn labels_unblock_ticket_of_finished_epic() {
    let scene = Scene::new();
    finished_epic_blocking_a_spike(&scene);
    let watched = watched(&scene.board);
    let gathered = scene.gather_on(&watched);

    let labelling = sync(&watched, &gathered, now());

    assert_eq!(labelling.lines(), ["#60 unblocked: #70 is done"]);
    assert_eq!(scene.label(60), Some(TicketState::Ready));
    // The epic stays open and no label is written to it.
    assert_eq!(scene.board.closed_as(id(70)), None);
    assert_eq!(scene.label(70), None);
    assert_eq!(watched.sets.get(), 1, "only #60's label");
    assert_eq!(
        watched.epics_read.get(),
        1,
        "the epics are the gather's one read: the labels add no call"
    );
}

#[test]
fn labels_keep_a_ticket_of_an_all_dropped_epic_blocked() {
    let scene = Scene::new();
    finished_epic_blocking_a_spike(&scene);
    // Dropped: closed as not planned, and no longer labelled `done`.
    scene
        .board
        .set_state(id(71), TicketState::Claimed)
        .expect("labelled");
    scene.board.close_not_planned(id(71));
    let gathered = scene.gather();

    let labelling = sync(&scene.board, &gathered, now());

    assert_eq!(labelling.lines(), Vec::<String>::new());
    assert_eq!(scene.label(60), Some(TicketState::Blocked));
}

#[test]
fn each_blocker_is_read_once_a_tick() {
    let scene = Scene::new();
    spikes(&scene);
    let watched = watched(&scene.board);
    let gathered = scene.gather();

    sync(&watched, &gathered, now());

    let read = watched.read.borrow();
    let mut distinct = read.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(read.len(), distinct.len(), "read twice: {read:?}");
    // #25 is named by three tasks and read once.
    assert_eq!(read.iter().filter(|&&blocker| blocker == id(25)).count(), 1);
}

#[test]
fn a_blocker_that_cannot_be_read_holds_its_task_and_the_others_still_go() {
    let scene = Scene::new();
    spikes(&scene);
    let watched = Watched {
        standing_fails: Some(id(24)),
        ..watched(&scene.board)
    };
    let gathered = scene.gather();

    let labelling = sync(&watched, &gathered, now());

    // #27 names #24, which cannot be read: it is named and stays blocked; #26 and #29 still go.
    assert_eq!(labelling.unblocked, unblocked(&[(26, &[25]), (29, &[])]));
    assert_eq!(
        labelling.failed,
        ["cannot unblock #27: unavailable: the board is down"]
    );
    assert_eq!(scene.label(27), Some(TicketState::Blocked));
}
