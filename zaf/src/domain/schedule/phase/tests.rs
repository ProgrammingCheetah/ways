use std::str::FromStr;

use super::super::tests::*;
use super::*;
use crate::domain::schedule::{CheckName, ExitCode, PullFacts, RunRecord};

/// The pack's ticket with a pull request at `h1` opened at 20.
fn with_pull() -> TicketFacts {
    let mut facts = facts(pack());
    facts.pull = Some(pull("h1", 20));
    facts
}

fn edit_pull(facts: &mut TicketFacts, edit: impl FnOnce(&mut PullFacts)) {
    if let Some(pull) = facts.pull.as_mut() {
        edit(pull);
    }
}

#[test]
fn the_ends_read_and_print_as_the_metadata_writes_them() {
    for (text, end) in [
        ("pr-open", PhaseEnd::PrOpen),
        ("review-posted", PhaseEnd::ReviewPosted),
        ("ci-done", PhaseEnd::CiDone),
        ("merged", PhaseEnd::Merged),
        ("exit", PhaseEnd::Exit),
    ] {
        assert_eq!(PhaseEnd::from_str(text), Ok(end));
        assert_eq!(end.to_string(), text);
    }
}

#[test]
fn pr_open_holds_only_for_a_pull_request_that_changes_a_file() {
    let implement = phase("implement", &[PhaseEnd::PrOpen], None);
    let mut facts = facts(pack());
    assert!(!end_fact_holds(&implement, &facts));
    facts.pull = Some(PullFacts {
        changed_files: crate::domain::ChangedFiles::default(),
        ..pull("h1", 20)
    });
    assert!(!end_fact_holds(&implement, &facts));
    edit_pull(&mut facts, |pull| {
        pull.changed_files = crate::domain::ChangedFiles::of(&["src/lib.rs"]);
    });
    assert!(end_fact_holds(&implement, &facts));
}

#[test]
fn pr_open_does_not_hold_while_the_last_run_raised_a_question() {
    let implement = phase("implement", &[PhaseEnd::PrOpen], None);
    let mut facts = with_pull();
    assert!(end_fact_holds(&implement, &facts), "opened by hand");

    // The run pushed a stand-in and asked: the phase is not ended, ruled or not.
    facts.runs.push(
        run("implement", None, 10)
            .exits(0, 15)
            .leaves(Some("h1"))
            .asks(71),
    );
    assert!(!end_fact_holds(&implement, &facts));

    // A later run that asked nothing ends it; a run of another action changes nothing.
    facts
        .runs
        .push(run("fix", Some("h1"), 30).exits(0, 35).asks(72));
    facts.runs.push(
        run("implement", Some("h1"), 40)
            .exits(0, 45)
            .leaves(Some("h1")),
    );
    assert!(end_fact_holds(&implement, &facts));
}

#[test]
fn review_posted_holds_only_for_a_review_made_on_the_head() {
    let posted = phase("review", &[PhaseEnd::ReviewPosted], None);
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| {
        pull.reviews = vec![review(None, 30), review(Some("h0"), 31)];
    });
    assert!(!end_fact_holds(&posted, &facts));
    edit_pull(&mut facts, |pull| pull.reviews.push(review(Some("h1"), 32)));
    assert!(end_fact_holds(&posted, &facts));
}

#[test]
fn ci_done_holds_once_every_check_at_the_head_passed() {
    let ci = phase("ci", &[PhaseEnd::CiDone], None);
    let mut facts = with_pull();
    assert!(!end_fact_holds(&ci, &facts));
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(30), pending()]);
    assert!(!end_fact_holds(&ci, &facts));
    edit_pull(&mut facts, |pull| {
        pull.checks = vec![passed(30), passed(31)]
    });
    assert!(end_fact_holds(&ci, &facts));
}

#[test]
fn merged_holds_once_merged() {
    let merge = phase("merge", &[PhaseEnd::Merged], None);
    let mut facts = with_pull();
    assert!(!end_fact_holds(&merge, &facts));
    edit_pull(&mut facts, |pull| pull.merged_at = Some(time(60)));
    assert!(end_fact_holds(&merge, &facts));
}

#[test]
fn exit_holds_for_a_run_that_left_the_head_with_exit_zero_and_no_question() {
    let review = phase("review", &[PhaseEnd::Exit], None);
    let holds = |run: RunRecord| {
        let mut facts = with_pull();
        facts.runs.push(run);
        end_fact_holds(&review, &facts)
    };
    assert!(holds(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1"))
    ));
    // An old folder's run is read at the head it started at.
    assert!(holds(run("review", Some("h1"), 25).exits(0, 26)));
    assert!(!holds(
        run("review", Some("h0"), 25)
            .exits(0, 26)
            .leaves(Some("h0"))
    ));
    assert!(!holds(
        run("review", Some("h1"), 25)
            .exits(1, 26)
            .leaves(Some("h1"))
    ));
    assert!(!holds(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1"))
            .asks(900)
    ));
    assert!(!holds(run("review", Some("h1"), 25).leaves(Some("h1"))));
    assert!(!holds(
        run("fix", Some("h1"), 25).exits(0, 26).leaves(Some("h1"))
    ));
    // A run that left the head the pull request moved to holds there.
    assert!(holds(
        run("review", Some("h0"), 25)
            .exits(0, 26)
            .leaves(Some("h1"))
    ));
}

/// The list of a ticket with a brief before implement: brief and review end at `exit`.
fn briefed() -> Vec<Phase> {
    vec![
        phase("brief", &[PhaseEnd::Exit], None),
        phase("implement", &[PhaseEnd::PrOpen], None),
        phase("ci", &[PhaseEnd::CiDone], Some("fix")),
        phase("review", &[PhaseEnd::Exit], Some("fix")),
        phase("merge", &[PhaseEnd::Merged], None),
    ]
}

#[test]
fn a_fix_push_reopens_the_review_and_neither_the_brief_nor_implement_before_the_pull_request() {
    let list = briefed();
    let mut facts = facts(list.clone());
    facts
        .runs
        .push(run("brief", None, 2).exits(0, 3).leaves(None));
    facts
        .runs
        .push(run("implement", None, 4).exits(0, 10).leaves(Some("h1")));
    facts.pull = Some(PullFacts {
        checks: vec![passed(12)],
        ..pull("h1", 8)
    });
    facts.runs.push(
        run("review", Some("h1"), 13)
            .exits(0, 14)
            .leaves(Some("h1")),
    );
    let current =
        |facts: &TicketFacts| current_phase(facts).map(|current| current.phase.name.clone());
    assert_eq!(
        current(&facts),
        Some(name("merge")),
        "at h1 all but merge ended"
    );

    // A fix moves the head to h2, and the checks there pass.
    facts
        .runs
        .push(run("fix", Some("h1"), 15).exits(0, 16).leaves(Some("h2")));
    edit_pull(&mut facts, |pull| {
        pull.head = Some(commit("h2"));
        pull.checks = vec![passed(18)];
    });
    let [brief, implement, ci, review, _] = &list[..] else {
        panic!("five phases");
    };
    // The brief ran before there was a head: listed before `pr-open`, any head does.
    assert!(end_fact_holds(brief, &facts), "the brief stays ended");
    assert!(end_fact_holds(implement, &facts), "implement stays ended");
    assert!(end_fact_holds(ci, &facts));
    // The review is listed after `pr-open`: its run left h1, and the head is h2.
    assert!(
        !end_fact_holds(review, &facts),
        "the push reopens the review"
    );
    assert_eq!(current(&facts), Some(name("review")));
}

#[test]
fn an_exit_never_holds_for_a_run_that_raised_a_question_open_or_ruled() {
    let list = briefed();
    let (brief, review) = (&list[0], &list[3]);
    let mut facts = facts(list.clone());
    facts
        .runs
        .push(run("brief", None, 2).exits(0, 3).leaves(None).asks(900));
    facts.pull = Some(PullFacts {
        checks: vec![passed(12)],
        ..pull("h1", 8)
    });
    facts.runs.push(
        run("review", Some("h1"), 13)
            .exits(0, 14)
            .leaves(Some("h1"))
            .asks(901),
    );
    facts.open_questions.extend([id(900), id(901)]);
    assert!(!end_fact_holds(brief, &facts), "its question is open");
    assert!(!end_fact_holds(review, &facts), "its question is open");
    // Ruled, before the pull request and after it alike: the phase runs again with the ruling.
    facts.open_questions.clear();
    assert!(!end_fact_holds(brief, &facts), "its question was ruled");
    assert!(!end_fact_holds(review, &facts), "its question was ruled");
    assert_eq!(
        current_phase(&facts).map(|current| current.phase.name.clone()),
        Some(name("brief"))
    );
    // A later run that raised none ends it.
    facts
        .runs
        .push(run("brief", None, 15).exits(0, 16).leaves(None));
    assert!(end_fact_holds(brief, &facts));
    // A list with no `pr-open` phase keys every `exit` on the head, as before.
    let keyed = vec![phase("brief", &[PhaseEnd::Exit], None)];
    let mut facts = self::facts(keyed.clone());
    facts
        .runs
        .push(run("brief", None, 2).exits(0, 3).leaves(None));
    facts.pull = Some(pull("h1", 8));
    assert!(!end_fact_holds(&keyed[0], &facts));
}

#[test]
fn a_folded_phase_ends_when_its_last_end_holds() {
    let folded = phase("implement", &[PhaseEnd::Exit, PhaseEnd::PrOpen], None);
    let mut facts = facts(vec![folded.clone()]);
    facts
        .runs
        .push(run("implement", None, 5).exits(0, 30).leaves(Some("h1")));
    assert!(!end_fact_holds(&folded, &facts));
    facts.pull = Some(pull("h1", 20));
    assert!(end_fact_holds(&folded, &facts));
    assert_eq!(ended_at(&folded, &facts, None), Some(time(30)));
}

#[test]
fn a_phase_with_no_end_never_ends() {
    let endless = phase("implement", &[], None);
    let facts = facts(vec![endless.clone()]);
    assert!(!end_fact_holds(&endless, &facts));
    assert_eq!(
        current_phase(&facts).map(|current| current.phase.name.clone()),
        Some(name("implement"))
    );
}

#[test]
fn the_current_phase_is_the_first_whose_end_does_not_hold_with_when_the_one_before_ended() {
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(30)]);
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.phase.name, name("review"));
    assert_eq!(current.status, PhaseStatus::Open);
    assert_eq!(current.previous, Some(time(30)));
    assert_eq!(current.next(), &name("review"));
    edit_pull(&mut facts, |pull| pull.merged_at = Some(time(60)));
    facts.runs.push(run("review", Some("h1"), 40).exits(0, 41));
    assert_eq!(current_phase(&facts), None);
    assert_eq!(current_phase(&self::facts(Vec::new())), None);
}

#[test]
fn a_failed_phase_runs_its_on_fail_next_and_one_with_none_runs_again() {
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![failed(30)]);
    let current = current_phase(&facts).expect("a current phase");
    assert!(matches!(current.status, PhaseStatus::Failed(_)));
    assert_eq!(current.next(), &name("fix"));
    facts.phases[1].on_fail = None;
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.next(), &name("ci"));
}

#[test]
fn a_run_the_machinery_ended_is_not_counted_so_the_phase_runs_again() {
    let review = phase("review", &[PhaseEnd::Exit], Some("fix"));
    for ended in [
        run("review", Some("h1"), 25).leaves(Some("h1")),
        run("review", Some("h1"), 25)
            .exits(125, 26)
            .leaves(Some("h1")),
        run("review", Some("h1"), 25)
            .exits(143, 26)
            .leaves(Some("h1")),
    ] {
        let mut facts = with_pull();
        edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
        facts.runs.push(ended);
        assert_eq!(phase_status(&review, &facts), PhaseStatus::Open);
    }
}

#[test]
fn the_latest_counted_run_at_the_head_decides() {
    let review = phase("review", &[PhaseEnd::Exit], Some("fix"));
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(1, 26)
            .leaves(Some("h1")),
    );
    facts.runs.push(
        run("review", Some("h1"), 27)
            .exits(143, 28)
            .leaves(Some("h1")),
    );
    let PhaseStatus::Failed(failure) = phase_status(&review, &facts) else {
        panic!("the exit 1 run decides");
    };
    assert_eq!(failure.exit, Some(ExitCode::from(1)));
    assert_eq!(failure.at, Some(time(26)));
}

#[test]
fn a_phase_awaiting_the_outside_after_exit_zero_awaits_the_check() {
    let posted = phase("review", &[PhaseEnd::ReviewPosted], Some("fix"));
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1")),
    );
    assert_eq!(
        phase_status(&posted, &facts),
        PhaseStatus::Awaiting(on_check("h1"))
    );
    assert!(posted.awaits_the_outside());
    assert!(!phase("implement", &[PhaseEnd::PrOpen], None).awaits_the_outside());
    assert!(!phase("review", &[PhaseEnd::Exit], None).awaits_the_outside());
}

#[test]
fn a_red_check_with_no_run_fails_the_phase_on_its_checks() {
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![failed(30)]);
    let PhaseStatus::Failed(failure) = phase_status(&facts.phases[1], &facts) else {
        panic!("a red check fails it");
    };
    assert_eq!(failure.checks, [CheckName::from("ci")]);
    assert_eq!(failure.exit, None);
    assert_eq!(failure.head, Some(commit("h1")));
    assert_eq!(failure.at, Some(time(30)));
}

/// The pack's list with the merge failing to `fix`, at `h1` with a green check and a clean review.
fn merging() -> TicketFacts {
    let mut facts = with_pull();
    facts.phases[3].on_fail = Some(name("fix"));
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1")),
    );
    facts
}

#[test]
fn a_merge_whose_base_conflicts_runs_its_on_fail() {
    let mut facts = merging();
    facts
        .runs
        .push(run("merge", Some("h1"), 30).exits(1, 31).leaves(Some("h1")));
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.phase.name, name("merge"));
    assert!(matches!(current.status, PhaseStatus::Failed(_)));
    assert_eq!(current.next(), &name("fix"), "exit 1 is the conflict");
}

#[test]
fn a_failure_no_on_fail_answers_runs_the_phase_again() {
    // 2 a backend, 3 the base merged and pushed, 4 a refusal: no programmer can answer them.
    for code in [2, 3, 4, 101] {
        let mut facts = merging();
        facts.runs.push(
            run("merge", Some("h1"), 30)
                .exits(code, 31)
                .leaves(Some("h1")),
        );
        let current = current_phase(&facts).expect("a current phase");
        assert!(matches!(current.status, PhaseStatus::Failed(_)), "{code}");
        assert_eq!(current.next(), &name("merge"), "exit {code}");
    }
    // A red check is the on-fail's whatever the run's code.
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![failed(30)]);
    facts
        .runs
        .push(run("ci", Some("h1"), 31).exits(2, 32).leaves(Some("h1")));
    assert_eq!(current_phase(&facts).expect("current").next(), &name("fix"));
}

#[test]
fn a_ruled_question_runs_the_on_fail_once_then_the_phase_applies_again() {
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
    // The review raised #45 and exited 0; the question is ruled, so it neither waits nor is done.
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(0, 26)
            .leaves(Some("h1"))
            .asks(45),
    );
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.next(), &name("fix"), "the ruling goes to a model");

    // The fix found nothing to change: the head did not move, and the review applies again.
    facts
        .runs
        .push(run("fix", Some("h1"), 40).exits(0, 41).leaves(Some("h1")));
    let current = current_phase(&facts).expect("a current phase");
    assert_eq!(current.phase.name, name("review"));
    assert_eq!(current.status, PhaseStatus::Open);
    assert_eq!(current.next(), &name("review"));

    // A fix that failed, raised a question or started before the review ended answers nothing.
    for unanswered in [
        run("fix", Some("h1"), 40).exits(1, 41).leaves(Some("h1")),
        run("fix", Some("h1"), 40)
            .exits(0, 41)
            .leaves(Some("h1"))
            .asks(46),
        run("fix", Some("h1"), 20).exits(0, 21).leaves(Some("h1")),
        run("fix", Some("h1"), 40).lives(),
    ] {
        let mut facts = facts.clone();
        facts.runs.pop();
        facts.runs.push(unanswered.clone());
        let current = current_phase(&facts).expect("a current phase");
        assert!(
            matches!(current.status, PhaseStatus::Failed(_)),
            "{unanswered:?}"
        );
    }

    // A failure with no question is not answered so: the fix runs until the head moves.
    let mut facts = with_pull();
    edit_pull(&mut facts, |pull| pull.checks = vec![passed(24)]);
    facts.runs.push(
        run("review", Some("h1"), 25)
            .exits(1, 26)
            .leaves(Some("h1")),
    );
    facts
        .runs
        .push(run("fix", Some("h1"), 40).exits(0, 41).leaves(Some("h1")));
    assert_eq!(current_phase(&facts).expect("current").next(), &name("fix"));
}

#[test]
fn the_latest_counted_run_is_the_one_the_status_reads() {
    let mut facts = merging();
    let merge = facts.phases[3].clone();
    assert_eq!(latest_counted_run(&merge, &facts), None);
    facts
        .runs
        .push(run("merge", Some("h1"), 30).exits(1, 31).leaves(Some("h1")));
    facts.runs.push(
        run("merge", Some("h1"), 32)
            .exits(143, 33)
            .leaves(Some("h1")),
    );
    assert_eq!(
        latest_counted_run(&merge, &facts).map(|run| run.started_at),
        Some(time(30))
    );
}

/// The pack's ticket with its pull request at `h1` (opened at 20), no check there, and the
/// Engineer's ruling on `h1`.
fn ruled_through() -> TicketFacts {
    let mut facts = with_pull();
    facts.no_check_ruled = Some(commit("h1"));
    facts
}

/// A run of `action` that began and ended at `h1`, ending at `end`.
fn ran_at_h1(action: &str, end: u64) -> RunRecord {
    run(action, Some("h1"), end - 1)
        .exits(0, end)
        .leaves(Some("h1"))
}

#[test]
fn ruled_no_check_head_ends_ci() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    let mut facts = with_pull();
    assert!(!end_fact_holds(&ci, &facts), "unruled, no check is open");
    assert_eq!(facts.awaited_checks(), Some(&commit("h1")));

    facts.no_check_ruled = Some(commit("h1"));
    assert!(end_fact_holds(&ci, &facts));
    assert_eq!(facts.awaited_checks(), None);
    let current = current_phase(&facts).expect("current");
    assert_eq!(current.phase.name, name("review"));
    assert_eq!(current.status, PhaseStatus::Open, "its next phase is ready");
}

#[test]
fn ruled_no_check_time_is_the_last_run_before_ci() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    let mut facts = ruled_through();
    facts.runs.push(ran_at_h1("implement", 25));
    facts.runs.push(ran_at_h1("implement", 30));
    // A run at another head, one of an unlisted action and one of a later phase do not count.
    facts.runs.push(
        run("implement", Some("h0"), 39)
            .exits(0, 40)
            .leaves(Some("h0")),
    );
    facts.runs.push(ran_at_h1("fix", 50));
    facts.runs.push(ran_at_h1("review", 60));
    assert_eq!(ended_at(&ci, &facts, None), Some(time(30)));
}

#[test]
fn ruled_no_check_time_falls_back_to_the_opening() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    let mut facts = ruled_through();
    assert_eq!(ended_at(&ci, &facts, None), Some(time(20)));
    // A run before ci that never ended gives no time.
    facts.runs.push(run("implement", Some("h1"), 24));
    assert_eq!(ended_at(&ci, &facts, None), Some(time(20)));
}

#[test]
fn ruled_no_check_time_does_not_move_after_review() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    let mut facts = ruled_through();
    facts.runs.push(ran_at_h1("implement", 25));
    assert_eq!(ended_at(&ci, &facts, None), Some(time(25)));
    // The phase after ci reads it as the time the phase before it ended.
    assert_eq!(
        current_phase(&facts).map(|current| current.previous),
        Some(Some(time(25)))
    );
    facts.runs.push(ran_at_h1("review", 90));
    assert_eq!(ended_at(&ci, &facts, None), Some(time(25)));
}

#[test]
fn ruling_on_an_old_head_does_not_pass_a_new_one() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    let mut facts = ruled_through();
    edit_pull(&mut facts, |pull| pull.head = Some(commit("h2")));
    assert!(!end_fact_holds(&ci, &facts));
    assert_eq!(facts.awaited_checks(), Some(&commit("h2")));
    assert_eq!(
        current_phase(&facts).map(|current| current.phase.name.clone()),
        Some(name("ci"))
    );
}

#[test]
fn ruling_does_not_pass_a_pending_check() {
    let ci = phase("ci", &[PhaseEnd::CiDone], Some("fix"));
    for checks in [
        vec![pending()],
        vec![passed(30), pending()],
        vec![failed(30)],
    ] {
        let mut facts = ruled_through();
        edit_pull(&mut facts, |pull| pull.checks = checks.clone());
        assert!(!end_fact_holds(&ci, &facts));
        assert!(!facts.ruled_no_check());
    }
    // A pending check keeps the wait on the head.
    let mut facts = ruled_through();
    edit_pull(&mut facts, |pull| pull.checks = vec![pending()]);
    assert_eq!(facts.awaited_checks(), Some(&commit("h1")));
}
