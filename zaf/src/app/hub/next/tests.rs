use std::path::PathBuf;

use super::*;
use crate::app::hub::LineState;
use crate::app::hub::state::{
    EpicState, MachineLine, Problem, ProblemKind, ProjectState, QuestionState, QueueTask,
    WatchState,
};
use crate::domain::{Budget, EpicStage, TicketState, Units};

fn project(name: &str) -> ProjectState {
    ProjectState {
        project: name.to_owned(),
        root: PathBuf::from(format!("/srv/{name}")),
        missing: false,
        gathered_at: "2026-11-13T10:00:00Z".to_owned(),
        reason: None,
        epics: Vec::new(),
        queue: Vec::new(),
        questions: Vec::new(),
        problems: Vec::new(),
        instances: Vec::new(),
        watch: Some(watching(true)),
        last_tick: None,
    }
}

fn watching(running: bool) -> WatchState {
    WatchState {
        running,
        pid: None,
        since: None,
        zaf: None,
        version: None,
        last_tick: None,
    }
}

fn machine(projects: Vec<ProjectState>) -> MachineState {
    MachineState {
        machine: MachineLine {
            instances: 0,
            projects: projects.len(),
            in_use: Units::default(),
            budget: Budget::default(),
        },
        projects,
    }
}

fn epic(number: u64, stage: EpicStage, done: u32, total: u32) -> EpicState {
    EpicState {
        number,
        title: format!("Epic {number}"),
        stage,
        release: None,
        url: Some(format!("https://board.invalid/issues/{number}")),
        done,
        total,
        dollars: 0.0,
        groups: Vec::new(),
        line_state: LineState::NotStarted,
        line_question: None,
        tasks: Vec::new(),
    }
}

fn question(number: u64, raised_on: Option<u64>, text: &str, door: bool) -> QuestionState {
    QuestionState {
        number,
        raised_on,
        text: text.to_owned(),
        door,
        url: Some(format!("https://board.invalid/issues/{number}")),
    }
}

fn ready(number: u64) -> QueueTask {
    QueueTask {
        number,
        title: format!("Task {number}"),
        epic: 1,
        state: TicketState::Ready,
        group: None,
        instance: None,
        phase: None,
        issue_url: None,
        pull_request: None,
        pull_request_url: None,
    }
}

/// A getting-nowhere question as the scheduler raises it.
const NOWHERE: &str = "merge got nowhere three times at d962227 (exit codes 1, 1, 1); the last \
                       line of its log: \"conflict\". It is not started again until this is \
                       ruled. Last run: 1789000000000000000.";

#[test]
fn sign_item_command() {
    let mut ways = project("ways");
    ways.epics.push(epic(7, EpicStage::Unsigned, 0, 4));
    ways.epics.push(epic(8, EpicStage::Challenged, 1, 3));

    let list = next_for_you(&machine(vec![ways]));

    assert_eq!(list.len(), 1);
    let item = &list[0];
    assert_eq!(item.kind, NextKind::Sign);
    assert_eq!(item.project, "ways");
    assert_eq!(item.title, "Sign epic #7: Epic 7");
    assert_eq!(item.blocks, 4);
    assert_eq!(
        item.command.as_deref(),
        Some("cd /srv/ways && zaf epic sign 7")
    );
    assert_eq!(item.link.as_deref(), Some("https://board.invalid/issues/7"));
}

#[test]
fn sign_command_quotes_a_folder_with_a_space() {
    let mut ways = project("ways");
    ways.root = PathBuf::from("/srv/my ways");
    ways.epics.push(epic(7, EpicStage::Unsigned, 0, 1));

    let list = next_for_you(&machine(vec![ways]));

    assert_eq!(
        list[0].command.as_deref(),
        Some("cd '/srv/my ways' && zaf epic sign 7")
    );
}

#[test]
fn question_with_proposal_and_door() {
    let mut ways = project("ways");
    ways.epics.push(epic(1, EpicStage::Challenged, 2, 5));
    ways.questions.push(question(
        20,
        Some(1),
        "Keep the cache?\n**Claude's answer (2026-11-13; the Engineer adjusts):** Keep it, it is cheap.",
        false,
    ));
    ways.questions.push(question(
        21,
        Some(9),
        "Drop the v1 endpoint?\nProposed answer: Drop it after 0.5.0.",
        true,
    ));
    ways.questions
        .push(question(22, Some(9), "Which name?", false));

    let list = next_for_you(&machine(vec![ways]));

    let by_number = |number: u64| {
        list.iter()
            .find(|item| item.title.starts_with(&format!("Question #{number}:")))
            .expect("the question is listed")
    };
    let proposed = by_number(20);
    assert_eq!(proposed.kind, NextKind::Question);
    assert_eq!(proposed.title, "Question #20: Keep the cache?");
    assert_eq!(
        proposed.blocks, 3,
        "a question on an epic holds its open tasks"
    );
    assert!(
        proposed.how.contains("Keep it, it is cheap"),
        "{}",
        proposed.how
    );
    assert_eq!(
        proposed.command.as_deref(),
        Some("cd /srv/ways && zaf board ruling 20 'Keep it, it is cheap.'")
    );
    assert_eq!(
        proposed.link.as_deref(),
        Some("https://board.invalid/issues/20")
    );

    let door = by_number(21);
    assert_eq!(door.kind, NextKind::Door);
    assert_eq!(door.blocks, 1, "a question on a task holds that task");
    assert!(door.how.contains("one-way"), "{}", door.how);
    assert!(door.how.contains("Drop it after 0.5.0"), "{}", door.how);

    let bare = by_number(22);
    assert_eq!(bare.kind, NextKind::Question);
    assert!(bare.how.contains("no answer"), "{}", bare.how);
    assert_eq!(
        bare.command.as_deref(),
        Some("cd /srv/ways && zaf board ruling 22 '<ruling>'")
    );
}

#[test]
fn proposal_is_read_from_the_lines_that_name_one() {
    assert_eq!(proposal_of("Which?"), None);
    assert_eq!(proposal_of("Which?\nProposal:"), None);
    assert_eq!(
        proposal_of("Which?\n- Proposal: ship it\nmore"),
        Some("ship it".to_owned())
    );
    assert_eq!(
        proposal_of("**Claude's answer (2026-11-13; ok):** Yes, because."),
        Some("Yes, because.".to_owned())
    );
    assert_eq!(
        proposal_of("A proposal for later: nothing"),
        None,
        "a line must open with the words"
    );
}

#[test]
fn ready_unticked() {
    let mut ways = project("ways");
    ways.watch = Some(watching(false));
    ways.queue.push(ready(12));
    ways.queue.push(ready(9));
    let mut ticking = project("zur");
    ticking.queue.push(ready(3));
    let mut unread = project("gone");
    unread.watch = None;
    unread.queue.push(ready(4));

    let list = next_for_you(&machine(vec![ways, ticking, unread]));

    assert_eq!(list.len(), 1, "only the project no watch ticks: {list:?}");
    let item = &list[0];
    assert_eq!(item.kind, NextKind::Unticked);
    assert_eq!(item.project, "ways");
    assert_eq!(item.blocks, 2);
    assert_eq!(item.title, "2 ready tasks in ways: no scheduler ticks");
    assert_eq!(
        item.command.as_deref(),
        Some("cd /srv/ways && zaf spawn --watch --version <version>")
    );
}

#[test]
fn blocked_work_is_not_ready_work() {
    let mut ways = project("ways");
    ways.queue.push(ready(9));
    ways.watch = Some(watching(false));
    ways.queue[0].state = TicketState::Blocked;

    assert_eq!(next_for_you(&machine(vec![ways])), []);
}

#[test]
fn failed_three_times() {
    let mut ways = project("ways");
    ways.epics.push(epic(1, EpicStage::Challenged, 0, 3));
    ways.questions.push(question(45, Some(8), NOWHERE, false));

    let list = next_for_you(&machine(vec![ways]));

    assert_eq!(list.len(), 1);
    let item = &list[0];
    assert_eq!(item.kind, NextKind::Failed);
    assert_eq!(
        item.title,
        "Question #45 on #8: merge got nowhere three times at d962227 (exit codes 1, 1, 1)"
    );
    assert_eq!(item.blocks, 1);
    assert!(item.how.contains("not started again"), "{}", item.how);
    assert_eq!(
        item.command.as_deref(),
        Some("cd /srv/ways && zaf board ruling 45 '<ruling>'")
    );
    assert_eq!(
        item.link.as_deref(),
        Some("https://board.invalid/issues/45")
    );
}

#[test]
fn project_cannot_run() {
    let mut token = project("zur");
    token.epics.push(epic(2, EpicStage::Challenged, 1, 6));
    token.reason = Some("the board cannot be reached: the variable `FORGEJO_TOKEN`".to_owned());
    token.problems.push(Problem {
        kind: ProblemKind::Token,
        detail: "the variable `FORGEJO_TOKEN` that holds its token is not set".to_owned(),
        fix: Some("export FORGEJO_TOKEN=<token>".to_owned()),
    });
    let mut stale = project("ways");
    stale.problems.push(Problem {
        kind: ProblemKind::Profile,
        detail: "profile lean is of another profile generation (written by zaf 0.3.0)".to_owned(),
        fix: Some("cd /srv/ways && zaf profile init".to_owned()),
    });
    let mut gone = project("old");
    gone.missing = true;
    gone.watch = None;
    gone.problems.push(Problem {
        kind: ProblemKind::Missing,
        detail: "its folder is gone".to_owned(),
        fix: None,
    });

    let list = next_for_you(&machine(vec![token, stale, gone]));

    assert_eq!(list.len(), 3);
    let first = &list[0];
    assert_eq!(first.kind, NextKind::Broken);
    assert_eq!(first.project, "zur");
    assert_eq!(
        first.blocks, 5,
        "the tasks of its open epics still to finish"
    );
    assert_eq!(
        first.command.as_deref(),
        Some("export FORGEJO_TOKEN=<token>")
    );
    assert!(
        first.how.contains("export FORGEJO_TOKEN=<token>"),
        "{}",
        first.how
    );
    assert_eq!(list[1].project, "ways");
    assert_eq!(
        list[1].command.as_deref(),
        Some("cd /srv/ways && zaf profile init")
    );
    assert_eq!(list[2].project, "old");
    assert_eq!(list[2].command, None);
    assert!(list[2].how.contains("Restore it"), "{}", list[2].how);
}

#[test]
fn next_for_you_has_one_item_for_stale_profiles() {
    let mut stale = project("ways");
    stale.problems.push(Problem {
        kind: ProblemKind::Profile,
        detail: "profiles build and lean are of another profile generation (written by zaf 0.3.0)"
            .to_owned(),
        fix: Some("cd /srv/ways && zaf profile init".to_owned()),
    });

    let list = next_for_you(&machine(vec![stale]));

    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].kind, NextKind::Broken);
    assert_eq!(list[0].project, "ways");
    assert!(
        list[0]
            .title
            .contains("profiles build and lean are of another profile generation"),
        "{}",
        list[0].title
    );
    assert_eq!(
        list[0].command.as_deref(),
        Some("cd /srv/ways && zaf profile init")
    );
}

#[test]
fn ordered_by_blocking() {
    let mut ways = project("ways");
    ways.epics.push(epic(1, EpicStage::Unsigned, 0, 2));
    ways.epics.push(epic(2, EpicStage::Challenged, 0, 6));
    ways.questions.push(question(30, Some(2), "Which?", false));
    ways.questions.push(question(31, Some(5), "Else?", false));
    let mut zur = project("zur");
    zur.watch = Some(watching(false));
    zur.queue.push(ready(4));
    zur.queue.push(ready(5));

    let list = next_for_you(&machine(vec![ways, zur]));

    let order: Vec<(NextKind, u32)> = list.iter().map(|item| (item.kind, item.blocks)).collect();
    assert_eq!(
        order,
        [
            (NextKind::Question, 6),
            // Equal at two: the sign item's epic (#1) is older than the ready task (#4), and the
            // question on task #5 is #31.
            (NextKind::Sign, 2),
            (NextKind::Unticked, 2),
            (NextKind::Question, 1),
        ],
        "{list:#?}"
    );
}

#[test]
fn ties_break_by_the_older_issue() {
    let mut ways = project("ways");
    ways.questions.push(question(40, Some(3), "Later?", false));
    ways.questions
        .push(question(12, Some(3), "Earlier?", false));

    let list = next_for_you(&machine(vec![ways]));

    assert_eq!(list[0].title, "Question #12: Earlier?");
    assert_eq!(list[1].title, "Question #40: Later?");
}

#[test]
fn one_of_each_in_one_project() {
    let mut ways = project("ways");
    ways.watch = Some(watching(false));
    ways.epics.push(epic(1, EpicStage::Unsigned, 0, 3));
    ways.questions.push(question(20, Some(9), "Which?", false));
    ways.queue.push(ready(9));

    let list = next_for_you(&machine(vec![ways]));

    let kinds: Vec<NextKind> = list.iter().map(|item| item.kind).collect();
    // Equal at one: the ready task (#9) is older than the question (#20).
    assert_eq!(
        kinds,
        [NextKind::Sign, NextKind::Unticked, NextKind::Question]
    );
    assert_eq!(list[0].blocks, 3, "the item blocking more tasks first");
}

#[test]
fn empty_when_nothing_waits() {
    let mut ways = project("ways");
    ways.epics.push(epic(1, EpicStage::Challenged, 1, 3));
    let mut idle = project("zur");
    idle.watch = Some(watching(false));

    assert_eq!(next_for_you(&machine(vec![ways, idle])), []);
    assert_eq!(next_for_you(&machine(Vec::new())), []);
}

#[test]
fn nowhere_is_told_from_any_other_question() {
    assert!(is_nowhere(NOWHERE));
    assert!(!is_nowhere("Which got nowhere? Last run: 1."));
    assert!(!is_nowhere("merge got nowhere three times at d962227"));
}

/// The number a title names after `prefix`: `Sign epic #7: …` is 7.
fn numbered(title: &str, prefix: &str) -> u64 {
    title
        .trim_start_matches(prefix)
        .split(':')
        .next()
        .and_then(|number| number.parse().ok())
        .expect("a number")
}

/// What `zaf status` and `zaf epic list` show waiting on the Engineer is what the list holds: the
/// unsigned epics and the open questions, the door marked, read from one board.
#[test]
fn matches_what_status_and_epic_list_show() {
    use crate::app::actions::SliceRecords;
    use crate::app::epic;
    use crate::app::hub::state::{StateInputs, gather_state};
    use crate::app::report::PriceTable;
    use crate::domain::{Ticket, TicketId};
    use crate::fakes::{FakeBoard, FakeForge, FakeVcs};
    use crate::ports::{Board, NewQuestion};

    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_epic(TicketId::from(100), "The hub");
    for number in 1..=3 {
        board.add_ticket(Ticket {
            id: TicketId::from(number),
            epic: Some(TicketId::from(100)),
            title: format!("Task {number}").into(),
            body: "".into(),
            state: Some(TicketState::Blocked),
            url: "".into(),
            kind: None,
        });
    }
    let door = board
        .open_question(&NewQuestion {
            on: TicketId::from(2),
            text: "Drop the old page?".to_owned(),
            door: true,
        })
        .expect("question");
    board
        .open_question(&NewQuestion {
            on: TicketId::from(3),
            text: "Which colour?".to_owned(),
            door: false,
        })
        .expect("question");
    let (forge, vcs, records, prices) = (
        FakeForge::default(),
        FakeVcs::default(),
        SliceRecords::in_state_dir(dir.path()),
        PriceTable::default(),
    );
    let state = gather_state(&StateInputs {
        project: "ways",
        root: dir.path(),
        state_dir: dir.path(),
        board: Ok(&board),
        forge: Some(&forge),
        vcs: Some(&vcs),
        records: &records,
        ledger: &[],
        prices: &prices,
        pull_base: None,
        alive: &|_| true,
        now: std::time::SystemTime::UNIX_EPOCH,
        places: None,
        gathered: None,
    });

    let list = next_for_you(&machine(vec![state]));

    // What the two verbs show waiting.
    let unsigned: Vec<u64> = epic::list(&board)
        .expect("epic list")
        .iter()
        .filter(|line| line.stage == EpicStage::Unsigned)
        .map(|line| line.number.get())
        .collect();
    let asked: Vec<(u64, bool)> = board
        .questions()
        .expect("questions")
        .iter()
        .map(|ticket| (ticket.id.get(), ticket.id == door))
        .collect();
    // What the list holds.
    let signs: Vec<u64> = list
        .iter()
        .filter(|item| item.kind == NextKind::Sign)
        .map(|item| numbered(&item.title, "Sign epic #"))
        .collect();
    let mut questions: Vec<(u64, bool)> = list
        .iter()
        .filter(|item| matches!(item.kind, NextKind::Question | NextKind::Door))
        .map(|item| {
            (
                numbered(&item.title, "Question #"),
                item.kind == NextKind::Door,
            )
        })
        .collect();
    questions.sort_unstable();
    assert_eq!(unsigned, [100]);
    assert_eq!(signs, unsigned);
    assert_eq!(questions, asked);
    assert!(questions.contains(&(door.get(), true)));
    assert_eq!(list.len(), 3, "{list:#?}");
}
