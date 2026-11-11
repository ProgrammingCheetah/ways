use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::domain::TicketId;
use crate::fakes::{FakeForge, PrRequest};
use crate::ports::{
    Author, BaseName, Check, CheckName, CheckState, CommitId, Holder, PrNumber, ReviewComment,
    SliceName,
};

const TASK: u64 = 437;
/// The pid the scene's `alive` says is a live process.
const LIVE_PID: u32 = 4242;

fn task() -> TicketId {
    TicketId::from(TASK)
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn name(text: &str) -> ActionName {
    ActionName::from(text)
}

fn listed(action: &str, kind: ActionKind, ends: PhaseEnd, on_fail: Option<&str>) -> ListedPhase {
    ListedPhase {
        action: name(action),
        kind,
        ends,
        on_fail: on_fail.map(name),
    }
}

/// implement, open the pull request, wait for a review, wait for CI, merge.
fn list() -> Vec<ListedPhase> {
    vec![
        listed("implement", ActionKind::Model, PhaseEnd::Exit, None),
        listed("open-pr", ActionKind::Zaf, PhaseEnd::PrOpen, None),
        listed(
            "review",
            ActionKind::Wait,
            PhaseEnd::ReviewPosted,
            Some("fix"),
        ),
        listed("ci", ActionKind::Wait, PhaseEnd::CiDone, Some("fix")),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged, None),
    ]
}

/// A project on disk and a forge, holding one task.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    forge: FakeForge,
}

impl Scene {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("real path");
        Self {
            _dir: dir,
            root,
            forge: FakeForge::default(),
        }
    }

    fn gather(&self) -> Result<StandingFacts, StandingUnreadable> {
        gather_standing_facts(
            &self.forge,
            &self.root,
            task(),
            time(1),
            &|holder: &Holder| holder.pid == LIVE_PID,
        )
    }

    fn standing_of(&self, list: &[ListedPhase]) -> Standing {
        derive_standing(list, &self.gather().expect("facts"))
    }

    fn standing(&self) -> Standing {
        self.standing_of(&list())
    }

    /// The task's pull request, opened at `opened`, at head `head`, and recorded.
    fn open_pr(&self, head: &str, opened: u64) -> PrNumber {
        let pr = self
            .forge
            .open_pr(&PrRequest {
                slice: SliceName::from("slice"),
                base: BaseName::from("base"),
                title: "Title".to_owned(),
                body: String::new(),
            })
            .expect("open pr");
        self.forge.set_head(pr, Some(head)).expect("head");
        self.forge.set_opened_at(pr, time(opened)).expect("opened");
        write(
            &self.root.join(format!(".zaf/slices/{TASK}.pr")),
            &pr.get().to_string(),
        );
        pr
    }

    fn review(&self, pr: PrNumber, head: Option<&str>, at: u64) {
        self.forge
            .add_review_comment(
                pr,
                ReviewComment {
                    author: Author::from("reviewer"),
                    body: "Looks fine".to_owned(),
                    location: None,
                    head: head.map(CommitId::from),
                    posted_at: time(at),
                },
            )
            .expect("review");
    }

    fn check(&self, pr: PrNumber, state: CheckState, at: Option<u64>) {
        self.forge
            .set_checks(
                pr,
                vec![Check {
                    name: CheckName::from("ci"),
                    state,
                    finished_at: at.map(time),
                }],
            )
            .expect("checks");
    }

    /// A phase run folder named `dir`, for task `of`: its creation files, the task file last,
    /// stamped `started`. Its process is dead.
    fn run_of(&self, of: u64, dir: &str, action: &str, head: &str, started: u64) {
        let folder = self.root.join(".zaf/phases").join(dir);
        write(&folder.join("action"), action);
        write(&folder.join("head"), head);
        write(&folder.join("pid"), "999999 gone");
        write(&folder.join("task"), &of.to_string());
        stamp(&folder.join("task"), started);
    }

    fn run(&self, dir: &str, action: &str, head: &str, started: u64) {
        self.run_of(TASK, dir, action, head, started);
    }

    /// Records that the run `dir` ended with `code` at `at`.
    fn end(&self, dir: &str, code: &str, at: u64) {
        let file = self.root.join(".zaf/phases").join(dir).join("exit");
        write(&file, code);
        stamp(&file, at);
    }

    /// Makes the run `dir`'s process a live one.
    fn live(&self, dir: &str) {
        let file = self.root.join(".zaf/phases").join(dir).join("pid");
        write(&file, &format!("{LIVE_PID} boot-1"));
    }
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

/// Sets a file's modified time.
fn stamp(file: &Path, secs: u64) {
    fs::OpenOptions::new()
        .write(true)
        .open(file)
        .expect("open")
        .set_modified(time(secs))
        .expect("set modified");
}

/// A scene whose task has implemented (exit 0 at 10) and opened its pull request at 20, head `h1`.
fn opened() -> (Scene, PrNumber) {
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    scene.end("1", "0", 10);
    let pr = scene.open_pr("h1", 20);
    (scene, pr)
}

#[test]
fn green_checks_make_merge_ready() {
    let (scene, pr) = opened();
    scene.review(pr, Some("h1"), 30);
    scene.check(pr, CheckState::Passed, Some(40));
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("merge"),
            since: time(40)
        }
    );
}

#[test]
fn a_failed_check_is_failed() {
    let (scene, pr) = opened();
    scene.review(pr, Some("h1"), 30);
    scene.check(pr, CheckState::Failed, Some(40));
    assert_eq!(
        scene.standing(),
        Standing::Failed {
            action: name("ci"),
            on_fail: Some(name("fix")),
            since: time(40)
        }
    );
}

#[test]
fn a_failed_wait_without_on_fail_has_none() {
    let (scene, pr) = opened();
    scene.review(pr, Some("h1"), 30);
    scene.check(pr, CheckState::Failed, Some(40));
    let mut list = list();
    list[3].on_fail = None;
    assert_eq!(
        scene.standing_of(&list),
        Standing::Failed {
            action: name("ci"),
            on_fail: None,
            since: time(40)
        }
    );
}

#[test]
fn a_failed_run_of_a_wait_at_the_head_is_failed() {
    let (scene, _pr) = opened();
    scene.run("2", "review", "h1", 25);
    scene.end("2", "1", 26);
    assert_eq!(
        scene.standing(),
        Standing::Failed {
            action: name("review"),
            on_fail: Some(name("fix")),
            since: time(26)
        }
    );
}

#[test]
fn a_wait_not_run_at_head_is_ready() {
    let (scene, _pr) = opened();
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("review"),
            since: time(20)
        }
    );
    // A run at an older head is not a run at this one.
    scene.run("2", "review", "h0", 25);
    scene.end("2", "0", 26);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("review"),
            since: time(20)
        }
    );
}

#[test]
fn a_wait_run_at_head_is_waiting() {
    let (scene, _pr) = opened();
    scene.run("2", "review", "h1", 25);
    scene.end("2", "0", 26);
    assert_eq!(
        scene.standing(),
        Standing::Waiting {
            action: name("review")
        }
    );
}

#[test]
fn a_live_process_is_running() {
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    scene.live("1");
    assert_eq!(
        scene.standing(),
        Standing::Running {
            action: name("implement")
        }
    );
}

#[test]
fn an_exit_zero_ends_the_phase() {
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    scene.end("1", "1", 10);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
    scene.run("2", "implement", "", 11);
    scene.end("2", "0", 15);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("open-pr"),
            since: time(15)
        }
    );
}

#[test]
fn a_dead_run_without_exit_does_not_end_it() {
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
}

#[test]
fn an_exit_that_is_no_number_is_no_exit() {
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    scene.end("1", "lost", 10);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
}

#[test]
fn a_run_before_the_previous_fact_does_not_end_the_phase() {
    // The list asks for a second exit phase after the pull request opens.
    let list = vec![
        listed("open-pr", ActionKind::Zaf, PhaseEnd::PrOpen, None),
        listed("polish", ActionKind::Model, PhaseEnd::Exit, None),
    ];
    let (scene, _pr) = opened();
    // The run at 10 came before the pull request opened at 20.
    scene.run("2", "polish", "", 5);
    scene.end("2", "0", 10);
    assert_eq!(
        scene.standing_of(&list),
        Standing::Ready {
            action: name("polish"),
            since: time(20)
        }
    );
}

#[test]
fn a_new_head_reopens_the_review() {
    let (scene, pr) = opened();
    scene.review(pr, Some("h1"), 30);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("ci"),
            since: time(30)
        }
    );
    scene.forge.set_head(pr, Some("h2")).expect("head");
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("review"),
            since: time(20)
        }
    );
}

#[test]
fn a_review_with_no_head_is_not_at_the_head() {
    let (scene, pr) = opened();
    scene.review(pr, None, 30);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("review"),
            since: time(20)
        }
    );
}

#[test]
fn a_merged_pr_is_done() {
    let (scene, pr) = opened();
    scene.forge.set_merged_at(pr, time(50)).expect("merged at");
    scene.forge.merge(pr).expect("merge");
    assert_eq!(scene.standing(), Standing::Done);
}

#[test]
fn adjacent_model_phases_are_one_run() {
    let list = vec![
        listed("implement", ActionKind::Model, PhaseEnd::Exit, None),
        listed("document", ActionKind::Model, PhaseEnd::Exit, None),
        listed("open-pr", ActionKind::Zaf, PhaseEnd::PrOpen, None),
    ];
    let scene = Scene::new();
    assert_eq!(
        scene.standing_of(&list),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
    scene.run("1", "implement", "", 5);
    scene.end("1", "0", 10);
    assert_eq!(
        scene.standing_of(&list),
        Standing::Ready {
            action: name("open-pr"),
            since: time(10)
        }
    );
}

#[test]
fn a_wait_between_model_phases_splits_the_run() {
    let list = vec![
        listed("implement", ActionKind::Model, PhaseEnd::Exit, None),
        listed("review", ActionKind::Wait, PhaseEnd::ReviewPosted, None),
        listed("document", ActionKind::Model, PhaseEnd::Exit, None),
    ];
    let scene = Scene::new();
    scene.run("1", "implement", "", 5);
    scene.end("1", "0", 10);
    scene.open_pr("h1", 20);
    assert_eq!(
        scene.standing_of(&list),
        Standing::Ready {
            action: name("review"),
            since: time(10)
        }
    );
}

#[test]
fn an_unreadable_forge_gives_no_standing() {
    let (scene, _pr) = opened();
    scene.forge.fail_merged_reads();
    let error = scene.gather().expect_err("unreadable");
    assert_eq!(error.task, task());
    assert!(error.reason.contains("the forge is down"), "{error}");
    assert!(error.to_string().contains("437"), "{error}");
}

#[test]
fn a_recorded_pr_the_forge_does_not_know_is_unreadable() {
    let scene = Scene::new();
    write(&scene.root.join(format!(".zaf/slices/{TASK}.pr")), "99");
    assert!(scene.gather().is_err());
}

#[test]
fn a_finished_check_without_a_time_is_unreadable() {
    let (scene, pr) = opened();
    scene.check(pr, CheckState::Passed, None);
    assert!(scene.gather().is_err());
}

#[test]
fn another_tasks_run_is_not_this_ones() {
    let scene = Scene::new();
    scene.run_of(1, "1", "implement", "", 5);
    scene.live("1");
    scene.end("1", "0", 10);
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
}

#[test]
fn a_task_with_nothing_is_ready_for_its_first_phase() {
    let scene = Scene::new();
    assert_eq!(
        scene.standing(),
        Standing::Ready {
            action: name("implement"),
            since: time(1)
        }
    );
}

/// Every file under `dir` with its time and contents, and every folder.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, Option<String>)> {
    let mut seen = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = fs::metadata(&path).expect("metadata");
        if meta.is_dir() {
            for entry in fs::read_dir(&path).expect("read dir") {
                pending.push(entry.expect("entry").path());
            }
            seen.push((path, None, None));
        } else {
            let text = fs::read_to_string(&path).expect("text");
            seen.push((path, meta.modified().ok(), Some(text)));
        }
    }
    seen.sort();
    seen
}

#[test]
fn nothing_is_written_while_deriving() {
    let (scene, pr) = opened();
    scene.run("2", "review", "h1", 25);
    scene.review(pr, Some("h1"), 30);
    scene.check(pr, CheckState::Failed, Some(40));
    let before = snapshot(&scene.root);
    let facts = scene.gather().expect("facts");
    let _ = derive_standing(&list(), &facts);
    assert_eq!(snapshot(&scene.root), before);
}
