//! The board's contract: what every backend behind [`Board`] does, checked the same way on
//! each.
//!
//! Each check takes an [`Arrange`]: a backend's own way to put epics and tasks on its board
//! and close them, which the port itself does not offer. The checks run on the in-memory
//! board on every test run. They run on the GitHub issues board too when
//! `ZAF_CONTRACT_REPO` names a scratch repository (`owner/name`) that gh can write to; each
//! check opens its own issues there and closes them as not planned when it ends. Without the
//! variable the GitHub checks pass without running. They run on the GitHub Projects board as
//! well when `ZAF_CONTRACT_PROJECT` also names a Project (`owner/number`) linked to that
//! repository: states then live in its `zaf state` field, created on the first write.
//!
//! A backend's lists may lag behind its writes: GitHub lists a new issue, label or comment a
//! few seconds after the write. A check that reads a list after a write reads it again until
//! it shows the write, for as long as the backend's [`Arrange::patience`], and then asserts
//! on the last read. A read of one ticket by its id does not lag.

use std::cell::{Cell, RefCell};
use std::fmt::Debug;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use zaf::adapters::github::{GhCommand, GithubBoard, GithubProjectBoard, ProjectRef};

use zaf::domain::{
    BodyHash, Epic, EpicStage, Instance, NewTask, Signature, Standing, Ticket, TicketId,
    TicketState,
};
use zaf::fakes::FakeBoard;
use zaf::manifest::{DEFAULT_STATE_FIELD, RepoName};
use zaf::ports::{Board, ClaimOutcome, NewQuestion, PortError};

/// The environment variable naming the scratch repository the GitHub checks write to.
const REPO_VAR: &str = "ZAF_CONTRACT_REPO";

/// The environment variable naming the scratch Project (`owner/number`) the Projects checks
/// keep states in.
const PROJECT_VAR: &str = "ZAF_CONTRACT_PROJECT";

/// A backend's way to arrange its board for one check.
trait Arrange {
    /// The board under test.
    fn board(&self) -> &dyn Board;
    /// Opens an epic.
    fn epic(&self, title: &str) -> TicketId;
    /// Opens a task under `epic` in `state`.
    fn task(&self, epic: TicketId, title: &str, body: &str, state: TicketState) -> TicketId;
    /// Closes a ticket, as completed or as not planned.
    fn close(&self, id: TicketId, completed: bool);
    /// Replaces a ticket's body, as a person editing it would.
    fn edit_body(&self, id: TicketId, body: &str);
    /// Takes a ticket the board itself opened into the arranger's care, to close at the end.
    fn adopt(&self, id: TicketId);
    /// An id no ticket has.
    fn missing(&self) -> TicketId;
    /// How long its lists may lag behind its writes.
    fn patience(&self) -> Duration;
}

/// Reads with `read` until `shows` holds of what it returns, or the backend's patience runs
/// out, and returns the last read for the check to assert on.
fn settled<T: Debug>(arrange: &dyn Arrange, read: impl Fn() -> T, shows: impl Fn(&T) -> bool) -> T {
    let deadline = Instant::now() + arrange.patience();
    loop {
        let value = read();
        if shows(&value) || Instant::now() >= deadline {
            return value;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// A team instance for the checks, on `epic`.
fn instance(n: u32, epic: TicketId) -> Instance {
    role_instance("team", n, epic)
}

/// An instance of `role` for the checks, on `epic`.
fn role_instance(role: &str, n: u32, epic: TicketId) -> Instance {
    format!("{role} instance {n} on {epic}")
        .parse()
        .expect("an instance")
}

const BODY: &str = "Goal: hold to the contract.";

// The checks.

fn a_task_reads_back_as_arranged(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "contract task", BODY, TicketState::Ready);
    let ticket = arrange.board().task(task).expect("the task reads");
    assert_eq!(ticket.id, task);
    assert_eq!(ticket.epic, Some(epic));
    assert_eq!(ticket.title.as_str(), "contract task");
    assert_eq!(ticket.body.as_str().trim(), BODY);
    assert_eq!(ticket.state, Some(TicketState::Ready));
    assert!(!ticket.url.as_str().is_empty(), "a ticket carries its link");
}

fn a_missing_ticket_is_not_found(arrange: &dyn Arrange) {
    let read = arrange.board().task(arrange.missing());
    assert!(matches!(read, Err(PortError::NotFound(_))), "got {read:?}");
}

fn ready_lists_ready_tasks_only_in_id_order(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let first = arrange.task(epic, "first", BODY, TicketState::Ready);
    let blocked = arrange.task(epic, "blocked", BODY, TicketState::Blocked);
    let second = arrange.task(epic, "second", BODY, TicketState::Ready);
    let ready_of_these = || -> Vec<TicketId> {
        arrange
            .board()
            .ready()
            .expect("ready reads")
            .iter()
            .map(|ticket| ticket.id)
            .filter(|id| [first, blocked, second].contains(id))
            .collect()
    };
    let ready = settled(arrange, ready_of_these, |ready| ready == &[first, second]);
    assert_eq!(ready, [first, second]);
}

fn a_claim_is_held_by_one_instance(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "claimed", BODY, TicketState::Ready);
    let (holder, other) = (instance(901, epic), instance(902, epic));
    let board = arrange.board();
    assert_eq!(board.claim(task, &holder), Ok(ClaimOutcome::Claimed));
    assert_eq!(board.claim(task, &holder), Ok(ClaimOutcome::AlreadyHeld));
    let by_other = board.claim(task, &other);
    assert!(
        matches!(by_other, Err(PortError::Conflict(_))),
        "got {by_other:?}"
    );
    let ticket = board.task(task).expect("the task reads");
    assert_eq!(ticket.state, Some(TicketState::Claimed));
    let holders = || -> Vec<Instance> {
        let claims = board.claims().expect("claims read");
        claims
            .into_iter()
            .filter(|claim| claim.task == task)
            .map(|claim| claim.instance)
            .collect()
    };
    let held = settled(arrange, holders, |held| !held.is_empty());
    assert_eq!(held, [holder]);
}

fn a_released_task_is_ready_again(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "released", BODY, TicketState::Ready);
    let holder = instance(903, epic);
    let board = arrange.board();
    board.claim(task, &holder).expect("the claim");
    board.release(task, &holder).expect("the release");
    let ticket = board.task(task).expect("the task reads");
    assert_eq!(ticket.state, Some(TicketState::Ready));
    let claims = settled(
        arrange,
        || board.claims().expect("claims read"),
        |claims| claims.iter().all(|claim| claim.task != task),
    );
    assert!(claims.iter().all(|claim| claim.task != task));
}

fn a_new_state_replaces_the_old(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "moved", BODY, TicketState::Ready);
    let board = arrange.board();
    for state in [TicketState::Blocked, TicketState::Done] {
        board.set_state(task, state).expect("the state is set");
        let ticket = board.task(task).expect("the task reads");
        assert_eq!(ticket.state, Some(state));
    }
}

fn a_ruling_closes_its_question_and_frees_the_task(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "waiting", BODY, TicketState::Question);
    let board = arrange.board();
    let asked = NewQuestion {
        on: task,
        text: "Which way does the contract go?".to_owned(),
        door: false,
    };
    let question = board.open_question(&asked).expect("the question opens");
    let open: Vec<Ticket> = settled(
        arrange,
        || board.questions().expect("questions read"),
        |open| open.iter().any(|ticket| ticket.id == question),
    );
    let listed = open
        .iter()
        .find(|ticket| ticket.id == question)
        .expect("the question is listed");
    assert!(
        !listed.url.as_str().is_empty(),
        "a question carries its link"
    );
    assert_eq!(NewQuestion::raised_on(listed.body.as_str()), Some(task));

    assert_eq!(board.record_ruling(question, "this way"), Ok(Some(task)));
    let ticket = board.task(task).expect("the task reads");
    assert_eq!(ticket.state, Some(TicketState::Ready));
    let open = settled(
        arrange,
        || board.questions().expect("questions read"),
        |open| open.iter().all(|ticket| ticket.id != question),
    );
    assert!(open.iter().all(|ticket| ticket.id != question));
    assert!(board.record_ruling(question, "this way").is_ok());
    let changed = board.record_ruling(question, "the other way");
    assert!(
        matches!(changed, Err(PortError::Conflict(_))),
        "got {changed:?}"
    );
    let rulings = || -> Vec<(TicketId, Option<String>)> {
        let raised = board
            .questions_on(task)
            .expect("questions on the task read");
        raised
            .into_iter()
            .map(|(ticket, ruling)| (ticket.id, ruling))
            .collect()
    };
    let expected = [(question, Some("this way".to_owned()))];
    let ruled = settled(arrange, rulings, |ruled| ruled == &expected);
    assert_eq!(ruled, expected);
}

fn standing_follows_how_a_ticket_closed(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let open = arrange.task(epic, "open", BODY, TicketState::Ready);
    let completed = arrange.task(epic, "completed", BODY, TicketState::Ready);
    let dropped = arrange.task(epic, "dropped", BODY, TicketState::Ready);
    arrange.close(completed, true);
    arrange.close(dropped, false);
    let board = arrange.board();
    assert_eq!(board.standing(open), Ok(Standing::Open));
    assert_eq!(board.standing(completed), Ok(Standing::Done));
    assert_eq!(board.standing(dropped), Ok(Standing::ClosedNotPlanned));
    assert_eq!(board.standing(arrange.missing()), Ok(Standing::Missing));
}

fn an_open_epic_lists_its_tasks_closed_ones_too(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic with tasks");
    let open = arrange.task(epic, "open", BODY, TicketState::Ready);
    let completed = arrange.task(epic, "completed", BODY, TicketState::Ready);
    let dropped = arrange.task(epic, "dropped", BODY, TicketState::Ready);
    arrange.close(completed, true);
    arrange.close(dropped, false);
    let expected = [
        (open, Standing::Open),
        (completed, Standing::Done),
        (dropped, Standing::ClosedNotPlanned),
    ];
    let standings = |epic: &Epic| -> Vec<(TicketId, Standing)> {
        epic.tasks
            .iter()
            .map(|task| (task.number, task.standing))
            .collect()
    };
    let read = settled(
        arrange,
        || arrange.board().epic(epic),
        |read| read.as_ref().is_ok_and(|read| standings(read) == expected),
    )
    .expect("the epic reads");
    assert_eq!(read.title.as_str(), "contract epic with tasks");
    assert_eq!(standings(&read), expected);
    assert_eq!(read.tally(), (1, 2), "a dropped task counts for nothing");
    let listed = arrange.board().epics().expect("epics read");
    assert!(listed.iter().any(|listed| listed.id == epic));
}

fn a_signature_holds_until_the_body_changes(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic to sign");
    let board = arrange.board();
    let read = || board.epic(epic);
    let before = settled(arrange, read, Result::is_ok).expect("the epic reads");
    assert_eq!(before.signature, None);
    assert_eq!(before.stage(), EpicStage::Unsigned);

    let signature = Signature::of(&before.body, SystemTime::now());
    board.sign(epic, &signature).expect("the epic is signed");
    let signed = settled(arrange, read, |read| read.as_ref().is_ok_and(Epic::signed))
        .expect("the epic reads");
    assert_eq!(
        signed.signature.as_ref().map(|signature| &signature.hash),
        Some(&BodyHash::of(&before.body))
    );
    assert!(signed.signed());
    assert_eq!(signed.stage(), EpicStage::Signed);

    arrange.edit_body(
        epic,
        "An epic of the contract checks, edited after signing.",
    );
    let edited = settled(arrange, read, |read| {
        read.as_ref().is_ok_and(Epic::signature_void)
    })
    .expect("the epic reads");
    assert!(edited.signature_void(), "an edit voids the signature");
    assert_eq!(edited.stage(), EpicStage::Unsigned);
}

fn a_challengers_report_challenges_its_epic(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic to challenge");
    arrange.task(epic, "planned", BODY, TicketState::Ready);
    let board = arrange.board();
    board
        .report(
            epic,
            &instance(904, epic),
            "A team's report is no challenge.",
        )
        .expect("the team reports");
    let listed = settled(arrange, || board.epic(epic), Result::is_ok);
    assert!(!listed.expect("the epic reads").challenged);
    let challenger = role_instance("challenger", 905, epic);
    board
        .report(epic, &challenger, "Every task is exact.")
        .expect("the challenger reports");
    let read = settled(
        arrange,
        || board.epic(epic),
        |read| read.as_ref().is_ok_and(|read| read.challenged),
    );
    assert!(read.expect("the epic reads").challenged);
}

fn a_ticket_that_is_no_open_epic_is_not_one(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic");
    let task = arrange.task(epic, "a task", BODY, TicketState::Ready);
    // Wait until the epic lists, so a task missing from it is not only lagging.
    let listed = settled(
        arrange,
        || arrange.board().epic(epic),
        |read| read.as_ref().is_ok_and(|read| !read.tasks.is_empty()),
    );
    assert!(listed.is_ok(), "got {listed:?}");
    let read = arrange.board().epic(task);
    assert!(matches!(read, Err(PortError::NotFound(_))), "got {read:?}");
}

fn a_filed_task_reads_back_under_its_epic(arrange: &dyn Arrange) {
    let epic = arrange.epic("contract epic to file under");
    let board = arrange.board();
    let filed = board
        .file_task(&NewTask {
            epic,
            title: "filed".into(),
            body: BODY.into(),
            state: TicketState::Blocked,
            group: Some("contract".into()),
        })
        .expect("the task is filed");
    arrange.adopt(filed);
    let ticket = board.task(filed).expect("the task reads");
    assert_eq!(ticket.epic, Some(epic));
    assert_eq!(ticket.title.as_str(), "filed");
    assert_eq!(ticket.body.as_str().trim(), BODY);
    assert_eq!(ticket.state, Some(TicketState::Blocked));
    let read = settled(
        arrange,
        || board.epic(epic),
        |read| read.as_ref().is_ok_and(|read| !read.tasks.is_empty()),
    )
    .expect("the epic reads");
    let listed: Vec<_> = read
        .tasks
        .iter()
        .map(|task| {
            (
                task.number,
                task.title.as_str(),
                task.group.as_ref().map(|g| g.as_str()),
            )
        })
        .collect();
    assert_eq!(listed, [(filed, "filed", Some("contract"))]);
}

// The backends.

/// The in-memory board, numbering tickets from 1.
struct FakeArrange {
    board: FakeBoard,
    next: Cell<u64>,
}

impl FakeArrange {
    fn new() -> Self {
        Self {
            board: FakeBoard::default(),
            next: Cell::new(1),
        }
    }

    fn take(&self) -> TicketId {
        let id = self.next.get();
        self.next.set(id + 1);
        TicketId::from(id)
    }
}

impl Arrange for FakeArrange {
    fn board(&self) -> &dyn Board {
        &self.board
    }

    fn epic(&self, title: &str) -> TicketId {
        let id = self.take();
        self.board.add_epic(id, title);
        id
    }

    fn task(&self, epic: TicketId, title: &str, body: &str, state: TicketState) -> TicketId {
        let id = self.take();
        self.board.add_ticket(Ticket {
            id,
            epic: Some(epic),
            title: title.into(),
            body: body.into(),
            state: Some(state),
            url: zaf::fakes::board_link(id).into(),
        });
        id
    }

    fn close(&self, id: TicketId, completed: bool) {
        if completed {
            self.board.close(id);
        } else {
            self.board.close_not_planned(id);
        }
    }

    fn edit_body(&self, id: TicketId, body: &str) {
        self.board.set_body(id, body);
    }

    fn adopt(&self, _: TicketId) {}

    fn missing(&self) -> TicketId {
        TicketId::from(999_999)
    }

    fn patience(&self) -> Duration {
        Duration::ZERO
    }
}

/// A GitHub board on the scratch repository, closing what it opened when dropped: states in
/// labels, or in a field of the scratch Project.
struct GithubArrange {
    repo: String,
    board: Box<dyn Board>,
    /// Whether states live in labels; otherwise the arranger sets them through the board.
    labels: bool,
    opened: RefCell<Vec<TicketId>>,
}

/// Every label the GitHub board reads or writes.
const LABELS: &[&str] = &[
    "task", "epic", "ready", "claimed", "blocked", "question", "done", "door",
];

impl GithubArrange {
    /// The repository `ZAF_CONTRACT_REPO` names, its labels made; none when it is unset.
    fn repo_from_env() -> Option<String> {
        let repo = std::env::var(REPO_VAR)
            .ok()
            .filter(|repo| !repo.is_empty())?;
        for label in LABELS {
            gh(&["label", "create", label, "--force", "--repo", &repo]);
        }
        Some(repo)
    }

    /// The issues board on the repository `ZAF_CONTRACT_REPO` names; none when it is unset.
    fn from_env() -> Option<Self> {
        let repo = Self::repo_from_env()?;
        let board = GithubBoard::from(GhCommand::from(RepoName::from(repo.as_str())));
        Some(Self {
            repo,
            board: Box::new(board),
            labels: true,
            opened: RefCell::new(Vec::new()),
        })
    }

    /// The Projects board on the Project `ZAF_CONTRACT_PROJECT` names, over the repository
    /// `ZAF_CONTRACT_REPO` names; none unless both are set.
    fn projects_from_env() -> Option<Self> {
        let project = std::env::var(PROJECT_VAR)
            .ok()
            .filter(|project| !project.is_empty())?;
        let (owner, number) = project
            .split_once('/')
            .and_then(|(owner, number)| Some((owner, number.parse().ok()?)))
            .unwrap_or_else(|| panic!("{PROJECT_VAR} is `owner/number`, got `{project}`"));
        let repo = Self::repo_from_env()?;
        let board = GithubProjectBoard::on_project(
            GhCommand::from(RepoName::from(repo.as_str())),
            ProjectRef {
                owner: owner.to_owned(),
                number,
                field: DEFAULT_STATE_FIELD.to_owned(),
            },
        );
        Some(Self {
            repo,
            board: Box::new(board),
            labels: false,
            opened: RefCell::new(Vec::new()),
        })
    }

    fn open(&self, title: &str, body: &str, labels: &[&str]) -> TicketId {
        let url = gh(&[
            "issue",
            "create",
            "--repo",
            &self.repo,
            "--title",
            title,
            "--body",
            body,
            "--label",
            &labels.join(","),
        ]);
        let number: u64 = url
            .trim()
            .rsplit('/')
            .next()
            .and_then(|tail| tail.parse().ok())
            .unwrap_or_else(|| panic!("gh issue create printed no issue link: {url}"));
        let id = TicketId::from(number);
        self.opened.borrow_mut().push(id);
        id
    }
}

impl Arrange for GithubArrange {
    fn board(&self) -> &dyn Board {
        self.board.as_ref()
    }

    fn epic(&self, title: &str) -> TicketId {
        self.open(title, "An epic of the contract checks.", &["epic"])
    }

    fn task(&self, epic: TicketId, title: &str, body: &str, state: TicketState) -> TicketId {
        let labels: &[&str] = if self.labels {
            &["task", state.into()]
        } else {
            &["task"]
        };
        let task = self.open(title, body, labels);
        let issue = format!("repos/{}/issues/{}", self.repo, task.get());
        let database_id = gh(&["api", &issue, "--jq", ".id"]);
        let sub_issues = format!("repos/{}/issues/{}/sub_issues", self.repo, epic.get());
        gh(&[
            "api",
            "-X",
            "POST",
            &sub_issues,
            "-F",
            &format!("sub_issue_id={}", database_id.trim()),
        ]);
        if !self.labels {
            // Only a ready or claimed task can wait on a question, so a waiting one is ready first.
            if state == TicketState::Question {
                self.board
                    .set_state(task, TicketState::Ready)
                    .expect("the task is made ready on the Project");
            }
            self.board
                .set_state(task, state)
                .expect("the task's state is set on the Project");
        }
        task
    }

    fn close(&self, id: TicketId, completed: bool) {
        let reason = if completed {
            "completed"
        } else {
            "not planned"
        };
        let number = id.get().to_string();
        gh(&[
            "issue", "close", &number, "--repo", &self.repo, "--reason", reason,
        ]);
    }

    fn edit_body(&self, id: TicketId, body: &str) {
        let number = id.get().to_string();
        gh(&[
            "issue", "edit", &number, "--repo", &self.repo, "--body", body,
        ]);
    }

    fn adopt(&self, id: TicketId) {
        self.opened.borrow_mut().push(id);
    }

    fn missing(&self) -> TicketId {
        TicketId::from(999_999_999)
    }

    fn patience(&self) -> Duration {
        Duration::from_secs(60)
    }
}

impl Drop for GithubArrange {
    fn drop(&mut self) {
        for id in self.opened.borrow().iter() {
            let number = id.get().to_string();
            let _ = Command::new("gh")
                .args(["issue", "close", &number, "--repo", &self.repo])
                .args(["--reason", "not planned"])
                .output();
        }
    }
}

/// Runs gh and returns what it printed; a failure fails the check with gh's own words.
fn gh(args: &[&str]) -> String {
    let output = Command::new("gh")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("gh does not run: {error}"));
    assert!(
        output.status.success(),
        "gh {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// One test per check and backend: `fake::<check>` always, `github::<check>` when
/// `ZAF_CONTRACT_REPO` is set, `projects::<check>` when `ZAF_CONTRACT_PROJECT` is set too.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod fake {
            $(
                #[test]
                fn $check() {
                    super::$check(&super::FakeArrange::new());
                }
            )*
        }

        mod github {
            $(
                #[test]
                fn $check() {
                    let Some(arrange) = super::GithubArrange::from_env() else {
                        return;
                    };
                    super::$check(&arrange);
                }
            )*
        }

        mod projects {
            $(
                #[test]
                fn $check() {
                    let Some(arrange) = super::GithubArrange::projects_from_env() else {
                        return;
                    };
                    super::$check(&arrange);
                }
            )*
        }
    };
}

contract![
    a_task_reads_back_as_arranged,
    a_missing_ticket_is_not_found,
    ready_lists_ready_tasks_only_in_id_order,
    a_claim_is_held_by_one_instance,
    a_released_task_is_ready_again,
    a_new_state_replaces_the_old,
    a_ruling_closes_its_question_and_frees_the_task,
    standing_follows_how_a_ticket_closed,
    an_open_epic_lists_its_tasks_closed_ones_too,
    a_signature_holds_until_the_body_changes,
    a_challengers_report_challenges_its_epic,
    a_ticket_that_is_no_open_epic_is_not_one,
    a_filed_task_reads_back_under_its_epic,
];
