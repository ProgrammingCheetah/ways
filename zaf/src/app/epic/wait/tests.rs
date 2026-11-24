use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;

use super::*;
use crate::app::chair::{
    EpicView, InboxItem, NowView, ProblemItem, QuestionView, Square, SquareState, TroubleKind,
};
use crate::app::hub::next::NextKind;
use crate::app::hub::state::ProblemKind;
use crate::manifest::ProjectName;

const EPIC: u64 = 10;
const TASK: u64 = 11;
const OTHER_TASK: u64 = 99;

/// A fake clock: time passes only when it is slept on.
#[derive(Default)]
struct FakeClock {
    now: Cell<Duration>,
    slept: Cell<u32>,
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
    fn sleep(&self, time: Duration) {
        self.now.set(self.now.get() + time);
        self.slept.set(self.slept.get() + 1);
    }
}

/// A fake board: one state per read, the last one standing once the script is spent.
struct FakeGather {
    script: RefCell<VecDeque<Result<ChairState, String>>>,
    reads: Cell<u32>,
}

impl FakeGather {
    fn new(script: Vec<Result<ChairState, String>>) -> Self {
        Self {
            script: RefCell::new(script.into()),
            reads: Cell::new(0),
        }
    }
}

impl Gather for FakeGather {
    fn read(&self) -> Result<ChairState, String> {
        self.reads.set(self.reads.get() + 1);
        let mut script = self.script.borrow_mut();
        if script.len() > 1 {
            script.pop_front().expect("a read")
        } else {
            script.front().expect("a read").clone()
        }
    }
}

fn square(ticket: u64) -> Square {
    Square {
        ticket: TicketId::from(ticket),
        title: format!("Task {ticket}"),
        state: SquareState::Ready,
        phase: None,
        blocked_by: Vec::new(),
        question: None,
        waits: Vec::new(),
        phases_done: 0,
        phases_total: 0,
        pull_request: None,
        cost: None,
    }
}

fn epic(stage: EpicStage) -> EpicView {
    EpicView {
        number: TicketId::from(EPIC),
        title: "The epic".to_owned(),
        stage,
        half_filed: false,
        release: None,
        done: 0,
        total: 1,
        question: None,
        squares: vec![square(TASK)],
    }
}

/// A quiet project with the epic open and at `stage`.
fn state(stage: EpicStage) -> ChairState {
    ChairState {
        project: ProjectName::from("ways"),
        root: PathBuf::from("/srv/ways"),
        gathered_at: std::time::UNIX_EPOCH,
        reason: None,
        inbox: Vec::new(),
        problems: Vec::new(),
        now: NowView::default(),
        epics: vec![epic(stage)],
    }
}

fn question_on(raised_on: u64, number: u64) -> InboxItem {
    InboxItem {
        kind: NextKind::Question,
        title: format!("Question #{number}: which?"),
        blocks: 1,
        how: String::new(),
        command: None,
        link: None,
        number: Some(TicketId::from(number)),
        question: Some(QuestionView {
            text: "Which one?\nA or B.".to_owned(),
            raised_on: Some(TicketId::from(raised_on)),
            door: false,
            proposal: None,
            holds: Vec::new(),
        }),
        begins: None,
    }
}

fn problem(ticket: Option<u64>, headline: &str) -> ProblemItem {
    ProblemItem {
        kind: match ticket {
            Some(_) => TroubleKind::Orphaned,
            None => TroubleKind::Project(ProblemKind::Board),
        },
        ticket: ticket.map(TicketId::from),
        question: None,
        headline: headline.to_owned(),
        detail: headline.to_owned(),
        fix: None,
        verb: None,
        holds: 0,
    }
}

const MINUTE: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(10);

fn run(gather: &FakeGather, clock: &FakeClock, minutes: u32) -> Result<Waited, WaitError> {
    wait(gather, clock, TicketId::from(EPIC), MINUTE * minutes, EVERY)
}

#[test]
fn epic_wait_returns_on_question() {
    let mut asked = state(EpicStage::Challenged);
    asked.inbox = vec![question_on(OTHER_TASK, 300), question_on(TASK, 285)];
    let gather = FakeGather::new(vec![Ok(asked)]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 60).expect("returns");
    let Waited::Returned {
        reason, questions, ..
    } = waited
    else {
        panic!("expected a return, got {waited:?}");
    };
    assert_eq!(reason, Reason::Question);
    assert_eq!(
        gather.reads.get(),
        1,
        "a question already waiting returns at once"
    );
    assert_eq!(clock.slept.get(), 0);
    // Only the question on the epic's task, with its ticket and text.
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].question, TicketId::from(285));
    assert_eq!(questions[0].on, Some(TicketId::from(TASK)));
    assert_eq!(questions[0].render(), "#285 on #11: Which one?\n  A or B.");
}

#[test]
fn epic_wait_returns_on_problem() {
    let quiet = state(EpicStage::Challenged);
    let mut broken = quiet.clone();
    broken.problems = vec![problem(Some(TASK), "#11 orphaned")];
    let gather = FakeGather::new(vec![Ok(quiet.clone()), Ok(quiet), Ok(broken)]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 60).expect("returns");
    let Waited::Returned { reason, .. } = waited else {
        panic!("expected a return, got {waited:?}");
    };
    assert_eq!(reason, Reason::Problem);
    assert_eq!(
        gather.reads.get(),
        3,
        "two reads with nothing, the third returns"
    );
}

#[test]
fn a_standing_project_problem_does_not_end_the_wait_but_a_new_one_does() {
    let mut standing = state(EpicStage::Challenged);
    standing.problems = vec![problem(None, "the guard is red")];
    let mut worse = standing.clone();
    worse
        .problems
        .push(problem(None, "the board cannot be reached"));
    let gather = FakeGather::new(vec![Ok(standing.clone()), Ok(standing), Ok(worse)]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 60).expect("returns");
    assert!(matches!(
        waited,
        Waited::Returned {
            reason: Reason::Problem,
            ..
        }
    ));
    assert_eq!(gather.reads.get(), 3);
}

#[test]
fn a_problem_on_another_epic_does_not_end_the_wait() {
    let mut other = state(EpicStage::Challenged);
    other.problems = vec![problem(Some(OTHER_TASK), "#99 orphaned")];
    let gather = FakeGather::new(vec![Ok(other)]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 1).expect("returns");
    assert_eq!(waited, Waited::NothingYet);
}

#[test]
fn epic_wait_returns_on_done() {
    let open = state(EpicStage::Challenged);
    let mut closed = open.clone();
    closed.epics = Vec::new();
    let gather = FakeGather::new(vec![Ok(open.clone()), Ok(closed)]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 60).expect("returns");
    assert!(matches!(
        waited,
        Waited::Returned {
            reason: Reason::Done,
            ..
        }
    ));
    assert_eq!(gather.reads.get(), 2);

    // Every task done, as `epic show` counts it, is done too.
    let finished = state(EpicStage::Done);
    let gather = FakeGather::new(vec![Ok(open), Ok(finished)]);
    let waited = run(&gather, &FakeClock::default(), 60).expect("returns");
    assert!(matches!(
        waited,
        Waited::Returned {
            reason: Reason::Done,
            ..
        }
    ));
}

#[test]
fn epic_wait_times_out_with_nothing_yet() {
    let gather = FakeGather::new(vec![Ok(state(EpicStage::Challenged))]);
    let clock = FakeClock::default();
    let waited = run(&gather, &clock, 2).expect("returns");
    assert_eq!(waited, Waited::NothingYet);
    assert_eq!(
        clock.now(),
        MINUTE * 2,
        "it waited the minutes given, no more"
    );
}

#[test]
fn epic_wait_unknown_epic_refused() {
    let gather = FakeGather::new(vec![Ok(state(EpicStage::Challenged))]);
    let clock = FakeClock::default();
    let refused = wait(&gather, &clock, TicketId::from(77), MINUTE, EVERY);
    assert_eq!(refused, Err(WaitError::NotAnEpic(TicketId::from(77))));
    assert_eq!(clock.slept.get(), 0);
}

#[test]
fn a_board_that_fails_on_the_first_read_is_an_error_and_a_later_one_is_tried_again() {
    let gather = FakeGather::new(vec![Err("down".to_owned())]);
    let first = run(&gather, &FakeClock::default(), 1);
    assert_eq!(first, Err(WaitError::Board("down".to_owned())));

    let mut asked = state(EpicStage::Challenged);
    asked.inbox = vec![question_on(TASK, 285)];
    let quiet = state(EpicStage::Challenged);
    let gather = FakeGather::new(vec![
        Ok(quiet),
        Err("down".to_owned()),
        Err("down".to_owned()),
        Ok(asked),
    ]);
    let waited = run(&gather, &FakeClock::default(), 60).expect("returns");
    assert!(matches!(
        waited,
        Waited::Returned {
            reason: Reason::Question,
            ..
        }
    ));
    assert_eq!(gather.reads.get(), 4);
}
