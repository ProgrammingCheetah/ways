use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::gather_listing as gather;
use super::*;
use crate::app::schedule::ask::{base_deadline_marker, no_check_marker};
use crate::domain::PullRequestFilter;
use crate::domain::schedule::{HeadEnd, PhaseEnd, Place, WaitingOn, place};
use crate::domain::{CheckName, CommitId, Instance, InstanceNumber, TaskSize, TicketState};
use crate::fakes::{FakeBoard, FakeCiFilter, FakeForge, PrRequest};
use crate::manifest::Manifest;
use crate::ports::{BaseName, Check, CiFilter, Holder, NewQuestion, SliceName};

const MANIFEST: &str = r#"
[project]
name = "bookshelf"

[board]
kind = "github-issues"
repo = "example/bookshelf-board"

[phases]
Task = ["implement", "ci", "merge"]
"#;

const EPIC: u64 = 2;

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// A task body that follows the template, of size S, naming `blocked_by` as its blockers.
fn task_body(blocked_by: &str) -> String {
    format!(
        "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
         Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
         Model: sonnet\nSize: S\nBlocked by: {blocked_by}\n"
    )
}

/// A project on disk with the pack's actions implement (a model ending at an open pull
/// request), ci (a wait ending when CI is done) and merge (zaf, ending at the merge), its board
/// with a signed and challenged epic, and its forge.
struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state_dir: PathBuf,
    board: FakeBoard,
    forge: FakeForge,
    /// The workflows of the forge, none for a scene that has no filter port: then every check is
    /// expected.
    ci_filter: Option<FakeCiFilter>,
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
            ci_filter: None,
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

    /// A task under `epic` in `state`, blocked by `blocked_by`.
    fn task(&self, number: u64, epic: u64, state: TicketState, blocked_by: &str) {
        self.board.add_ticket(Ticket {
            id: id(number),
            epic: Some(id(epic)),
            title: format!("Issue {number}").into(),
            body: task_body(blocked_by).into(),
            state: Some(state),
            url: "".into(),
            kind: None,
        });
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

    /// `task`'s pull request at head `h1`, opened at 20 and recorded, with one check `ci`.
    fn open_pr(&self, task: u64, check: CheckState) -> PrNumber {
        let pr = self
            .forge
            .open_pr(&PrRequest {
                slice: SliceName::from(format!("slice-{task}")),
                base: BaseName::from("base"),
                title: "Title".to_owned(),
                body: String::new(),
            })
            .expect("open pr");
        self.forge.set_head(pr, Some("h1")).expect("head");
        self.forge.set_opened_at(pr, time(20)).expect("opened");
        let finished_at = (check != CheckState::Pending).then(|| time(30));
        self.forge
            .set_checks(
                pr,
                vec![Check {
                    name: CheckName::from("ci"),
                    state: check,
                    finished_at,
                }],
            )
            .expect("checks");
        self.record_pr(task, &pr.get().to_string());
        pr
    }

    fn record_pr(&self, task: u64, text: &str) {
        write(&self.root.join(format!(".zaf/slices/{task}.pr")), text);
    }

    fn gather(&self) -> Gathered {
        self.with_ports(None, |ports| gather(ports).expect("the board reads"))
    }

    /// The gather of a tick for `version`, which names the integration branch `feature/<version>`.
    fn gather_for_version(&self, version: &str) -> Gathered {
        self.with_ports(Some(version), |ports| {
            gather(ports).expect("the board reads")
        })
    }

    /// [`Scene::with_ports_at`] at a time when a pull request opened at 20 is old enough for a
    /// no-check question to exist.
    fn with_ports<T>(&self, version: Option<&str>, read: impl FnOnce(&Ports<'_>) -> T) -> T {
        self.with_ports_at(version, time(20) + NO_CHECK_DEADLINE, read)
    }

    fn with_ports_at<T>(
        &self,
        version: Option<&str>,
        now: SystemTime,
        read: impl FnOnce(&Ports<'_>) -> T,
    ) -> T {
        read(&Ports {
            board: &self.board,
            forge: &self.forge,
            ci_filter: self
                .ci_filter
                .as_ref()
                .map(|filter| filter as &dyn CiFilter),
            manifest: &self.manifest,
            root: &self.root,
            home: &self.home,
            state_dir: &self.state_dir,
            alive: &|_: &Holder| false,
            version,
            now,
            vcs: None,
        })
    }

    /// `task`'s pull request ([`Scene::open_pr`]) changing a file, so its implement has ended.
    fn open_changing_pr(&self, task: u64, check: CheckState) -> PrNumber {
        let pr = self.open_pr(task, check);
        self.forge
            .set_changed_files(pr, &["src/lib.rs"])
            .expect("files");
        pr
    }

    /// The ci wait failing to `fix`, and `task` brought to its head `h1` by a run of `fix` that
    /// exited 0.
    fn fixed(&self, task: u64) {
        let file = self.root.join(".zaf/actions/ci");
        write(
            &file,
            "#!/bin/sh\n# zaf description: ci\n# zaf 1: failed\n# zaf kind: wait\n\
             # zaf ends: ci-done\n# zaf on-fail: fix\nexit 0\n",
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");
        self.action("fix", "model", "exit");
        let action = ActionName::from("fix");
        let instance = Instance {
            role: "fix".into(),
            epic: id(EPIC),
            number: InstanceNumber::from(1),
        };
        let old = CommitId::from("old");
        let run = record::open(
            &self.state_dir.join(record::PHASES_DIR),
            &record::Opening {
                task: id(task),
                action: &action,
                head: Some(&old),
                size: TaskSize::S,
                instance: &instance,
                inline: None,
            },
        )
        .expect("opened");
        record::end(&run, 0, &HeadEnd::Left(Some(CommitId::from("h1"))), &|| {}).expect("ended");
    }

    /// The facts gathered for `task`.
    fn facts_of(&self, task: u64) -> TicketFacts {
        self.gather()
            .tickets
            .into_iter()
            .find(|facts| facts.ticket == id(task))
            .expect("the task is gathered")
    }
}

fn write(file: &Path, text: &str) {
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

fn listed(action: &str, kind: ActionKind, ends: PhaseEnd, on_fail: Option<&str>) -> ListedPhase {
    ListedPhase {
        action: ActionName::from(action),
        kind,
        ends,
        on_fail: on_fail.map(ActionName::from),
    }
}

#[test]
fn every_open_task_with_a_phase_list_is_gathered_whatever_its_state_and_no_done_one() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.task(5, EPIC, TicketState::Blocked, "—");
    scene.task(6, EPIC, TicketState::Question, "—");
    scene.task(8, EPIC, TicketState::Done, "—");
    let gathered = scene.gather();
    let tickets: Vec<TicketId> = gathered.tickets.iter().map(|facts| facts.ticket).collect();
    assert_eq!(tickets, [id(5), id(6), id(7)]);
    assert!(gathered.unreadable.is_empty());
    let facts = &gathered.tickets[2];
    assert_eq!(facts.size, TaskSize::S);
    assert_eq!(
        facts.phases,
        [
            Phase {
                name: ActionName::from("implement"),
                ends: vec![PhaseEnd::PrOpen],
                on_fail: None,
            },
            Phase {
                name: ActionName::from("ci"),
                ends: vec![PhaseEnd::CiDone],
                on_fail: None,
            },
            Phase {
                name: ActionName::from("merge"),
                ends: vec![PhaseEnd::Merged],
                on_fail: None,
            },
        ]
    );
    assert_eq!(
        facts.epic,
        Some(EpicFacts {
            id: id(EPIC),
            signed: true,
            challenged: true,
        })
    );
    assert_eq!(facts.pull, None);
    assert!(facts.runs.is_empty());
}

#[test]
fn a_ticket_whose_type_has_no_phase_list_is_passed_over() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.task(8, EPIC, TicketState::Ready, "—");
    scene.board.set_type(id(8), "Spike");
    let gathered = scene.gather();
    let tickets: Vec<TicketId> = gathered.tickets.iter().map(|facts| facts.ticket).collect();
    assert_eq!(tickets, [id(7)]);
    assert!(
        gathered.unreadable.is_empty(),
        "passed over, not unreadable"
    );
}

#[test]
fn an_unsigned_epic_is_read_and_holds_its_ticket() {
    let scene = Scene::new();
    scene.board.add_epic(id(3), "Issue 3");
    scene.task(7, 3, TicketState::Ready, "—");
    let facts = scene.facts_of(7);
    assert_eq!(
        facts.epic,
        Some(EpicFacts {
            id: id(3),
            signed: false,
            challenged: false,
        })
    );
    assert_eq!(
        place(&facts, time(100)),
        Place::Waiting([WaitingOn::Epic(id(3))].into())
    );
}

#[test]
fn a_question_open_on_the_epic_is_open_on_its_tickets() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.task(8, EPIC, TicketState::Question, "—");
    scene.question(30, EPIC);
    scene.question(31, 8);
    assert_eq!(scene.facts_of(7).open_questions, [id(30)].into());
    let facts = scene.facts_of(8);
    assert_eq!(facts.open_questions, [id(30), id(31)].into());
    assert_eq!(
        place(&facts, time(100)),
        Place::Waiting([WaitingOn::Question(id(30)), WaitingOn::Question(id(31))].into())
    );
}

#[test]
fn a_dependency_not_done_is_read_with_where_it_stands() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "#25, #26, #27");
    scene.task(25, EPIC, TicketState::Ready, "—");
    scene.task(26, EPIC, TicketState::Done, "—");
    let facts = scene.facts_of(7);
    assert_eq!(
        facts.dependencies,
        [
            Dependency {
                ticket: id(25),
                standing: Standing::Open,
            },
            Dependency {
                ticket: id(26),
                standing: Standing::Done,
            },
            Dependency {
                ticket: id(27),
                standing: Standing::Missing,
            },
        ]
    );
    let Place::Waiting(waits) = place(&facts, time(100)) else {
        panic!("a ticket with a dependency not done waits");
    };
    assert!(waits.contains(&WaitingOn::Ticket(id(25))), "{waits:?}");
    assert!(!waits.contains(&WaitingOn::Ticket(id(26))), "{waits:?}");
}

/// Epic #50, open and unlabelled, with these tasks `(number, label, closed as not planned,
/// closed at)`, and #7 of epic #2 blocked by it.
fn epic_blocking_seven(scene: &Scene, tasks: &[(u64, TicketState, bool, Option<u64>)]) {
    scene.board.add_challenged_epic(id(50), "Other epic");
    for &(number, state, dropped, closed_at) in tasks {
        scene.task(number, 50, state, "—");
        if dropped {
            scene.board.close_not_planned(id(number));
        } else if let Some(secs) = closed_at {
            scene.board.close_at(id(number), time(secs));
        }
    }
    scene.task(7, EPIC, TicketState::Ready, "#50");
    scene.board.set_opened_at(id(7), time(1000));
}

fn waits_on_epic(facts: &TicketFacts) -> bool {
    matches!(
        place(facts, time(100_000)),
        Place::Waiting(waits) if waits.contains(&WaitingOn::Ticket(id(50)))
    )
}

#[test]
fn scheduler_unblocks_ticket_of_finished_epic() {
    let scene = Scene::new();
    epic_blocking_seven(
        &scene,
        &[
            (51, TicketState::Done, false, Some(3000)),
            (52, TicketState::Done, false, Some(2000)),
            (53, TicketState::Ready, true, None),
        ],
    );

    let facts = scene.facts_of(7);

    assert_eq!(
        facts.dependencies,
        [Dependency {
            ticket: id(50),
            standing: Standing::Done,
        }]
    );
    assert!(!waits_on_epic(&facts));
    assert_eq!(
        facts.ready_since,
        Some(time(3000)),
        "from the last task of the epic to close"
    );
    assert_eq!(
        scene.board.closed_as(id(50)),
        None,
        "the epic stays open, and no label is written to it"
    );
}

#[test]
fn all_dropped_epic_keeps_ticket_waiting() {
    let scene = Scene::new();
    epic_blocking_seven(
        &scene,
        &[
            (51, TicketState::Ready, true, None),
            (52, TicketState::Ready, true, None),
        ],
    );

    let facts = scene.facts_of(7);

    assert_eq!(facts.dependencies[0].standing, Standing::Open);
    assert!(waits_on_epic(&facts));
}

#[test]
fn an_epic_with_a_task_open_keeps_its_blocked_ticket_waiting() {
    let scene = Scene::new();
    epic_blocking_seven(
        &scene,
        &[
            (51, TicketState::Done, false, Some(3000)),
            (52, TicketState::Ready, false, None),
        ],
    );

    assert!(waits_on_epic(&scene.facts_of(7)));
}

#[test]
fn read_pull_carries_mergeable() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let pr = scene.open_pr(7, CheckState::Pending);
    let mergeable = |scene: &Scene| scene.facts_of(7).pull.expect("read").mergeable;
    assert_eq!(mergeable(&scene), None);
    scene.forge.set_mergeable(pr, Some(true)).expect("true");
    assert_eq!(mergeable(&scene), Some(true));
    scene.forge.set_mergeable(pr, Some(false)).expect("false");
    assert_eq!(mergeable(&scene), Some(false));
}

#[test]
fn a_pull_request_with_a_pending_check_is_read_at_its_head() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let pr = scene.open_pr(7, CheckState::Pending);
    let facts = scene.facts_of(7);
    let pull = facts.pull.clone().expect("the pull request is read");
    assert_eq!(pull.head, Some(CommitId::from("h1")));
    assert_eq!(pull.opened_at, time(20));
    assert_eq!(pull.merged_at, None);
    assert_eq!(
        pull.checks,
        [CheckFact {
            name: CheckName::from("ci"),
            result: CheckResult::Pending,
        }]
    );
    assert!(!pull.changes_a_file(), "the forge lists no changed file");
    scene
        .forge
        .set_changed_files(pr, &["src/lib.rs"])
        .expect("files");
    let facts = scene.facts_of(7);
    assert!(facts.pull.as_ref().is_some_and(PullFacts::changes_a_file));
    assert_eq!(
        place(&facts, time(100)),
        Place::Waiting([WaitingOn::Check(id(7), CommitId::from("h1"))].into())
    );
}

#[test]
fn gather_keeps_the_changed_file_the_forge_names() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let pr = scene.open_pr(7, CheckState::Pending);
    scene
        .forge
        .set_changed_files(pr, &["src/a.rs", "src/lib.rs"])
        .expect("files");
    let named = scene.forge.changed_files(pr).expect("forge reads");
    let pull = scene.facts_of(7).pull.expect("the pull request is read");
    assert_eq!(pull.changed_files, named);
    assert!(pull.changes_a_file());
}

#[test]
fn gather_keeps_no_changed_file_when_the_forge_names_none() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_pr(7, CheckState::Pending);
    let pull = scene.facts_of(7).pull.expect("the pull request is read");
    assert!(!pull.changes_a_file());
}

#[test]
fn adjacent_model_actions_are_folded_into_one_phase_with_each_end() {
    let list = [
        listed("implement", ActionKind::Model, PhaseEnd::PrOpen, None),
        listed(
            "document",
            ActionKind::Model,
            PhaseEnd::Exit,
            Some("ignored"),
        ),
        listed("ci", ActionKind::Wait, PhaseEnd::CiDone, Some("fix")),
        listed("review", ActionKind::Model, PhaseEnd::Exit, Some("fix")),
        listed("merge", ActionKind::Zaf, PhaseEnd::Merged, None),
    ];
    assert_eq!(
        phases_of(&list),
        [
            Phase {
                name: ActionName::from("implement"),
                ends: vec![PhaseEnd::PrOpen, PhaseEnd::Exit],
                on_fail: None,
            },
            Phase {
                name: ActionName::from("ci"),
                ends: vec![PhaseEnd::CiDone],
                on_fail: Some(ActionName::from("fix")),
            },
            Phase {
                name: ActionName::from("review"),
                ends: vec![PhaseEnd::Exit],
                on_fail: Some(ActionName::from("fix")),
            },
            Phase {
                name: ActionName::from("merge"),
                ends: vec![PhaseEnd::Merged],
                on_fail: None,
            },
        ]
    );
}

#[test]
fn a_folded_pair_of_model_actions_is_gathered_as_one_phase() {
    let mut scene = Scene::new();
    scene.manifest = MANIFEST
        .replace(
            r#"Task = ["implement", "ci", "merge"]"#,
            r#"Task = ["implement", "document", "ci", "merge"]"#,
        )
        .parse()
        .expect("manifest parses");
    scene.action("document", "model", "exit");
    scene.task(7, EPIC, TicketState::Ready, "—");
    let names: Vec<(String, Vec<PhaseEnd>)> = scene
        .facts_of(7)
        .phases
        .into_iter()
        .map(|phase| (phase.name.to_string(), phase.ends))
        .collect();
    assert_eq!(
        names,
        [
            (
                "implement".to_owned(),
                vec![PhaseEnd::PrOpen, PhaseEnd::Exit]
            ),
            ("ci".to_owned(), vec![PhaseEnd::CiDone]),
            ("merge".to_owned(), vec![PhaseEnd::Merged]),
        ]
    );
}

#[test]
fn the_runs_are_read_from_the_record_with_the_head_they_left() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let action = ActionName::from("implement");
    let instance = Instance {
        role: "implement".into(),
        epic: id(EPIC),
        number: InstanceNumber::from(1),
    };
    let run = record::open(
        &scene.state_dir.join(record::PHASES_DIR),
        &record::Opening {
            task: id(7),
            action: &action,
            head: None,
            size: TaskSize::S,
            instance: &instance,
            inline: None,
        },
    )
    .expect("opened");
    record::end(&run, 0, &HeadEnd::Left(Some(CommitId::from("h1"))), &|| {}).expect("ended");
    let facts = scene.facts_of(7);
    assert_eq!(facts.runs.len(), 1);
    assert_eq!(facts.runs[0].action, action);
    assert_eq!(
        facts.runs[0].head_end,
        HeadEnd::Left(Some(CommitId::from("h1")))
    );
}

#[test]
fn a_ruled_question_marks_the_last_run_it_counted_once_the_runs_are_enough_to_count() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let action = ActionName::from("implement");
    let phases = scene.state_dir.join(record::PHASES_DIR);
    for number in 1..=3 {
        let instance = Instance {
            role: "implement".into(),
            epic: id(EPIC),
            number: InstanceNumber::from(number),
        };
        let run = record::open(
            &phases,
            &record::Opening {
                task: id(7),
                action: &action,
                head: None,
                size: TaskSize::S,
                instance: &instance,
                inline: None,
            },
        )
        .expect("opened");
        record::end(&run, 1, &HeadEnd::Left(None), &|| {}).expect("ended");
    }
    let asked = NewQuestion {
        on: id(7),
        text: "implement failed 3 times in a row with exit codes 1, 1, 1; the last line of its \
               log: none was recorded. It is not started again until this is ruled. \
               Last run: 1500000000000000001."
            .to_owned(),
        door: false,
    };
    let question = scene.board.open_question(&asked).expect("asked");
    scene
        .board
        .record_ruling(question, "try again")
        .expect("ruled");
    let facts = scene.facts_of(7);
    assert_eq!(
        facts.ruled,
        [RuledMarker {
            action,
            last_run: UNIX_EPOCH + Duration::from_nanos(1_500_000_000_000_000_001),
        }]
    );
}

#[test]
fn a_ticket_that_cannot_be_read_is_named_and_the_others_still_gathered() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.task(8, EPIC, TicketState::Ready, "—");
    // A pull request the forge does not know.
    scene.record_pr(7, "12");
    let gathered = scene.gather();
    let tickets: Vec<TicketId> = gathered.tickets.iter().map(|facts| facts.ticket).collect();
    assert_eq!(tickets, [id(8)]);
    assert_eq!(gathered.unreadable.len(), 1);
    assert_eq!(gathered.unreadable[0].ticket, id(7));
    assert!(
        gathered.unreadable[0].reason.contains("pull request #12"),
        "{:?}",
        gathered.unreadable[0].reason
    );
}

#[test]
fn an_unreadable_forge_makes_the_ticket_unreadable() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.open_pr(7, CheckState::Passed);
    scene.forge.fail_merged_reads();
    let gathered = scene.gather();
    assert!(gathered.tickets.is_empty(), "{gathered:?}");
    assert_eq!(gathered.unreadable.len(), 1);
    assert_eq!(gathered.unreadable[0].ticket, id(7));
    assert!(
        gathered.unreadable[0].reason.contains("the forge is down"),
        "{:?}",
        gathered.unreadable[0].reason
    );
}

#[test]
fn a_finished_check_without_a_time_makes_the_ticket_unreadable() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    let pr = scene.open_pr(7, CheckState::Passed);
    scene
        .forge
        .set_checks(
            pr,
            vec![Check {
                name: CheckName::from("ci"),
                state: CheckState::Passed,
                finished_at: None,
            }],
        )
        .expect("checks");
    let gathered = scene.gather();
    assert!(gathered.tickets.is_empty(), "{gathered:?}");
    assert!(
        gathered.unreadable[0]
            .reason
            .contains("check ci finished with no time"),
        "{:?}",
        gathered.unreadable
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
fn nothing_is_written_while_gathering_or_placing() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.task(8, EPIC, TicketState::Claimed, "#7");
    scene.open_pr(7, CheckState::Failed);
    let before = files(&scene.root);
    let home_before = files(&scene.home);
    let gathered = scene.gather();
    let placement = crate::domain::schedule::Placement::of(&gathered.tickets, time(100));
    assert!(!placement.lane.entries().is_empty(), "{placement:?}");
    assert_eq!(files(&scene.root), before);
    assert_eq!(files(&scene.home), home_before);
    assert_eq!(scene.gather(), gathered, "read again, the same facts");
}

#[test]
fn the_done_tasks_of_the_open_epics_are_kept_for_the_count() {
    let scene = Scene::new();
    scene.task(5, EPIC, TicketState::Ready, "—");
    scene.task(6, EPIC, TicketState::Claimed, "—");
    scene
        .board
        .set_state(id(6), TicketState::Done)
        .expect("done");
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.board.close(id(7));
    // A done Spike has no phase list, so it is no ticket the count counts.
    scene.task(8, EPIC, TicketState::Done, "—");
    scene.board.set_type(id(8), "Spike");

    let gathered = scene.gather();

    assert_eq!(gathered.done, [id(6), id(7)].into());
    // What the label sync reads: every open ticket with its label, and the open questions.
    let open: Vec<(TicketId, Option<TicketState>)> = gathered
        .open
        .iter()
        .map(|ticket| (ticket.id, ticket.state))
        .collect();
    assert_eq!(open, [(id(5), Some(TicketState::Ready))]);
}

#[test]
fn a_ticket_s_priority_label_reaches_its_facts() {
    let scene = Scene::new();
    scene.task(5, EPIC, TicketState::Ready, "—");
    scene.task(6, EPIC, TicketState::Ready, "—");
    scene.board.add_label(id(5), "priority/1");
    let gathered = scene.gather();
    let priorities: Vec<(TicketId, Option<u8>)> = gathered
        .tickets
        .iter()
        .map(|facts| {
            (
                facts.ticket,
                facts.priority.map(|priority| priority.level()),
            )
        })
        .collect();
    assert_eq!(priorities, [(id(5), Some(1)), (id(6), None)]);
    assert!(gathered.warnings.is_empty(), "{:?}", gathered.warnings);
}

#[test]
fn an_unknown_priority_label_is_ignored_and_named() {
    let scene = Scene::new();
    scene.task(5, EPIC, TicketState::Ready, "—");
    scene.task(6, EPIC, TicketState::Ready, "—");
    scene.board.add_label(id(5), "priority/urgent");
    scene.board.add_label(id(6), "priority/1");
    scene.board.add_label(id(6), "priority/2");
    let gathered = scene.gather();
    assert!(
        gathered.unreadable.is_empty(),
        "never a failure: {gathered:?}"
    );
    assert!(
        gathered
            .tickets
            .iter()
            .all(|facts| facts.priority.is_none()),
        "an ignored label gives no priority"
    );
    assert_eq!(
        gathered.warnings,
        [
            "#5: its priority labels are ignored: the label `priority/urgent` is not priority/1, \
             priority/2 or priority/3",
            "#6: its priority labels are ignored: it carries more than one priority label \
             (priority/1, priority/2)",
        ]
    );
}

#[test]
fn ready_since_is_the_latest_of_its_opening_and_its_last_done_dependency() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "#25, #26");
    scene.task(25, EPIC, TicketState::Ready, "—");
    scene.task(26, EPIC, TicketState::Ready, "—");
    scene.board.set_opened_at(id(7), time(1000));
    scene.board.close_at(id(25), time(3000));
    scene.board.close_at(id(26), time(2000));
    assert_eq!(
        scene.facts_of(7).ready_since,
        Some(time(3000)),
        "its last done dependency closed after it opened"
    );
    scene.board.set_opened_at(id(7), time(4000));
    assert_eq!(scene.facts_of(7).ready_since, Some(time(4000)));
    // With neither time the core falls back as before.
    scene.task(8, EPIC, TicketState::Ready, "—");
    assert_eq!(scene.facts_of(8).ready_since, None);
}

#[test]
fn the_open_epics_are_kept_for_the_challenge_deadline() {
    let scene = Scene::new();
    scene.board.add_signed_epic(id(4), "Issue 4");
    let epics: Vec<TicketId> = scene.gather().epics.iter().map(|epic| epic.id).collect();
    assert_eq!(epics, [id(EPIC), id(4)]);
}

/// The integration branch of version `v1` under the default pattern.
const BASE: &str = "feature/v1";

fn base_check(state: CheckState) -> Check {
    Check {
        name: CheckName::from("ci"),
        state,
        finished_at: (state != CheckState::Pending).then(|| time(40)),
    }
}

#[test]
fn the_integration_branch_is_the_pattern_with_the_version_filled() {
    let branches = |pattern: &str| crate::manifest::BranchesSection {
        integration: pattern.into(),
        ..crate::manifest::BranchesSection::default()
    };
    let default = crate::manifest::BranchesSection::default();
    assert_eq!(
        integration_branch(&default, Some("0.4.0")),
        Some(BranchName::from("feature/0.4.0"))
    );
    assert_eq!(integration_branch(&default, None), None);
    assert_eq!(integration_branch(&default, Some("")), None);
    // A pattern with no version needs none; one that names the epic is no one branch.
    assert_eq!(
        integration_branch(&branches("develop"), None),
        Some(BranchName::from("develop"))
    );
    assert_eq!(
        integration_branch(&branches("feature/{version}-{epic}"), Some("1")),
        None
    );
    assert_eq!(integration_branch(&branches("-x"), None), None);
}

#[test]
fn the_integration_branchs_checks_are_read_once_and_handed_to_every_ticket() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.task(8, EPIC, TicketState::Ready, "—");
    scene.task(9, EPIC, TicketState::Ready, "—");
    scene
        .forge
        .set_branch(BASE, "b1", vec![base_check(CheckState::Pending)]);
    let gathered = scene.gather_for_version("v1");
    assert_eq!(scene.forge.branch_reads(), 1, "once a tick, not per ticket");
    let read = BaseFacts {
        head: Some(CommitId::from("b1")),
        checks: vec![CheckFact {
            name: CheckName::from("ci"),
            result: CheckResult::Pending,
        }],
        reached_at: None,
    };
    assert_eq!(
        gathered.base,
        Some(Base {
            branch: BranchName::from(BASE),
            facts: read.clone(),
        })
    );
    assert_eq!(gathered.tickets.len(), 3);
    for facts in &gathered.tickets {
        assert_eq!(facts.merging.base, read);
    }
    assert!(gathered.warnings.is_empty(), "{:?}", gathered.warnings);
}

#[test]
fn a_ticket_at_its_merge_waits_on_the_integration_branchs_pending_checks() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene
        .forge
        .set_branch(BASE, "b1", vec![base_check(CheckState::Pending)]);
    let gathered = scene.gather_for_version("v1");
    assert_eq!(
        place(&gathered.tickets[0], time(100)),
        Place::Waiting([WaitingOn::Base(CommitId::from("b1"))].into())
    );
    // Once they ended, green or red, it is in the lane at its merge.
    for ended in [CheckState::Passed, CheckState::Failed] {
        scene.forge.set_branch(BASE, "b1", vec![base_check(ended)]);
        let gathered = scene.gather_for_version("v1");
        assert!(
            matches!(
                place(&gathered.tickets[0], time(100)),
                Place::InLane { ref next, .. } if next.as_str() == "merge"
            ),
            "{ended:?}"
        );
    }
}

#[test]
fn a_branch_read_that_fails_fails_nothing_holds_nothing_and_is_named() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene
        .forge
        .set_branch(BASE, "b1", vec![base_check(CheckState::Pending)]);
    scene.forge.fail_branch_checks();
    let gathered = scene.gather_for_version("v1");
    assert_eq!(gathered.base, None);
    assert_eq!(gathered.tickets[0].merging.base, BaseFacts::default());
    assert!(matches!(
        place(&gathered.tickets[0], time(100)),
        Place::InLane { .. }
    ));
    assert_eq!(
        gathered.warnings,
        [
            "the checks of the integration branch feature/v1 were not read, so no merge waits on \
          them: backend failed: the forge is down"
        ]
    );
}

#[test]
fn a_branch_the_forge_does_not_have_and_a_tick_with_no_version_read_as_unknown() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    // No such branch on the forge: asked, unknown, and nothing to warn of.
    let gathered = scene.gather_for_version("v1");
    assert_eq!(scene.forge.branch_reads(), 1);
    assert_eq!(
        gathered.base,
        Some(Base {
            branch: BranchName::from(BASE),
            facts: BaseFacts::default(),
        })
    );
    assert!(gathered.warnings.is_empty());
    // No version: no branch to name, and the forge is not asked.
    let gathered = scene.gather();
    assert_eq!(scene.forge.branch_reads(), 1);
    assert_eq!(gathered.base, None);
    assert!(gathered.warnings.is_empty());
}

#[test]
fn a_finished_branch_check_without_a_time_is_a_failed_read_not_a_guess() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Ready, "—");
    scene.forge.set_branch(
        BASE,
        "b1",
        vec![Check {
            name: CheckName::from("ci"),
            state: CheckState::Failed,
            finished_at: None,
        }],
    );
    let gathered = scene.gather_for_version("v1");
    assert_eq!(gathered.base, None);
    assert_eq!(gathered.tickets.len(), 1, "the ticket is still gathered");
    assert!(
        gathered.warnings[0].contains("check ci finished with no time"),
        "{:?}",
        gathered.warnings
    );
}

#[test]
fn a_ticket_fresh_from_a_fix_is_named_to_every_other_ticket_as_merging_first() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene.task(9, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(9, CheckState::Pending);
    scene.fixed(9);
    let gathered = scene.gather();
    assert_eq!(gathered.tickets[0].merging.first, Some(id(9)));
    assert_eq!(gathered.tickets[1].merging.first, None, "it goes first");
    assert_eq!(
        place(&gathered.tickets[0], time(100)),
        Place::Waiting([WaitingOn::Merge(id(9))].into())
    );
}

#[test]
fn one_ticket_gathered_alone_is_placed_as_the_tick_places_it() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene.task(9, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(9, CheckState::Pending);
    scene.fixed(9);
    scene
        .forge
        .set_branch(BASE, "b1", vec![base_check(CheckState::Pending)]);
    let whole = scene.gather_for_version("v1");
    let alone = |task: u64| {
        scene.with_ports(Some("v1"), |ports| {
            gather_one(ports, id(task))
                .expect("it reads")
                .expect("it is open")
                .0
        })
    };
    // #7 stands at its merge: the other tickets and the branch are read for it.
    let seven = alone(7);
    assert_eq!(seven, whole.tickets[0]);
    assert_eq!(
        place(&seven, time(100)),
        Place::Waiting(
            [
                WaitingOn::Merge(id(9)),
                WaitingOn::Base(CommitId::from("b1"))
            ]
            .into()
        )
    );
    // #9 waits on its checks: nothing more is read for it.
    let reads = scene.forge.branch_reads();
    let nine = alone(9);
    assert_eq!(scene.forge.branch_reads(), reads);
    assert_eq!(nine.merging, MergeFacts::default());
    assert_eq!(place(&nine, time(100)), place(&whole.tickets[1], time(100)));
}

/// The group of a tick that began with the branch at `b0`, its checks ended, and whose own last
/// merge left the branch at `head`.
fn group_at(head: &str) -> GroupBase {
    GroupBase {
        head: CommitId::from(head),
        base: BaseFacts {
            head: Some(CommitId::from("b0")),
            checks: vec![CheckFact {
                name: CheckName::from("ci"),
                result: CheckResult::Passed(time(40)),
            }],
            reached_at: None,
        },
    }
}

/// #7 at its merge, and the place the re-check before its merge gives it for `group`, the branch
/// being as `scene` set it.
fn rechecked(scene: &Scene, group: Option<&GroupBase>) -> Place {
    let (facts, _) = scene.with_ports(Some("v1"), |ports| {
        gather_one_in(ports, id(7), group)
            .expect("it reads")
            .expect("it is open")
    });
    place(&facts, time(100))
}

fn at_merge_behind_pending_checks_at(head: &str) -> Scene {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene
        .forge
        .set_branch(BASE, head, vec![base_check(CheckState::Pending)]);
    scene
}

#[test]
fn the_groups_own_commit_does_not_hold_its_next_merge() {
    // The group's first merge left the branch at b1, where checks are pending.
    let scene = at_merge_behind_pending_checks_at("b1");
    assert_eq!(
        rechecked(&scene, None),
        Place::Waiting([WaitingOn::Base(CommitId::from("b1"))].into()),
        "judged as the branch reads, they hold it"
    );
    // The branch is still at the group's last commit: R29 is judged at the group's base.
    assert!(
        matches!(
            rechecked(&scene, Some(&group_at("b1"))),
            Place::InLane { ref next, .. } if next.as_str() == "merge"
        ),
        "{:?}",
        rechecked(&scene, Some(&group_at("b1")))
    );
}

#[test]
fn a_foreign_push_mid_group_holds_the_next_merge() {
    // Someone else pushed b2 after the group's b1, and checks are pending at b2.
    let scene = at_merge_behind_pending_checks_at("b2");
    assert_eq!(
        rechecked(&scene, Some(&group_at("b1"))),
        Place::Waiting([WaitingOn::Base(CommitId::from("b2"))].into())
    );
}

#[test]
fn the_next_tick_waits_at_the_groups_last_commit() {
    // A tick that follows the group gathers with no group, so the checks at b1 hold the merge.
    let scene = at_merge_behind_pending_checks_at("b1");
    let gathered = scene.gather_for_version("v1");
    assert_eq!(
        place(&gathered.tickets[0], time(100)),
        Place::Waiting([WaitingOn::Base(CommitId::from("b1"))].into())
    );
    // And the base the tick holds is the branch as read, never the group's.
    assert_eq!(
        gathered.base.map(|base| base.facts.head),
        Some(Some(CommitId::from("b1")))
    );
}

#[test]
fn merges_are_ordered_only_among_the_tickets_of_one_integration_branch() {
    let mut scene = Scene::new();
    scene.board.add_challenged_epic(id(3), "Issue 3");
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene.task(8, 3, TicketState::Claimed, "—");
    scene.open_changing_pr(8, CheckState::Passed);
    scene.task(9, 3, TicketState::Claimed, "—");
    scene.open_changing_pr(9, CheckState::Pending);
    scene.fixed(9);
    let first = |gathered: &Gathered| -> Vec<Option<TicketId>> {
        gathered
            .tickets
            .iter()
            .map(|facts| facts.merging.first)
            .collect()
    };
    // One branch for the version: #9 goes before both.
    assert_eq!(first(&scene.gather()), [Some(id(9)), Some(id(9)), None]);
    // A branch for each epic: only #8 shares #9's.
    scene.manifest.branches.integration = "feature/{version}-{epic}".into();
    let gathered = scene.gather();
    assert_eq!(first(&gathered), [None, Some(id(9)), None]);
    assert_eq!(
        gathered
            .tickets
            .iter()
            .map(|facts| facts.ticket)
            .collect::<Vec<_>>(),
        [id(7), id(8), id(9)],
        "still in ticket order"
    );
    // A branch for each task: nobody shares one.
    scene.manifest.branches.integration = "feature/{version}-{task}".into();
    assert_eq!(first(&scene.gather()), [None, None, None]);
}

#[test]
fn a_ruled_question_on_the_base_deadline_lets_its_ticket_through_at_that_head() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Passed);
    scene
        .forge
        .set_branch(BASE, "b1", vec![base_check(CheckState::Pending)]);
    let waits = Place::Waiting([WaitingOn::Base(CommitId::from("b1"))].into());
    let place_of_seven = || place(&scene.gather_for_version("v1").tickets[0], time(100));
    assert_eq!(place_of_seven(), waits);

    // The scheduler's question on it, as the tick words its marker; open, it rules nothing.
    let asked = NewQuestion {
        on: id(7),
        text: format!(
            "The checks are still pending. {}",
            base_deadline_marker(&CommitId::from("b1"))
        ),
        door: false,
    };
    scene.board.add_question(Ticket {
        id: id(50),
        epic: None,
        title: asked.title().into(),
        body: asked.body().into(),
        state: None,
        url: "".into(),
        kind: None,
    });
    let gathered = scene.gather_for_version("v1");
    assert_eq!(gathered.tickets[0].merging.ruled_at, None);
    assert_eq!(
        place(&gathered.tickets[0], time(100)),
        Place::Waiting([WaitingOn::Question(id(50))].into())
    );

    // Ruled: the branch's checks no longer hold it at b1.
    scene
        .board
        .record_ruling(id(50), "Merge it.")
        .expect("rules");
    let gathered = scene.gather_for_version("v1");
    assert_eq!(
        gathered.tickets[0].merging.ruled_at,
        Some(CommitId::from("b1"))
    );
    assert!(matches!(
        place(&gathered.tickets[0], time(100)),
        Place::InLane { ref next, .. } if next.as_str() == "merge"
    ));
    assert!(gathered.warnings.is_empty(), "{:?}", gathered.warnings);

    // The branch moved on: the ruling was for b1, and b2's checks hold it again.
    scene
        .forge
        .set_branch(BASE, "b2", vec![base_check(CheckState::Pending)]);
    let gathered = scene.gather_for_version("v1");
    assert_eq!(gathered.tickets[0].merging.ruled_at, None);
    assert_eq!(
        place(&gathered.tickets[0], time(100)),
        Place::Waiting([WaitingOn::Base(CommitId::from("b2"))].into())
    );
}

impl Scene {
    /// `task`'s pull request at head `h1`, changing a file, with no check registered there.
    fn open_unchecked_pr(&self, task: u64) -> PrNumber {
        let pr = self.open_changing_pr(task, CheckState::Passed);
        self.forge.set_checks(pr, Vec::new()).expect("no checks");
        pr
    }

    /// The question the scheduler raises on `task` for a head with no check, number `number`,
    /// ruled when `ruling` is given.
    fn no_check_question(&self, number: u64, task: u64, head: &str, ruling: Option<&str>) {
        let asked = NewQuestion {
            on: id(task),
            text: format!(
                "No check has registered. {}",
                no_check_marker(&CommitId::from(head))
            ),
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
        if let Some(ruling) = ruling {
            self.board.record_ruling(id(number), ruling).expect("rules");
        }
    }
}

/// The integration branch of version `1.0`, where the workflows are read.
const INTEGRATION: &str = "feature/1.0";

impl Scene {
    /// A scene whose forge has one workflow at the integration branch, which runs on a pull
    /// request only when it changes a file under `zaf/`.
    fn filtered(mut self) -> Self {
        let filter = FakeCiFilter::default();
        filter.set_workflow(
            INTEGRATION,
            ".forgejo/workflows/zaf.yml",
            PullRequestFilter::Paths(vec!["zaf/**".to_owned()]),
        );
        self.ci_filter = Some(filter);
        self
    }

    /// `task`'s pull request with no check at its head, changing `files`.
    fn unchecked_pr_changing(&self, task: u64, files: &[&str]) -> PrNumber {
        let pr = self.open_unchecked_pr(task);
        self.forge.set_changed_files(pr, files).expect("files");
        pr
    }

    /// The facts of `task` gathered for version `1.0`, the question it raises read as the tick
    /// reads it: whether its wait is overdue.
    fn facts_for_version(&self, task: u64) -> TicketFacts {
        self.gather_for_version("1.0")
            .tickets
            .into_iter()
            .find(|facts| facts.ticket == id(task))
            .expect("the task is gathered")
    }
}

/// The ticket goes on to its merge and asks nothing, however long it has waited.
fn asks_nothing(facts: &TicketFacts) -> bool {
    goes_on_to_merge(facts)
        && crate::domain::schedule::overdue(facts, time(20) + Duration::from_secs(24 * 3600))
            .is_none()
}

#[test]
fn docs_only_pull_request_raises_no_question() {
    let scene = Scene::new().filtered();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.unchecked_pr_changing(7, &["docs/x.md"]);
    let facts = scene.facts_for_version(7);
    assert_eq!(
        facts.pull.as_ref().map(|pull| pull.checks_expected),
        Some(false)
    );
    assert!(asks_nothing(&facts), "{:?}", place(&facts, time(100)));
    // The same pull request on a forge with no filter port waits for a check, as today.
    let unfiltered = Scene::new();
    unfiltered.task(7, EPIC, TicketState::Claimed, "—");
    unfiltered.unchecked_pr_changing(7, &["docs/x.md"]);
    let facts = unfiltered.facts_for_version(7);
    assert_eq!(
        facts.pull.as_ref().map(|pull| pull.checks_expected),
        Some(true)
    );
    assert!(!goes_on_to_merge(&facts));
}

#[test]
fn a_push_workflow_is_expected_only_for_the_slice_branch_it_matches() {
    for (patterns, expected) in [(vec!["main"], false), (vec!["main", "feature/**"], true)] {
        let mut scene = Scene::new();
        let filter = FakeCiFilter::default();
        filter.set_workflow(
            INTEGRATION,
            ".forgejo/workflows/zaf.yml",
            PullRequestFilter::Branches(patterns.iter().map(|p| (*p).to_owned()).collect()),
        );
        scene.ci_filter = Some(filter);
        scene.task(7, EPIC, TicketState::Claimed, "—");
        scene.unchecked_pr_changing(7, &["docs/x.md"]);
        let facts = scene.facts_for_version(7);
        assert_eq!(
            facts.pull.as_ref().map(|pull| pull.checks_expected),
            Some(expected),
            "{patterns:?}"
        );
    }
}

#[test]
fn workflow_change_still_asks() {
    let scene = Scene::new().filtered();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.unchecked_pr_changing(7, &["docs/x.md", ".forgejo/workflows/zaf.yml"]);
    let facts = scene.facts_for_version(7);
    assert_eq!(
        facts.pull.as_ref().map(|pull| pull.checks_expected),
        Some(true)
    );
    assert!(!goes_on_to_merge(&facts));
    assert!(
        crate::domain::schedule::overdue(&facts, time(20) + NO_CHECK_DEADLINE).is_some(),
        "the No check question is raised"
    );
}

#[test]
fn rename_into_docs_still_asks() {
    let scene = Scene::new().filtered();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    // A code file moved into docs/: the forge lists the old name too.
    scene.unchecked_pr_changing(7, &["zaf/src/lib.rs", "docs/lib.md"]);
    let facts = scene.facts_for_version(7);
    assert_eq!(
        facts.pull.as_ref().map(|pull| pull.checks_expected),
        Some(true)
    );
    assert!(!goes_on_to_merge(&facts));
}

#[test]
fn failed_workflow_read_counts_as_expected() {
    let scene = Scene::new().filtered();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.unchecked_pr_changing(7, &["docs/x.md"]);
    if let Some(filter) = &scene.ci_filter {
        filter.fail("forge down");
    }
    let facts = scene.facts_for_version(7);
    assert_eq!(
        facts.pull.as_ref().map(|pull| pull.checks_expected),
        Some(true)
    );
    assert!(!goes_on_to_merge(&facts));
}

#[test]
fn workflows_are_read_for_no_pull_request_with_a_check_and_once_per_base() {
    let scene = Scene::new().filtered();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.task(8, EPIC, TicketState::Claimed, "—");
    scene.task(9, EPIC, TicketState::Claimed, "—");
    scene.unchecked_pr_changing(7, &["docs/x.md"]);
    scene.unchecked_pr_changing(8, &["docs/y.md"]);
    scene.open_changing_pr(9, CheckState::Pending);
    let gathered = scene.gather_for_version("1.0");
    let expected: Vec<bool> = gathered
        .tickets
        .iter()
        .map(|facts| facts.pull.as_ref().expect("pull").checks_expected)
        .collect();
    // The pending one is not asked; the two with no check are not expected one.
    assert_eq!(expected, vec![false, false, true]);
    assert_eq!(scene.ci_filter.as_ref().map(FakeCiFilter::reads), Some(1));
}

fn goes_on_to_merge(facts: &TicketFacts) -> bool {
    matches!(
        place(facts, time(100)),
        Place::InLane { ref next, .. } if next.as_str() == "merge"
    )
}

#[test]
fn gather_reads_a_ruled_no_check_question() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", None);
    // Open, the question rules nothing: the ticket waits on a check that never comes.
    let facts = scene.facts_of(7);
    assert_eq!(facts.no_check_ruled, None);
    assert!(!goes_on_to_merge(&facts));

    scene
        .board
        .record_ruling(id(50), "Go on without it.")
        .expect("rules");
    let gathered = scene.gather();
    let facts = &gathered.tickets[0];
    assert_eq!(facts.no_check_ruled, Some(CommitId::from("h1")));
    assert!(goes_on_to_merge(facts), "{:?}", place(facts, time(100)));
    assert!(gathered.warnings.is_empty(), "{:?}", gathered.warnings);
}

#[test]
fn a_young_pull_request_is_not_read_for_a_no_check_ruling() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    let reads = scene.board.question_reads();
    // One second short of the deadline after the opening at 20.
    let now = time(20) + NO_CHECK_DEADLINE - Duration::from_secs(1);
    let gathered = scene.with_ports_at(None, now, |ports| gather(ports).expect("it reads"));
    assert_eq!(gathered.tickets[0].no_check_ruled, None);
    assert_eq!(scene.board.question_reads(), reads);
    assert!(gathered.warnings.is_empty(), "{:?}", gathered.warnings);
}

#[test]
fn an_old_enough_pull_request_reads_its_no_check_ruling() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    let reads = scene.board.question_reads();
    // Exactly the deadline after the opening at 20.
    let now = time(20) + NO_CHECK_DEADLINE;
    let gathered = scene.with_ports_at(None, now, |ports| gather(ports).expect("it reads"));
    assert_eq!(
        gathered.tickets[0].no_check_ruled,
        Some(CommitId::from("h1"))
    );
    assert!(scene.board.question_reads() > reads);
}

#[test]
fn gather_one_reads_a_ruled_no_check_question() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    let (alone, _) = scene.with_ports(None, |ports| {
        gather_one(ports, id(7))
            .expect("it reads")
            .expect("it is open")
    });
    assert_eq!(alone.no_check_ruled, Some(CommitId::from("h1")));
    assert!(goes_on_to_merge(&alone));
    assert_eq!(alone, scene.facts_of(7));
}

#[test]
fn gather_ignores_a_ruling_on_an_old_head() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    let pr = scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    assert_eq!(scene.facts_of(7).no_check_ruled, Some(CommitId::from("h1")));

    // A push moves the head, and no check has registered there either.
    scene.forge.set_head(pr, Some("h2")).expect("head");
    let facts = scene.facts_of(7);
    assert_eq!(facts.no_check_ruled, None);
    assert!(!goes_on_to_merge(&facts));
}

#[test]
fn gather_skips_the_read_when_the_head_has_checks() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_changing_pr(7, CheckState::Pending);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    let reads = scene.board.question_reads();
    let facts = scene.facts_of(7);
    assert_eq!(scene.board.question_reads(), reads);
    assert_eq!(facts.no_check_ruled, None);
    // No pull request at all reads nothing either.
    scene.task(8, EPIC, TicketState::Claimed, "—");
    scene.facts_of(8);
    assert_eq!(scene.board.question_reads(), reads);
}

#[test]
fn gather_keeps_waiting_when_rulings_cannot_be_read() {
    let scene = Scene::new();
    scene.task(7, EPIC, TicketState::Claimed, "—");
    scene.open_unchecked_pr(7);
    scene.no_check_question(50, 7, "h1", Some("Go on without it."));
    scene.board.fail_question_reads();
    let gathered = scene.gather();
    assert!(gathered.unreadable.is_empty(), "{:?}", gathered.unreadable);
    let facts = &gathered.tickets[0];
    assert_eq!(facts.no_check_ruled, None);
    assert_eq!(
        place(facts, time(100)),
        Place::Waiting([WaitingOn::Check(id(7), CommitId::from("h1"))].into())
    );
    assert_eq!(gathered.warnings.len(), 1, "{:?}", gathered.warnings);
    assert!(
        gathered.warnings[0].starts_with("#7: its ruled questions were not read"),
        "{}",
        gathered.warnings[0]
    );
}

/// A VCS that counts how it is asked for trees.
struct CountingVcs {
    inner: crate::fakes::FakeVcs,
    one_at_a_time: std::cell::Cell<usize>,
    batches: std::cell::Cell<usize>,
}

impl crate::ports::Vcs for CountingVcs {
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError> {
        self.inner.latest_commit(workspace)
    }
    fn holds(&self, workspace: &Path, head: &CommitId) -> Result<bool, PortError> {
        self.inner.holds(workspace, head)
    }
    fn tree_of(&self, root: &Path, commit: &CommitId) -> Result<crate::domain::TreeId, PortError> {
        self.one_at_a_time.set(self.one_at_a_time.get() + 1);
        self.inner.tree_of(root, commit)
    }
    fn trees_of(
        &self,
        root: &Path,
        commits: &[&CommitId],
    ) -> Vec<(CommitId, crate::domain::TreeId)> {
        self.batches.set(self.batches.get() + 1);
        self.inner.trees_of(root, commits)
    }
    fn diff_stat(
        &self,
        workspace: &Path,
        base: &BaseName,
    ) -> Result<crate::ports::DiffStat, PortError> {
        self.inner.diff_stat(workspace, base)
    }
    fn has_tag(&self, root: &Path, tag: &str) -> Result<bool, PortError> {
        self.inner.has_tag(root, tag)
    }
    fn has_remote(&self, root: &Path) -> Result<bool, PortError> {
        self.inner.has_remote(root)
    }
    fn tags(&self, root: &Path) -> Result<Vec<String>, PortError> {
        self.inner.tags(root)
    }
    fn diff_hunks(
        &self,
        workspace: &Path,
        base: &BaseName,
        path: &str,
    ) -> Result<String, PortError> {
        self.inner.diff_hunks(workspace, base, path)
    }
}

#[test]
fn gather_finds_git_folder_once() {
    let scene = Scene::new();
    let vcs = CountingVcs {
        inner: crate::fakes::FakeVcs::default(),
        one_at_a_time: std::cell::Cell::new(0),
        batches: std::cell::Cell::new(0),
    };
    // Four commits to read trees of: the head, and the head each of three runs began at.
    let fix = ActionName::from("fix");
    let runs: Vec<RunRecord> = ["h2", "h3", "h4"]
        .iter()
        .map(|head| RunRecord {
            action: fix.clone(),
            head: Some(CommitId::from(*head)),
            head_end: HeadEnd::Unknown,
            started_at: time(1),
            alive: false,
            exit: None,
            questions: BTreeSet::new(),
            last_line: None,
        })
        .collect();
    let head = CommitId::from("h1");
    let ports = Ports {
        board: &scene.board,
        forge: &scene.forge,
        ci_filter: None,
        manifest: &scene.manifest,
        root: &scene.root,
        home: &scene.home,
        state_dir: &scene.state_dir,
        alive: &|_: &Holder| false,
        version: None,
        now: time(100),
        vcs: Some(&vcs),
    };
    read_trees(&ports, Some(&head), &runs, &[fix]);
    // One lookup for all four commits, and none for each.
    assert_eq!(vcs.batches.get(), 1);
    assert_eq!(vcs.one_at_a_time.get(), 0);
}
