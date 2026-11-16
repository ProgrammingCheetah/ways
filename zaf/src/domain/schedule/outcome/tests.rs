use super::super::tests::*;
use super::*;
use crate::domain::schedule::{PhaseEnd, PullFacts, RuledMarker, RunExit, end_fact_holds};

/// The pack's ticket at `h1` with a green check, and its review phase (ends at `exit`).
fn reviewed() -> (TicketFacts, Phase) {
    let mut facts = facts(pack());
    facts.pull = Some(PullFacts {
        checks: vec![passed(24)],
        ..pull("h1", 20)
    });
    let review = facts.phases[2].clone();
    (facts, review)
}

/// A report of a run that exited `code` at 26 and left `head`.
fn report(code: i32, head: Option<&str>) -> Report {
    Report {
        exit: Some(RunExit {
            code: ExitCode::from(code),
            at: time(26),
        }),
        questions: std::collections::BTreeSet::new(),
        head: head.map(commit),
    }
}

#[test]
fn an_open_question_the_run_raised_comes_before_everything() {
    let (mut facts, review) = reviewed();
    facts.open_questions.insert(id(900));
    let mut asked = report(0, Some("h1"));
    asked.questions.insert(id(900));
    assert_eq!(
        outcome(&asked, &review, &facts),
        Outcome::WaitOn(WaitingOn::Question(id(900)))
    );
    // A question the run raised that is ruled is no wait, and no done either.
    facts.open_questions.clear();
    assert!(matches!(
        outcome(&asked, &review, &facts),
        Outcome::Failed(_)
    ));
}

#[test]
fn the_end_fact_holding_at_the_head_is_done_reading_the_report_as_a_run() {
    let (facts, review) = reviewed();
    assert!(!end_fact_holds(&review, &facts));
    assert_eq!(
        outcome(&report(0, Some("h1")), &review, &facts),
        Outcome::Done
    );
    let implement = facts.phases[0].clone();
    assert_eq!(
        outcome(&report(1, Some("h1")), &implement, &facts),
        Outcome::Done,
        "the pull request changes a file, whatever the exit"
    );
}

#[test]
fn checks_that_have_not_ended_are_a_wait_on_the_check() {
    let (mut facts, review) = reviewed();
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![pending()];
    }
    assert_eq!(
        outcome(&report(1, Some("h1")), &review, &facts),
        Outcome::WaitOn(on_check("h1"))
    );
}

#[test]
fn exit_zero_of_a_phase_awaiting_the_outside_waits_unless_a_check_failed() {
    let (mut facts, _) = reviewed();
    let posted = phase("review", &[PhaseEnd::ReviewPosted], Some("fix"));
    assert_eq!(
        outcome(&report(0, Some("h1")), &posted, &facts),
        Outcome::WaitOn(on_check("h1"))
    );
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![failed(25)];
    }
    let Outcome::Failed(failure) = outcome(&report(0, Some("h1")), &posted, &facts) else {
        panic!("a red check fails it");
    };
    assert_eq!(failure.checks, [CheckName::from("ci")]);
    assert_eq!(failure.at, Some(time(25)));
}

#[test]
fn anything_else_failed_on_the_exit_and_the_head() {
    let (facts, review) = reviewed();
    assert_eq!(
        outcome(&report(1, Some("h1")), &review, &facts),
        Outcome::Failed(Failure {
            head: Some(commit("h1")),
            checks: Vec::new(),
            exit: Some(ExitCode::from(1)),
            at: Some(time(26)),
        })
    );
    // Exit 0 at another head than the pull request's is no end at its head.
    assert!(matches!(
        outcome(&report(0, Some("h0")), &review, &facts),
        Outcome::Failed(_)
    ));
}

#[test]
fn the_machinery_is_no_exit_125_and_128_up() {
    assert!(is_machinery(None));
    for code in [125, 128, 137, 143, 255] {
        assert!(is_machinery(Some(ExitCode::from(code))), "{code}");
    }
    for code in [0, 1, 2, 124, 126, 127, -1] {
        assert!(!is_machinery(Some(ExitCode::from(code))), "{code}");
    }
    let stopped = Failure {
        head: None,
        checks: Vec::new(),
        exit: Some(ExitCode::from(143)),
        at: None,
    };
    assert!(stopped.by_machinery());
    let red = Failure {
        checks: vec![CheckName::from("ci")],
        ..stopped
    };
    assert!(!red.by_machinery());
}

/// The pack's ticket at `h1`, red, with `runs` of fix.
fn red_with(runs: Vec<RunRecord>) -> TicketFacts {
    let mut facts = facts(pack());
    facts.pull = Some(PullFacts {
        checks: vec![failed(30)],
        ..pull("h1", 20)
    });
    facts.runs = runs;
    facts
}

fn fix(started: u64, code: Option<i32>, left: Option<&str>) -> RunRecord {
    let run = run("fix", Some("h1"), started).leaves(left);
    match code {
        Some(code) => run.exits(code, started + 5),
        None => run,
    }
}

#[test]
fn nowhere_counts_the_ended_runs_that_began_and_ended_at_the_head() {
    let facts = red_with(vec![
        fix(31, Some(0), Some("h1")),
        fix(41, Some(1), Some("h1")),
        fix(51, Some(0), Some("h2")),
        run("fix", Some("h1"), 61).exits(0, 66),
        run("review", Some("h1"), 71)
            .exits(1, 76)
            .leaves(Some("h1")),
        fix(81, None, Some("h1")).lives(),
    ]);
    let counted: Vec<SystemTime> = nowhere(&facts, &name("fix"))
        .iter()
        .map(|run| run.started_at)
        .collect();
    assert_eq!(counted, [time(31), time(41)]);
    assert!(!gets_nowhere(&facts, &name("fix")));
}

#[test]
fn a_machinery_failure_never_counts() {
    let facts = red_with(vec![
        fix(31, Some(125), Some("h1")),
        fix(41, Some(143), Some("h1")),
        fix(51, None, Some("h1")),
        fix(61, Some(1), Some("h1")),
    ]);
    assert_eq!(nowhere(&facts, &name("fix")).len(), 1);
}

#[test]
fn a_ruling_lets_it_try_again() {
    let mut facts = red_with(vec![
        fix(31, Some(0), Some("h1")),
        fix(41, Some(0), Some("h1")),
        fix(51, Some(0), Some("h1")),
    ]);
    assert_eq!(NOWHERE_LIMIT, 3);
    assert!(gets_nowhere(&facts, &name("fix")));
    facts.ruled.push(RuledMarker {
        action: name("fix"),
        last_run: time(51),
    });
    assert!(nowhere(&facts, &name("fix")).is_empty());
    facts.runs.push(fix(61, Some(1), Some("h1")));
    assert_eq!(nowhere(&facts, &name("fix")).len(), 1);
    // A marker of another action counts for nothing here.
    facts.ruled = vec![RuledMarker {
        action: name("implement"),
        last_run: time(100),
    }];
    assert_eq!(nowhere(&facts, &name("fix")).len(), 4);
}

#[test]
fn with_no_pull_request_the_runs_at_no_head_count() {
    let mut facts = facts(pack());
    for started in [5, 15, 25] {
        facts.runs.push(
            run("implement", None, started)
                .exits(0, started + 5)
                .leaves(None),
        );
    }
    assert!(gets_nowhere(&facts, &name("implement")));
}

#[test]
fn an_on_fail_answers_a_red_check_exit_one_and_exit_zero_alone() {
    let failure = |checks: Vec<CheckName>, exit: Option<i32>| Failure {
        head: None,
        checks,
        exit: exit.map(ExitCode::from),
        at: None,
    };
    assert!(failure(vec![CheckName::from("ci")], Some(2)).for_on_fail());
    assert!(failure(vec![CheckName::from("ci")], None).for_on_fail());
    assert!(failure(Vec::new(), Some(1)).for_on_fail());
    assert!(
        failure(Vec::new(), Some(0)).for_on_fail(),
        "a ruled question"
    );
    for code in [2, 3, 4, 125, 143, -1] {
        assert!(!failure(Vec::new(), Some(code)).for_on_fail(), "{code}");
    }
    assert!(!failure(Vec::new(), None).for_on_fail());
    assert_eq!(ExitCode::FAILURE, ExitCode::from(1));
}
