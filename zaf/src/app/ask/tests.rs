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

fn verdict_of(outcome: &AskOutcome, index: usize) -> &Verdict {
    &outcome.answer.questions[index].verdict
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

    let outcome = scene.ask(ONE_QUESTION);

    let CheckResult::Failed(reasons) = &outcome.lines[0].check else {
        panic!("expected a failed check, got {:?}", outcome.lines[0].check);
    };
    assert!(
        reasons[0].starts_with("local: answered q2 for q1"),
        "{reasons:?}"
    );
    assert!(reasons[1].contains("1002 is superseded"), "{reasons:?}");
}

#[test]
fn every_rung_failing_is_not_settled() {
    let scene = Scene::new();
    Scene::record(&scene.local, BAD_QUOTE);
    scene.haiku.record(
        RoleName::from(KEEPER_ROLE),
        RunOutput {
            exit_code: 1,
            transcript: GOOD.to_owned(),
        },
    );

    let outcome = scene.ask(ONE_QUESTION);

    let expected = Verdict::NotSettled {
        next: "question ticket on #12 (door: no)".to_owned(),
    };
    assert_eq!(*verdict_of(&outcome, 0), expected);
    let line = &outcome.lines[0];
    assert_eq!(line.rung, None);
    assert_eq!(line.verdict, "not settled");
    let CheckResult::Failed(reasons) = &line.check else {
        panic!("expected a failed check, got {:?}", line.check);
    };
    assert_eq!(reasons.len(), 2);
    assert!(
        reasons[0].contains("the quote is not in its cited pages"),
        "{reasons:?}"
    );
    assert_eq!(reasons[1], "haiku: run exited 1");
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
    let outcome = ask(&later, &ports).expect("ask");
    let CheckResult::Failed(reasons) = &outcome.lines[0].check else {
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
    assert_eq!(raw.lines().count(), 2, "one JSON object per line");
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
    let failed = scene.ask(ONE_QUESTION);
    assert!(matches!(failed.lines[0].check, CheckResult::Failed(_)));

    Scene::record(&scene.local, GOOD);
    let retried = scene.ask(ONE_QUESTION);
    assert_eq!(
        retried.lines[0].check,
        CheckResult::Passed,
        "a failure is not served"
    );

    let other_kind = scene.ask_at(
        &["fact: Is an empty list exported as a header row only?"],
        noon(),
    );
    assert!(matches!(
        other_kind.expect("ask").lines[0].check,
        CheckResult::Failed(_)
    ));

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
