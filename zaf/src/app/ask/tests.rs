use super::*;
use crate::domain::{PageStatus, TaskLabel};
use crate::fakes::{FakeBoard, FakeDesign, FakeRunner};
use crate::ports::{RunOutput, Ticket};

const BODY: &str = "\
Goal: Members export their reading list as CSV.
Owned paths: src/export/**
Interfaces: none
Design pages: 1001 (decided)
Architecture: app over ports
Acceptance:
  - an empty list exports the header row only
Tests: beside the use case
Security trigger: no
Model: sonnet
Size: S
Blocked by: —
";

const EXPORT_PAGE: &str =
    "On export, an empty list exports the header\nrow only. Columns follow the list.";

const GOOD: &str = "\
q1: settled
    answer: An empty list exports the header row and nothing else.
    pages: 1001 (decided)
    quote: \"an empty list exports the header row only\"
";

const BAD_QUOTE: &str = "\
q1: settled
    answer: An empty list exports nothing.
    pages: 1001 (decided)
    quote: \"an empty list exports nothing\"
";

const ONE_QUESTION: &[&str] = &["rule: Is an empty list exported as a header row only?"];

struct Scene {
    state: tempfile::TempDir,
    board: FakeBoard,
    design: FakeDesign,
    local: FakeRunner,
    haiku: FakeRunner,
}

impl Scene {
    fn new() -> Self {
        let board = FakeBoard::default();
        board.add_ticket(Ticket {
            number: task(),
            epic: Some(IssueNumber::from(10)),
            title: "Export a reading list".to_owned(),
            body: BODY.to_owned(),
            label: Some(TaskLabel::Claimed),
        });
        let design = FakeDesign::default();
        design.add_page(page("1001", PageStatus::Decided, EXPORT_PAGE));
        design.add_page(page(
            "1002",
            PageStatus::Superseded,
            "An empty list exports nothing.",
        ));
        Self {
            state: tempfile::tempdir().expect("temp dir"),
            board,
            design,
            local: FakeRunner::default(),
            haiku: FakeRunner::default(),
        }
    }

    fn record(runner: &FakeRunner, transcript: &str) {
        runner.record(
            RoleName::from(KEEPER_ROLE),
            RunOutput {
                exit_code: 0,
                transcript: transcript.to_owned(),
            },
        );
    }

    fn ports(&self) -> AskPorts<'_> {
        AskPorts {
            board: &self.board,
            design: Some(&self.design),
            ladder: vec![
                LadderRung {
                    name: Rung::from("local"),
                    runner: Ok(&self.local),
                },
                LadderRung {
                    name: Rung::from("haiku"),
                    runner: Ok(&self.haiku),
                },
            ],
        }
    }

    fn ask_at(&self, questions: &[&str], now: SystemTime) -> Result<AskOutcome, AskFailure> {
        let questions: Vec<String> = questions.iter().map(|&text| text.to_owned()).collect();
        let query = AskQuery {
            task: task(),
            from: "programmer-3",
            questions: &questions,
            max: 5,
            root: Path::new("/project"),
            state_dir: self.state.path(),
            cache_window: Duration::from_secs(DEFAULT_CACHE_HOURS * 3600),
            now,
        };
        ask(&query, &self.ports())
    }

    fn ask(&self, questions: &[&str]) -> AskOutcome {
        self.ask_at(questions, noon()).expect("ask")
    }

    fn log(&self) -> Vec<LogLine> {
        read_log(&self.state.path().join(ASK_LOG)).expect("log")
    }
}

fn task() -> IssueNumber {
    IssueNumber::from(12)
}

fn noon() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn page(id: &str, status: PageStatus, text: &str) -> Page {
    Page {
        id: id.parse().expect("page id"),
        title: format!("Page {id}"),
        status,
        text: text.to_owned(),
    }
}

#[test]
fn a_verified_answer_on_the_first_rung_is_returned() {
    let scene = Scene::new();
    Scene::record(&scene.local, GOOD);

    let outcome = scene.ask(ONE_QUESTION);

    assert_eq!(outcome.answer, GOOD.parse::<Answer>().expect("answer"));
    assert_eq!(outcome.lines[0].rung.as_deref(), Some("local"));
    assert_eq!(outcome.lines[0].check, CheckResult::Passed);
    assert!(
        scene.haiku.requests().is_empty(),
        "the ladder stops at the first pass"
    );

    let requests = scene.local.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.role, RoleName::from(KEEPER_ROLE));
    assert_eq!(request.profile, Profile::Lean);
    assert!(
        request
            .dir
            .starts_with(scene.state.path().join(RUNS_DIR).join("programmer-3"))
    );
    assert!(request.dir.is_dir());
    let prompt = &request.prompt;
    assert!(
        prompt.starts_with("# The domain keeper"),
        "the default prompt"
    );
    assert!(prompt.contains("for: #12"));
    assert!(prompt.contains("context: Export a reading list"));
    assert!(prompt.contains("q1: rule  | Is an empty list exported as a header row only?"));
    let pages_at = prompt
        .find("### 1001 Page 1001 (decided)")
        .expect("named page");
    assert!(prompt.find("## The ask").expect("ask") < pages_at);
    assert!(prompt.contains(EXPORT_PAGE));
    assert!(
        !prompt.contains("1002"),
        "only the named pages are preloaded"
    );
}

#[test]
fn run_folders_are_named_by_the_askers_slug() {
    let scene = Scene::new();
    Scene::record(&scene.local, GOOD);
    let questions = [ONE_QUESTION[0].to_owned()];
    let query = AskQuery {
        task: task(),
        from: "Team instance 12 on #16",
        questions: &questions,
        max: 5,
        root: Path::new("/project"),
        state_dir: scene.state.path(),
        cache_window: Duration::from_secs(DEFAULT_CACHE_HOURS * 3600),
        now: noon(),
    };
    ask(&query, &scene.ports()).expect("ask");
    let requests = scene.local.requests();
    let folder = scene
        .state
        .path()
        .join(RUNS_DIR)
        .join("team-instance-12-on-16");
    assert!(
        requests[0].dir.starts_with(&folder),
        "{:?}",
        requests[0].dir
    );
    assert!(requests[0].dir.is_dir());
}

#[test]
fn a_failed_quote_falls_to_the_second_rung() {
    let scene = Scene::new();
    Scene::record(&scene.local, BAD_QUOTE);
    Scene::record(&scene.haiku, GOOD);

    let outcome = scene.ask(ONE_QUESTION);

    assert_eq!(outcome.answer, GOOD.parse::<Answer>().expect("answer"));
    assert_eq!(outcome.lines[0].rung.as_deref(), Some("haiku"));
    assert_eq!(scene.local.requests().len(), 1);
    assert_eq!(scene.haiku.requests().len(), 1);
}

#[test]
fn a_malformed_answer_falls_through() {
    let scene = Scene::new();
    Scene::record(&scene.local, "Probably yes, the header row only.");
    Scene::record(&scene.haiku, GOOD);

    let outcome = scene.ask(ONE_QUESTION);

    assert_eq!(outcome.lines[0].rung.as_deref(), Some("haiku"));
    assert_eq!(outcome.lines[0].check, CheckResult::Passed);
}

#[test]
fn an_answer_to_other_questions_or_an_uncitable_page_falls_through() {
    let scene = Scene::new();
    Scene::record(&scene.local, &GOOD.replace("q1:", "q2:"));
    Scene::record(
        &scene.haiku,
        "q1: settled\n    answer: Nothing.\n    pages: 1002 (decided)\n    quote: \"An empty list exports nothing.\"\n",
    );

    let Err(AskFailure::Exhausted(reasons)) = scene.ask_at(ONE_QUESTION, noon()) else {
        panic!("expected every rung to fail");
    };
    assert!(
        reasons[0].starts_with("local: answered q2 for q1"),
        "{reasons:?}"
    );
    assert!(reasons[1].contains("1002 is superseded"), "{reasons:?}");
}

#[test]
fn every_rung_failing_fails_the_ask_and_logs_a_failed_line_per_question() {
    let scene = Scene::new();
    let both_bad = "\
q1: settled
    answer: An empty list exports nothing.
    pages: 1001 (decided)
    quote: \"an empty list exports nothing\"
q2: settled
    answer: A shelf is a list.
    pages: 1001 (decided)
    quote: \"a shelf is a list\"
";
    Scene::record(&scene.local, both_bad);
    scene.haiku.record(
        RoleName::from(KEEPER_ROLE),
        RunOutput {
            exit_code: 1,
            transcript: GOOD.to_owned(),
        },
    );

    let result = scene.ask_at(
        &[
            "rule: Is an empty list exported as a header row only?",
            "term: What is a shelf?",
        ],
        noon(),
    );

    let Err(AskFailure::Exhausted(reasons)) = result else {
        panic!("expected every rung to fail");
    };
    assert_eq!(reasons.len(), 2);
    assert!(
        reasons[0].contains("the quote is not in its cited pages"),
        "{reasons:?}"
    );
    assert_eq!(reasons[1], "haiku: run exited 1");

    let log = scene.log();
    assert_eq!(log.len(), 2, "one line per question");
    assert_eq!(log[0].kind, "rule");
    assert_eq!(log[1].kind, "term");
    for line in &log {
        assert_eq!(line.verdict, "failed");
        assert_eq!(line.task, 12);
        assert_eq!(line.rung, None);
        assert!(line.pages.is_empty());
        assert!(
            line.answer
                .starts_with("no rung gave a verified answer: local: "),
            "{}",
            line.answer
        );
        assert!(matches!(line.check, CheckResult::Failed(_)));
    }
}

#[test]
fn a_failed_ask_is_not_served_from_the_log_and_the_next_ask_runs_again() {
    let scene = Scene::new();
    Scene::record(&scene.local, BAD_QUOTE);
    Scene::record(&scene.haiku, BAD_QUOTE);
    assert!(scene.ask_at(ONE_QUESTION, noon()).is_err());

    Scene::record(&scene.local, GOOD);
    let outcome = scene.ask(ONE_QUESTION);

    assert_eq!(outcome.lines[0].check, CheckResult::Passed);
    let verdicts: Vec<String> = scene.log().into_iter().map(|line| line.verdict).collect();
    assert_eq!(verdicts, ["failed", "settled"]);
}

#[test]
fn a_second_ask_on_another_task_appends_its_own_lines() {
    let scene = Scene::new();
    let other = IssueNumber::from(13);
    scene.board.add_ticket(Ticket {
        number: other,
        epic: Some(IssueNumber::from(10)),
        title: "Import a reading list".to_owned(),
        body: BODY.to_owned(),
        label: Some(TaskLabel::Claimed),
    });
    let not_settled = "q1: not settled\n    next: question ticket on #12 (door: no)\n";
    Scene::record(&scene.local, not_settled);
    let first = scene.ask(ONE_QUESTION);
    assert_eq!(first.lines[0].verdict, "not settled");

    Scene::record(&scene.local, not_settled);
    let questions = vec!["rule: Is a missing file an error?".to_owned()];
    let query = AskQuery {
        task: other,
        from: "programmer-3",
        questions: &questions,
        max: 5,
        root: Path::new("/project"),
        state_dir: scene.state.path(),
        cache_window: Duration::from_secs(DEFAULT_CACHE_HOURS * 3600),
        now: noon() + Duration::from_secs(170),
    };
    ask(&query, &scene.ports()).expect("second ask");

    let log = scene.log();
    assert_eq!(log.len(), 2, "both asks are in the log");
    assert_eq!(log[0].task, 12);
    assert_eq!(log[1].task, 13);
    assert_eq!(log[1].time, 1_800_000_170);
    assert_eq!(log[1].verdict, "not settled");
    assert_eq!(scene.local.requests().len(), 2);
}

/// A runner that reads the ask log while the keeper "runs".
struct LogReader {
    path: PathBuf,
    seen: std::cell::RefCell<Vec<LogLine>>,
}

impl Runner for LogReader {
    fn run(&self, _request: &RunRequest) -> Result<RunOutput, PortError> {
        *self.seen.borrow_mut() = read_log(&self.path).expect("log");
        Ok(RunOutput {
            exit_code: 0,
            transcript: GOOD.to_owned(),
        })
    }
}

#[test]
fn the_questions_are_in_the_log_before_the_keeper_runs() {
    let scene = Scene::new();
    let reader = LogReader {
        path: scene.state.path().join(ASK_LOG),
        seen: std::cell::RefCell::default(),
    };
    let mut ports = scene.ports();
    ports.ladder[0].runner = Ok(&reader);
    let questions = vec![ONE_QUESTION[0].to_owned()];
    let query = AskQuery {
        task: task(),
        from: "programmer-3",
        questions: &questions,
        max: 5,
        root: Path::new("/project"),
        state_dir: scene.state.path(),
        cache_window: Duration::from_secs(3600),
        now: noon(),
    };

    ask(&query, &ports).expect("ask");

    let during = reader.seen.borrow();
    assert_eq!(during.len(), 1, "a run cut off here would leave this line");
    assert_eq!(during[0].verdict, "failed");
    assert_eq!(during[0].answer, INTERRUPTED);
    assert_eq!(
        during[0].question,
        "Is an empty list exported as a header row only?"
    );

    let after = scene.log();
    assert_eq!(after.len(), 1, "the answer supersedes the marker");
    assert_eq!(after[0].verdict, "settled");
    let raw = fs::read_to_string(scene.state.path().join(ASK_LOG)).expect("log file");
    assert_eq!(raw.lines().count(), 2, "marker, then the answer");
}

#[test]
fn a_run_cut_off_before_it_answered_leaves_a_failed_line() {
    let scene = Scene::new();
    let marker = LogLine {
        time: 1_800_000_000,
        task: 49,
        from: "team instance 4 on #37".to_owned(),
        kind: "rule".to_owned(),
        question: "Is it so?".to_owned(),
        verdict: FAILED.to_owned(),
        pages: Vec::new(),
        answer: INTERRUPTED.to_owned(),
        rung: None,
        check: CheckResult::Failed(vec![INTERRUPTED.to_owned()]),
    };
    let json = serde_json::to_string(&marker).expect("json");
    fs::write(scene.state.path().join(ASK_LOG), format!("{json}\n")).expect("log");

    assert_eq!(scene.log(), [marker]);
}

#[test]
fn a_missing_runner_or_design_source_fails_only_its_rung() {
    let scene = Scene::new();
    Scene::record(&scene.haiku, GOOD);
    let mut ports = scene.ports();
    ports.ladder[0].runner = Err("no [runners.local]".to_owned());
    let questions = vec![ONE_QUESTION[0].to_owned()];
    let query = AskQuery {
        task: task(),
        from: "programmer-3",
        questions: &questions,
        max: 5,
        root: Path::new("/project"),
        state_dir: scene.state.path(),
        cache_window: Duration::from_secs(3600),
        now: noon(),
    };

    let outcome = ask(&query, &ports).expect("ask");
    assert_eq!(outcome.lines[0].rung.as_deref(), Some("haiku"));

    Scene::record(&scene.haiku, GOOD);
    ports.design = None;
    let later = AskQuery {
        now: noon() + Duration::from_secs(7200),
        ..query
    };
    let Err(AskFailure::Exhausted(reasons)) = ask(&later, &ports) else {
        panic!("a quote cannot be checked without a design source");
    };
    assert!(reasons[1].contains("no design source"), "{reasons:?}");
}

#[test]
fn one_log_line_is_written_per_question() {
    let scene = Scene::new();
    let two = "\
q1: settled
    answer: An empty list exports the header row and nothing else.
    pages: 1001 (decided)
    quote: \"an empty list exports the header row only\"
q2: not settled
    next: question ticket on #12 (door: no)
";
    Scene::record(&scene.local, two);

    let outcome = scene.ask(&[
        "rule: Is an empty list exported as a header row only?",
        "term: What is a shelf?",
    ]);

    let log = scene.log();
    assert_eq!(log, outcome.lines);
    assert_eq!(log.len(), 2);
    let first = &log[0];
    assert_eq!(first.time, 1_800_000_000);
    assert_eq!(first.task, 12);
    assert_eq!(first.from, "programmer-3");
    assert_eq!(first.kind, "rule");
    assert_eq!(
        first.question,
        "Is an empty list exported as a header row only?"
    );
    assert_eq!(first.verdict, "settled");
    assert_eq!(first.pages, ["1001"]);
    assert!(first.answer.starts_with("q1: settled\n"));
    assert_eq!(first.rung.as_deref(), Some("local"));
    assert_eq!(first.check, CheckResult::Passed);
    assert_eq!(log[1].kind, "term");
    assert_eq!(log[1].verdict, "not settled");
    assert!(log[1].pages.is_empty());

    let raw = fs::read_to_string(scene.state.path().join(ASK_LOG)).expect("log file");
    assert_eq!(
        raw.lines().count(),
        4,
        "one JSON object per line: a marker for each question, then each answer"
    );
}

#[test]
fn a_repeated_question_is_served_from_the_log() {
    let scene = Scene::new();
    Scene::record(&scene.local, GOOD);
    let first = scene.ask(ONE_QUESTION);

    let an_hour_later = noon() + Duration::from_secs(3600);
    let again = scene.ask_at(ONE_QUESTION, an_hour_later).expect("ask");

    assert_eq!(again.answer, first.answer);
    assert_eq!(again.lines[0].check, CheckResult::Cached);
    assert_eq!(again.lines[0].rung.as_deref(), Some("local"));
    assert_eq!(scene.local.requests().len(), 1, "no second run");
    assert_eq!(scene.log().len(), 2, "the cached ask is logged too");
}

#[test]
fn only_fresh_passed_answers_to_the_same_question_are_served() {
    let scene = Scene::new();
    Scene::record(&scene.local, BAD_QUOTE);
    Scene::record(&scene.haiku, BAD_QUOTE);
    assert!(scene.ask_at(ONE_QUESTION, noon()).is_err());

    Scene::record(&scene.local, GOOD);
    let retried = scene.ask(ONE_QUESTION);
    assert_eq!(
        retried.lines[0].check,
        CheckResult::Passed,
        "a failure is not served"
    );

    Scene::record(&scene.local, BAD_QUOTE);
    let other_kind = scene.ask_at(
        &["fact: Is an empty list exported as a header row only?"],
        noon(),
    );
    assert!(other_kind.is_err(), "another kind is not served");

    Scene::record(&scene.local, GOOD);
    let two_days_later = noon() + Duration::from_secs(48 * 3600);
    let stale = scene.ask_at(ONE_QUESTION, two_days_later).expect("ask");
    assert_eq!(
        stale.lines[0].check,
        CheckResult::Passed,
        "a stale answer is re-asked"
    );
}

#[test]
fn only_the_questions_the_log_cannot_answer_reach_the_keeper() {
    let scene = Scene::new();
    Scene::record(&scene.local, GOOD);
    scene.ask(ONE_QUESTION);
    Scene::record(
        &scene.local,
        "q2: not settled\n    next: question ticket on #12 (door: no)\n",
    );

    let outcome = scene.ask(&[ONE_QUESTION[0], "term: What is a shelf?"]);

    let prompt = &scene.local.requests()[1].prompt;
    assert!(
        !prompt.contains("q1: rule"),
        "the cached question is not re-asked"
    );
    assert!(prompt.contains("q2: term  | What is a shelf?"));
    assert_eq!(outcome.lines[0].check, CheckResult::Cached);
    assert_eq!(outcome.lines[1].check, CheckResult::Passed);
    let ids: Vec<String> = outcome
        .answer
        .questions
        .iter()
        .map(|entry| entry.id.to_string())
        .collect();
    assert_eq!(ids, ["q1", "q2"]);
}

#[test]
fn the_keeper_prompt_can_be_overridden() {
    let scene = Scene::new();
    let roles = scene.state.path().join("roles");
    fs::create_dir_all(&roles).expect("roles");
    fs::write(roles.join("keeper.md"), "# A project keeper\n").expect("override");
    Scene::record(&scene.local, GOOD);

    scene.ask(ONE_QUESTION);

    let prompt = &scene.local.requests()[0].prompt;
    assert!(prompt.starts_with("# A project keeper\n\n## The ask"));
}

#[test]
fn malformed_asks_are_refused_before_any_run() {
    let scene = Scene::new();
    let too_many = ["fact: a?"; 6];
    assert!(matches!(
        scene.ask_at(&too_many, noon()),
        Err(AskFailure::TooMany { given: 6, max: 5 })
    ));
    assert!(matches!(
        scene.ask_at(&["no kind here"], noon()),
        Err(AskFailure::Question(_))
    ));
    assert!(matches!(
        scene.ask_at(&["opinion: Is it nice?"], noon()),
        Err(AskFailure::Ask(AskError::Kind(_)))
    ));
    assert!(scene.local.requests().is_empty());
    assert!(scene.log().is_empty());
}

const CITES_UNREADABLE: &str = "\
q1: settled
    answer: An empty list exports the header row and nothing else.
    pages: 1003 (decided)
    quote: \"an empty list exports the header row only\"
";

const CHECK_FAILED_LINE: &str =
    "q1: check failed — page 1003 could not be read (design dir or index misconfigured)\n";

/// A design source whose index lists page 1003 but cannot read it: the misconfigured
/// design dir of the 2026-11-01 report. With `index_fails`, the index itself errors.
struct Unreadable<'a> {
    inner: &'a FakeDesign,
    index_fails: bool,
}

impl Unreadable<'_> {
    fn is_broken(id: &PageId) -> bool {
        id.as_ref() == "1003"
    }
}

impl Design for Unreadable<'_> {
    fn page(&self, id: &PageId) -> Result<Page, PortError> {
        if Self::is_broken(id) {
            return Err(PortError::Unavailable(format!("design source: page {id}")));
        }
        self.inner.page(id)
    }

    fn search(&self, query: &str) -> Result<Vec<PageId>, PortError> {
        self.inner.search(query)
    }

    fn status(&self, id: &PageId) -> Result<PageStatus, PortError> {
        if self.index_fails && Self::is_broken(id) {
            return Err(PortError::Unavailable("design source: index".to_owned()));
        }
        self.inner.status(id)
    }
}

/// Asks `questions` through `design`.
fn ask_through(
    scene: &Scene,
    design: &dyn Design,
    questions: &[&str],
) -> Result<AskOutcome, AskFailure> {
    let mut ports = scene.ports();
    ports.design = Some(design);
    let questions: Vec<String> = questions.iter().map(|&text| text.to_owned()).collect();
    let query = AskQuery {
        task: task(),
        from: "programmer-3",
        questions: &questions,
        max: 5,
        root: Path::new("/project"),
        state_dir: scene.state.path(),
        cache_window: Duration::from_secs(3600),
        now: noon(),
    };
    ask(&query, &ports)
}

fn scene_with_page_1003() -> Scene {
    let scene = Scene::new();
    scene.design.add_page(page(
        "1003",
        PageStatus::Decided,
        "An empty list exports the header row only.",
    ));
    scene
}

#[test]
fn a_cited_page_that_cannot_be_read_is_a_check_failure_not_a_not_settled() {
    let scene = scene_with_page_1003();
    Scene::record(&scene.local, CITES_UNREADABLE);
    Scene::record(&scene.haiku, CITES_UNREADABLE);
    let design = Unreadable {
        inner: &scene.design,
        index_fails: false,
    };

    let outcome =
        ask_through(&scene, &design, ONE_QUESTION).expect("a check failure is an outcome");

    assert!(outcome.has_check_failure());
    assert_eq!(outcome.render(), CHECK_FAILED_LINE);
    assert_eq!(scene.local.requests().len(), 1, "the ladder was climbed");
    assert_eq!(scene.haiku.requests().len(), 1);
    let line = &outcome.lines[0];
    assert_eq!(line.verdict, CHECK_FAILED);
    assert_eq!(line.rung, None);
    assert_eq!(
        line.check,
        CheckResult::Failed(vec!["page 1003 could not be read".to_owned()])
    );
    assert_eq!(scene.log(), outcome.lines, "the marker is superseded");
}

#[test]
fn an_index_that_cannot_be_read_is_a_check_failure_too() {
    let scene = scene_with_page_1003();
    Scene::record(&scene.local, CITES_UNREADABLE);
    Scene::record(&scene.haiku, CITES_UNREADABLE);
    let design = Unreadable {
        inner: &scene.design,
        index_fails: true,
    };

    let outcome = ask_through(&scene, &design, ONE_QUESTION).expect("outcome");

    assert_eq!(outcome.render(), CHECK_FAILED_LINE);
}

#[test]
fn a_page_the_index_does_not_know_is_a_wrong_citation_not_a_check_failure() {
    let scene = Scene::new();
    let hallucinated = CITES_UNREADABLE.replace("1003", "1999");
    Scene::record(&scene.local, &hallucinated);
    Scene::record(&scene.haiku, &hallucinated);

    let Err(AskFailure::Exhausted(reasons)) = scene.ask_at(ONE_QUESTION, noon()) else {
        panic!("a page nobody indexed is the keeper's fault");
    };
    assert!(reasons[0].contains("1999"), "{reasons:?}");
}

#[test]
fn a_rejected_answer_beside_an_unreadable_page_still_exhausts_the_ladder() {
    let scene = scene_with_page_1003();
    Scene::record(&scene.local, CITES_UNREADABLE);
    Scene::record(&scene.haiku, BAD_QUOTE);
    let design = Unreadable {
        inner: &scene.design,
        index_fails: false,
    };

    assert!(matches!(
        ask_through(&scene, &design, ONE_QUESTION),
        Err(AskFailure::Exhausted(_))
    ));
}

#[test]
fn a_not_settled_with_no_citations_is_printed_as_before_and_is_no_failure() {
    let scene = Scene::new();
    let silent = "q1: not settled\n    next: question ticket on #12 (door: no)\n";
    Scene::record(&scene.local, silent);

    let outcome = scene.ask(ONE_QUESTION);

    assert!(!outcome.has_check_failure());
    assert_eq!(outcome.render(), silent);
    assert_eq!(outcome.lines[0].verdict, "not settled");
    assert_eq!(outcome.lines[0].check, CheckResult::Passed);
}

#[test]
fn a_question_with_sound_citations_in_a_discarded_answer_names_the_asks_unreadable_pages() {
    let scene = scene_with_page_1003();
    let two = format!(
        "{CITES_UNREADABLE}q2: settled\n    answer: A shelf is a list.\n    pages: 1001 (decided)\n    quote: \"Columns follow the list\"\n"
    );
    Scene::record(&scene.local, &two);
    Scene::record(&scene.haiku, &two);
    let design = Unreadable {
        inner: &scene.design,
        index_fails: false,
    };

    let outcome = ask_through(
        &scene,
        &design,
        &[ONE_QUESTION[0], "term: What is a shelf?"],
    )
    .expect("outcome");

    assert_eq!(
        outcome.render(),
        format!(
            "{CHECK_FAILED_LINE}{}",
            CHECK_FAILED_LINE.replace("q1", "q2")
        )
    );
}
