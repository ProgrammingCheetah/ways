use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use super::*;
use crate::app::hub::LineState;
use crate::app::hub::next::next_for_project;
use crate::app::hub::state::{
    EpicState, InstanceState, LineQuestion, LineTask, OrphanedClaim, Problem, ProjectState,
    QuestionState, QueueTask, ReleaseDueState, StatusState, WaitKind, WatchState,
};
use crate::domain::TicketState;

fn wait(on: WaitKind, number: u64) -> WaitState {
    WaitState {
        on,
        number: Some(number),
        commit: None,
    }
}

fn task(number: u64, state: LineState, waits: Option<Vec<WaitState>>) -> LineTask {
    LineTask {
        number,
        title: format!("Task {number}"),
        size: None,
        state,
        phase: None,
        phases_done: 0,
        phases_total: 0,
        blocked_by: Vec::new(),
        question: None,
        issue_url: None,
        waits,
        cost: None,
    }
}

fn queued(number: u64, state: TicketState) -> QueueTask {
    QueueTask {
        number,
        title: format!("Task {number}"),
        epic: 224,
        state,
        group: None,
        instance: None,
        phase: None,
        issue_url: None,
        pull_request: None,
        pull_request_url: None,
    }
}

fn question(number: u64, raised_on: u64, text: &str) -> QuestionState {
    QuestionState {
        number,
        raised_on: Some(raised_on),
        text: text.to_owned(),
        door: false,
        url: None,
    }
}

/// The ways project as the hub reads it: one epic, two questions, an orphaned claim.
fn ways() -> ProjectState {
    let mut tasks = vec![
        task(230, LineState::Working, Some(Vec::new())),
        task(
            233,
            LineState::Blocked,
            Some(vec![wait(WaitKind::Ticket, 230)]),
        ),
        task(
            234,
            LineState::Question,
            Some(vec![wait(WaitKind::Question, 285)]),
        ),
        task(235, LineState::Done, None),
        task(
            240,
            LineState::NotStarted,
            Some(vec![wait(WaitKind::Question, 285)]),
        ),
        task(241, LineState::NotStarted, Some(Vec::new())),
        task(243, LineState::Working, Some(Vec::new())),
    ];
    tasks[2].question = Some(LineQuestion {
        number: 285,
        text: "Change nothing?".to_owned(),
    });
    ProjectState {
        project: "ways".to_owned(),
        root: PathBuf::from("/srv/ways"),
        missing: false,
        gathered_at: "2026-11-19T03:00:00Z".to_owned(),
        reason: None,
        epics: vec![EpicState {
            number: 224,
            title: "the screens\u{1b}[2J and setup".to_owned(),
            stage: EpicStage::Challenged,
            half_filed: false,
            release: Some("0.5.2".to_owned()),
            url: None,
            done: 1,
            total: 7,
            dollars: 0.0,
            groups: Vec::new(),
            line_state: LineState::Working,
            line_question: None,
            tasks,
        }],
        queue: vec![
            queued(233, TicketState::Blocked),
            queued(234, TicketState::Question),
            queued(241, TicketState::Ready),
            queued(243, TicketState::Claimed),
        ],
        questions: vec![
            question(278, 235, "May this task add a tag listing?"),
            question(
                285,
                228,
                "Should zaf update change nothing?\r\n(Recommended: change nothing.)",
            ),
        ],
        problems: vec![Problem {
            kind: ProblemKind::Profile,
            detail: "profiles lean and build are older than the manifest".to_owned(),
            fix: Some("cd /srv/ways && zaf profile init".to_owned()),
            verb: Some(FixVerb::ProfileInit),
        }],
        instances: vec![InstanceState {
            instance: "team instance 1 on #224".to_owned(),
            role: "team".to_owned(),
            task: Some(243),
            number: Some(243),
            title: Some("setup plan before writing".to_owned()),
            phase: Some("implement".to_owned()),
            run_seconds: Some(95),
            last_tool_call: Some("testing".to_owned()),
        }],
        orphaned: vec![OrphanedClaim {
            task: 230,
            instance: "team instance 3 on #224".to_owned(),
        }],
        releases_due: vec![ReleaseDueState {
            version: "0.5.3".to_owned(),
            phase: "design".to_owned(),
            command: "zaf begin --version 0.5.3".to_owned(),
        }],
        watch: Some(WatchState {
            running: true,
            pid: Some(4242),
            since: None,
            zaf: None,
            version: Some("0.5.2".to_owned()),
            last_tick: None,
        }),
        last_tick: None,
        status: StatusState::default(),
    }
}

fn chair() -> ChairState {
    let project = ways();
    build(&project, &next_for_project(&project))
}

/// The chair over one epic as the hub reads it with the filing `marked` or not: the stage and
/// the half-filed flag are what `Epic::stage()` and `Epic::half_filed()` give for it.
fn filed(marked: bool) -> ChairState {
    let mut project = ways();
    let epic = &mut project.epics[0];
    epic.stage = if marked {
        EpicStage::Planned
    } else {
        EpicStage::Signed
    };
    epic.half_filed = !marked;
    build(&project, &next_for_project(&project))
}

#[test]
fn chair_half_filed_epic() {
    let state = filed(false);
    let epic = state.epic(TicketId::from(224)).expect("the epic is open");
    assert!(epic.half_filed);
    assert_eq!(epic.stage_text(), "half filed");
    assert_eq!(
        epic.way_on().as_deref(),
        Some("the tick finishes it, or `zaf plan file 224` with empty stdin")
    );
}

#[test]
fn chair_half_filed_finished() {
    let state = filed(true);
    let epic = state.epic(TicketId::from(224)).expect("the epic is open");
    assert!(!epic.half_filed);
    assert_eq!(epic.stage_text(), "planned");
    assert_eq!(epic.way_on(), None);
}

#[test]
fn chair_half_filed_unsigned_stays_unsigned() {
    let mut project = ways();
    project.epics[0].stage = EpicStage::Unsigned;
    project.epics[0].half_filed = true;
    let state = build(&project, &next_for_project(&project));
    assert_eq!(state.epics[0].stage_text(), "unsigned");
}

#[test]
fn the_inbox_is_the_next_for_you_list_most_blocking_first() {
    let state = chair();

    let inbox: Vec<(NextKind, Option<u64>, u32)> = state
        .inbox
        .iter()
        .map(|item| (item.kind, item.number.map(TicketId::get), item.blocks))
        .collect();
    // The stale profiles hold every open task; #285 holds the two tickets that wait on it; #278
    // names no waiting ticket, so it holds the one it was raised on; the 0.5.3 design has no
    // epic yet.
    assert_eq!(
        inbox,
        [
            (NextKind::Broken, None, 6),
            (NextKind::Question, Some(285), 2),
            (NextKind::Question, Some(278), 1),
            (NextKind::Phase, None, 0),
        ]
    );
    assert_eq!(state.questions(), 2);
}

#[test]
fn a_question_carries_its_whole_text_its_proposal_and_what_it_holds() {
    let state = chair();

    let item = state.question(TicketId::from(285)).expect("open");
    assert!(item.answerable());
    let question = item.question.as_ref().expect("a question");
    assert_eq!(
        question.text, "Should zaf update change nothing?\n(Recommended: change nothing.)",
        "the carriage return is gone"
    );
    assert_eq!(question.headline(), "Should zaf update change nothing?");
    assert_eq!(question.proposal.as_deref(), Some("change nothing."));
    assert_eq!(question.raised_on, Some(TicketId::from(228)));
    assert_eq!(question.holds, [TicketId::from(234), TicketId::from(240)]);
    let phase = state
        .inbox
        .iter()
        .find(|item| item.kind == NextKind::Phase)
        .expect("the due design");
    assert!(!phase.answerable());
    assert_eq!(
        phase.command.as_deref(),
        Some("cd /srv/ways && zaf begin --version 0.5.3")
    );
}

#[test]
fn problems_lead_with_what_stops_the_project() {
    let state = chair();

    let problems: Vec<(TroubleKind, Option<u64>, usize)> = state
        .problems
        .iter()
        .map(|problem| {
            (
                problem.kind,
                problem.ticket.map(TicketId::get),
                problem.holds,
            )
        })
        .collect();
    assert_eq!(
        problems,
        [
            (TroubleKind::Project(ProblemKind::Profile), None, 0),
            (TroubleKind::Orphaned, Some(230), 1),
        ]
    );
    assert!(
        state.problems[1].detail.contains("team instance 3 on #224"),
        "{}",
        state.problems[1].detail
    );
}

#[test]
fn a_failed_phase_is_a_problem_that_names_its_question() {
    let mut project = ways();
    project.questions.push(question(
        299,
        241,
        "merge got nowhere three times at d962227 (exit codes 1, 1, 1); the last line of its \
         log: \"conflict\". It is not started again until this is ruled. Last run: \
         1789000000000000000.",
    ));

    let state = build(&project, &next_for_project(&project));

    let failed = state
        .problems
        .iter()
        .find(|problem| problem.kind == TroubleKind::FailedPhase)
        .expect("a failed phase");
    assert_eq!(failed.question, Some(TicketId::from(299)));
    assert_eq!(failed.ticket, Some(TicketId::from(241)));
    assert_eq!(
        failed.headline,
        "merge got nowhere three times at d962227 (exit codes 1, 1, 1)"
    );
    let item = state
        .question(TicketId::from(299))
        .expect("in the inbox too");
    assert_eq!(item.kind, NextKind::Failed);
}

#[test]
fn squares_show_the_hub_line_states_with_orphans_failed() {
    let state = chair();

    let epic = state.epic(TicketId::from(224)).expect("open");
    assert_eq!(
        epic.title, "the screens[2J and setup",
        "no escape reaches the screen"
    );
    assert_eq!(epic.release, Some(ReleaseVersion::from("0.5.2")));
    let squares: Vec<(u64, SquareState)> = epic
        .squares
        .iter()
        .map(|square| (square.ticket.get(), square.state))
        .collect();
    assert_eq!(
        squares,
        [
            (230, SquareState::Failed),
            (233, SquareState::Waiting),
            (234, SquareState::Question),
            (235, SquareState::Done),
            (240, SquareState::Waiting),
            (241, SquareState::Ready),
            (243, SquareState::Running),
        ]
    );
    assert_eq!(epic.squares[1].waits, [wait(WaitKind::Ticket, 230)]);
    assert_eq!(SquareState::Question.word(), "waits on you");
}

#[test]
fn now_counts_from_the_queue_and_names_what_runs_and_is_due() {
    let state = chair();

    assert_eq!(state.now.ready, 1);
    assert_eq!(state.now.waiting, 2);
    assert_eq!(state.now.running.len(), 1);
    assert_eq!(state.now.running[0].ticket, Some(TicketId::from(243)));
    assert_eq!(state.now.running[0].doing.as_deref(), Some("testing"));
    assert_eq!(state.now.due[0].version, ReleaseVersion::from("0.5.3"));
    assert_eq!(
        state.now.watch,
        Some(WatchView {
            running: true,
            pid: Some(4242),
            version: Some(ReleaseVersion::from("0.5.2")),
        })
    );
    assert_eq!(
        state.gathered_at,
        humantime::parse_rfc3339("2026-11-19T03:00:00Z").expect("time")
    );
}

#[test]
fn a_hidden_question_leaves_the_inbox_and_the_rest_stays() {
    let state = chair();

    let shown = state.without(&BTreeSet::from([TicketId::from(285)]));

    assert!(shown.question(TicketId::from(285)).is_none());
    assert!(shown.question(TicketId::from(278)).is_some());
    assert_eq!(shown.inbox.len(), state.inbox.len() - 1);
    assert_eq!(shown.questions(), 1);
}

#[test]
fn a_ruling_with_no_words_is_refused_as_the_verb_refuses_it() {
    let refused = Ruling::new(PathBuf::from("/srv/ways"), TicketId::from(285), "  \n ");
    assert_eq!(refused, Err(TypedRulingError::Empty));
    let gaps = Ruling::new(
        PathBuf::from("/srv/ways"),
        TicketId::from(285),
        " Make it  so. ",
    )
    .expect("a gap is fine: it never passed a shell");
    assert_eq!(gaps.answer, RulingText::from("Make it  so."));
}

/// What the chair's actions were asked, and what they answer.
struct Recorder {
    asked: Mutex<Vec<Ruling>>,
    answer: Result<Option<TicketId>, Failure>,
}

impl ChairActions for Recorder {
    fn rule(&self, ruling: &Ruling) -> Result<Option<TicketId>, Failure> {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(ruling.clone());
        self.answer.clone()
    }

    fn fix(&self, fix: &Fix) -> Result<String, Failure> {
        Err(Failure::Failed(format!(
            "no fix in a test: {}",
            fix.command
        )))
    }

    /// Epic #1 signs now, #2 was signed before, and any other is refused as inside an instance.
    fn sign(&self, signing: &Signing) -> Result<bool, Failure> {
        match signing.epic.get() {
            1 => Ok(true),
            2 => Ok(false),
            _ => Err(Failure::Refused(
                "team instance 1 on #224 runs above this process; only the Engineer signs an \
                 epic\u{7}"
                    .to_owned(),
            )),
        }
    }
}

#[test]
fn commit_rules_through_the_actions_and_says_how_it_went() {
    let ruling = Ruling::new(PathBuf::from("/srv/ways"), TicketId::from(285), "Yes").expect("ok");
    let action = ChairAction::Rule(ruling.clone());
    let ruled = Recorder {
        asked: Mutex::new(Vec::new()),
        answer: Ok(Some(TicketId::from(273))),
    };

    let outcome = commit(&ruled, &action);

    assert_eq!(*ruled.asked.lock().expect("not poisoned"), [ruling]);
    assert!(outcome.written());
    assert_eq!(outcome.line(), "#285 ruled; #273 ready");
    assert_eq!(action.question(), Some(TicketId::from(285)));
    assert_eq!(action.done_words(), "#285 ruled");

    let refused = Recorder {
        asked: Mutex::new(Vec::new()),
        answer: Err(Failure::Refused(
            "#285 is not an open question; nothing was changed".to_owned(),
        )),
    };
    let outcome = commit(&refused, &action);
    assert!(!outcome.written());
    assert_eq!(
        outcome.line(),
        "#285 was not ruled: #285 is not an open question; nothing was changed"
    );
}

#[test]
fn a_fix_runs_the_verb_the_hub_named_never_its_text() {
    let state = build(&ways(), &[]);
    let problem = state
        .problems
        .iter()
        .find(|problem| problem.kind == TroubleKind::Project(ProblemKind::Profile))
        .expect("the stale profiles");
    assert_eq!(problem.verb, Some(FixVerb::ProfileInit));

    let fix = Fix::new(
        PathBuf::from("/srv/ways"),
        FixVerb::ProfileInit,
        "cd /etc && zaf do ship",
    );
    assert_eq!(fix.args(), ["profile", "init"], "the words are zaf's own");
    assert_eq!(ChairAction::Fix(fix).done_words(), "zaf profile init runs");
    assert_eq!(FixVerb::Doctor.args(), ["doctor"]);
}

#[test]
fn a_fix_that_fails_says_why_without_control_characters() {
    let fix = Fix::new(PathBuf::from("/srv/ways"), FixVerb::Doctor, "zaf doctor");
    let failing = Recorder {
        asked: Mutex::new(Vec::new()),
        answer: Ok(None),
    };

    let outcome = commit(&failing, &ChairAction::Fix(fix));

    assert!(!outcome.written());
    assert_eq!(outcome.question(), None);
    assert_eq!(
        outcome.line(),
        "zaf doctor failed: no fix in a test: zaf doctor"
    );
}

#[test]
fn with_no_release_due_the_inbox_offers_to_begin_the_next_version() {
    let mut project = ways();
    project.releases_due.clear();
    project.status.next_version = Some("0.5.3".to_owned());

    let state = build(&project, &next_for_project(&project));

    let offer = state.inbox.last().expect("the offer");
    assert_eq!(offer.kind, NextKind::Phase);
    assert_eq!(offer.begins, Some(ReleaseVersion::from("0.5.3")));
    assert_eq!(offer.command.as_deref(), Some("zaf begin --version 0.5.3"));
    assert_eq!(offer.title, "Begin 0.5.3: its objectives come first");
    assert_eq!(
        state.waiting(),
        state.inbox.len() - 1,
        "an offer waits on no one"
    );

    let due = chair();
    assert!(
        due.inbox.iter().all(|item| item.begins.is_none()),
        "a release due is the next step, so nothing is offered"
    );
}

#[test]
fn an_epic_with_every_task_dropped_is_a_project_problem_and_no_next_item() {
    let mut project = ways();
    project.problems = vec![Problem {
        kind: ProblemKind::EveryTaskDropped,
        detail: "epic #226 begin: every task dropped".to_owned(),
        fix: None,
        verb: None,
    }];

    let next = next_for_project(&project);
    let state = build(&project, &next);

    assert!(state.problems.iter().any(|problem| {
        problem.kind == TroubleKind::Project(ProblemKind::EveryTaskDropped)
            && problem.headline == "epic #226 begin: every task dropped"
            && problem.verb.is_none()
    }));
    assert!(next.iter().all(|item| item.kind != NextKind::Broken));
}

#[test]
fn unparsed_tasks_cycles_and_dead_blockers_are_problems() {
    let mut project = ways();
    project.status.unparsed = vec![crate::app::hub::state::UnparsedTask {
        number: 241,
        title: "Task 241".to_owned(),
        problems: vec![
            "no Scope section".to_owned(),
            "no Verify section".to_owned(),
        ],
    }];
    project.status.cycles = vec![vec![233, 240]];
    project.status.dead_blockers = vec![crate::app::spawn::blockers::DeadLine {
        blocker: 199,
        on: 243,
        reason: "closed as not planned".to_owned(),
    }];

    let state = build(&project, &next_for_project(&project));

    let tasks: Vec<(TroubleKind, Option<u64>, &str)> = state
        .problems
        .iter()
        .filter(|problem| matches!(problem.kind, TroubleKind::Task(_)))
        .map(|problem| {
            (
                problem.kind,
                problem.ticket.map(TicketId::get),
                problem.headline.as_str(),
            )
        })
        .collect();
    // Most held first, then by ticket: none here holds another.
    assert_eq!(
        tasks,
        [
            (
                TroubleKind::Task(TaskTrouble::Cycle),
                Some(233),
                "cycle #233 → #240 → #233"
            ),
            (
                TroubleKind::Task(TaskTrouble::Unparsed),
                Some(241),
                "not to the template: no Scope section"
            ),
            (
                TroubleKind::Task(TaskTrouble::DeadBlocker),
                Some(243),
                "dead blocker #199: closed as not planned"
            ),
        ]
    );
    let unparsed = state
        .problems
        .iter()
        .find(|problem| problem.kind == TroubleKind::Task(TaskTrouble::Unparsed))
        .expect("the unparsed task");
    assert!(unparsed.detail.contains("no Verify section"));
    assert!(unparsed.detail.contains("zaf board lint 241"));
}

#[test]
fn now_carries_the_slots_and_the_guard() {
    let mut project = ways();
    project.status.slots = Some(crate::app::hub::state::SlotsState {
        total: 4,
        in_use: 4,
    });

    let state = build(&project, &next_for_project(&project));

    assert_eq!(
        state.now.slots,
        Some(SlotsView {
            total: 4,
            in_use: 4
        })
    );
    assert_eq!(state.now.guard, None, "no guard computed yet");
    assert_eq!(
        state.now.budget, None,
        "the machine's budget is the source's"
    );
}

fn signing(epic: u64, last: bool) -> ChairAction {
    ChairAction::Sign(Signing {
        root: PathBuf::from("/srv/ways"),
        epic: TicketId::from(epic),
        version: Some(ReleaseVersion::from("0.5.2")),
        last,
    })
}

#[test]
fn commit_signs_through_the_actions_and_says_when_the_build_may_start() {
    let actions = Recorder {
        asked: Mutex::new(Vec::new()),
        answer: Ok(None),
    };

    let signed: Vec<Outcome> = [signing(1, true), signing(2, true), signing(1, false)]
        .iter()
        .map(|action| commit(&actions, action))
        .collect();

    let lines: Vec<String> = signed.iter().map(Outcome::line).collect();
    assert_eq!(
        lines,
        [
            "epic #1 signed; every epic of 0.5.2 is signed: b starts the build",
            "epic #2 was already signed",
            "epic #1 signed; zaf plans its tickets",
        ]
    );
    assert!(signed.iter().all(Outcome::written));
    assert_eq!(signing(1, true).undone_words(), "epic #1 not signed");
}

#[test]
fn a_refused_signature_says_why_without_control_characters() {
    let actions = Recorder {
        asked: Mutex::new(Vec::new()),
        answer: Ok(None),
    };

    let outcome = commit(&actions, &signing(3, true));

    assert!(!outcome.written());
    assert_eq!(
        outcome.line(),
        "epic #3 was not signed: team instance 1 on #224 runs above this process; only the \
         Engineer signs an epic"
    );
}

#[test]
fn a_refused_ruling_says_why_without_control_characters() {
    let action = ChairAction::Rule(
        Ruling::new(PathBuf::from("/srv/ways"), TicketId::from(285), "Yes").expect("words"),
    );
    for failure in [
        Failure::Refused("closed\x1b[2J by \u{202e}someone".to_owned()),
        Failure::Failed("the board\x07 is down".to_owned()),
    ] {
        let board = Recorder {
            asked: Mutex::new(Vec::new()),
            answer: Err(failure),
        };

        let line = commit(&board, &action).line();

        assert!(
            !line.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{line:?}"
        );
    }
}

#[test]
fn chair_squares_graph_waits() {
    // Fixture 12's epic on the fake board: C and D wait on the interface ticket #104 until it is
    // done, though no body has a `Blocked by:` line.
    use crate::app::actions::SliceRecords;
    use crate::app::hub::state::{StateInputs, gather_state};
    use crate::app::report::PriceTable;
    use crate::fakes::{FakeBoard, FakeForge, FakeVcs, add_fixture_12};

    let dir = tempfile::tempdir().expect("tempdir");
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(100), "The graph");
    add_fixture_12(&board, TicketId::from(100), 101);
    let (forge, vcs) = (FakeForge::default(), FakeVcs::default());
    let records = SliceRecords::in_state_dir(dir.path());
    let prices = PriceTable::default();
    let squares_now = || {
        let project = gather_state(&StateInputs {
            lists: None,
            project: "example",
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
            releases: None,
        });
        let state = build(&project, &next_for_project(&project));
        let epic = state.epic(TicketId::from(100)).expect("open");
        epic.squares
            .iter()
            .map(|square| (square.ticket.get(), square.state, square.blocked_by.clone()))
            .collect::<Vec<_>>()
    };

    let squares = squares_now();

    let interface = vec![TicketId::from(104)];
    assert_eq!(
        squares[1..3],
        [
            (102, SquareState::Waiting, interface.clone()),
            (103, SquareState::Waiting, interface),
        ]
    );
    assert_eq!(squares[0].1, SquareState::Ready);

    board.close(TicketId::from(104));
    let squares = squares_now();
    assert_eq!(squares[1].1, SquareState::Ready, "the interface is done");
    assert_eq!(squares[2].1, SquareState::Ready, "the interface is done");
}
