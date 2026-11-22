use super::super::tests::*;
use super::*;

#[test]
fn changes_a_file_follows_changed_files() {
    let mut pull = pull("h1", 20);
    pull.changed_files = ChangedFiles::default();
    assert!(!pull.changes_a_file());
    pull.changed_files = ChangedFiles::of(&["src/lib.rs"]);
    assert!(pull.changes_a_file());
    pull.changed_files = ChangedFiles::of(&["zaf/x.rs", "docs/x.rs"]);
    assert!(pull.changes_a_file());
    pull.changed_files = ChangedFiles::default();
    assert!(!pull.changes_a_file());
}

#[test]
fn no_check_at_all_has_not_ended() {
    let mut pull = pull("h1", 20);
    assert!(!pull.checks_ended());
    assert_eq!(pull.checks_passed_at(), None);
    pull.checks = vec![passed(30), pending()];
    assert!(!pull.checks_ended());
    pull.checks = vec![passed(30), failed(35)];
    assert!(pull.checks_ended());
}

#[test]
fn checks_passed_at_is_the_last_green_finish_and_none_with_a_red_one() {
    let mut pull = pull("h1", 20);
    pull.checks = vec![passed(30), passed(40)];
    assert_eq!(pull.checks_passed_at(), Some(time(40)));
    pull.checks.push(failed(35));
    assert_eq!(pull.checks_passed_at(), None);
}

#[test]
fn the_failed_checks_are_named_in_the_forges_order_with_their_first_failure() {
    let mut pull = pull("h1", 20);
    pull.checks = vec![
        check("lint", CheckResult::Failed(time(50))),
        passed(30),
        check("test", CheckResult::Failed(time(40))),
        pending(),
    ];
    assert_eq!(
        pull.failed_checks(),
        [CheckName::from("lint"), CheckName::from("test")]
    );
    assert_eq!(pull.first_failure_at(), Some(time(40)));
    assert_eq!(pull.last_check_at(), Some(time(50)));
}

#[test]
fn a_recorded_exit_ends_a_run_whatever_the_process_table_says() {
    let running = run("implement", None, 5).lives();
    assert!(running.live());
    assert!(!running.exits(0, 9).live());
    assert!(!run("implement", None, 5).live());
}

#[test]
fn the_head_a_run_left_falls_back_to_its_start_head_for_an_old_folder() {
    let old = run("review", Some("h1"), 5);
    assert_eq!(old.left(), Some(&commit("h1")));
    let moved = run("fix", Some("h1"), 5).leaves(Some("h2"));
    assert_eq!(moved.left(), Some(&commit("h2")));
    assert_eq!(
        moved.report(),
        Report {
            exit: None,
            questions: BTreeSet::new(),
            head: Some(commit("h2")),
        }
    );
}

#[test]
fn a_run_stayed_at_a_head_only_by_its_record() {
    let stayed = run("fix", Some("h1"), 5).leaves(Some("h1"));
    assert!(stayed.stayed_at(Some(&commit("h1"))));
    assert!(!stayed.stayed_at(Some(&commit("h2"))));
    // An end head that is unknown never stayed anywhere, even at its start head.
    assert!(!run("fix", Some("h1"), 5).stayed_at(Some(&commit("h1"))));
    // With no pull request, a run that began and ended at none stayed at none.
    assert!(run("implement", None, 5).leaves(None).stayed_at(None));
}

fn trees(read: &[(&str, &str)]) -> Trees {
    let mut trees = Trees::default();
    for (commit_id, tree) in read {
        trees.insert(commit(commit_id), TreeId::from(*tree));
    }
    trees
}

#[test]
fn stayed_at_counts_an_empty_commit_as_the_same_tree() {
    // h2 and h3 are empty commits on h1: one tree.
    let trees = trees(&[("h1", "t1"), ("h2", "t1"), ("h3", "t1")]);
    let run = run("fix", Some("h2"), 5).leaves(Some("h3"));
    assert!(run.stayed_at_tree(Some(&commit("h1")), &trees));
    // By commit id alone it did move.
    assert!(!run.stayed_at(Some(&commit("h1"))));
    // The same commit is the same tree, read or not.
    let same = self::run("fix", Some("h9"), 5).leaves(Some("h9"));
    assert!(same.stayed_at_tree(Some(&commit("h9")), &Trees::default()));
    // An unknown end is never at a head, and no head is only no head.
    assert!(!self::run("fix", Some("h1"), 5).stayed_at_tree(Some(&commit("h1")), &trees));
    assert!(
        self::run("implement", None, 5)
            .leaves(None)
            .stayed_at_tree(None, &trees)
    );
    assert!(
        !self::run("implement", None, 5)
            .leaves(None)
            .stayed_at_tree(Some(&commit("h1")), &trees)
    );
}

#[test]
fn stayed_at_false_for_a_changed_tree() {
    let trees = trees(&[("h1", "t1"), ("h2", "t2"), ("h3", "t2")]);
    // Began at the head's tree and left another: it moved.
    let moved = run("fix", Some("h1"), 5).leaves(Some("h2"));
    assert!(!moved.stayed_at_tree(Some(&commit("h1")), &trees));
    // Began elsewhere and ended at the head's tree: it did not stay.
    assert!(!moved.stayed_at_tree(Some(&commit("h2")), &trees));
    // Both ends at another tree than the head's.
    let before = run("fix", Some("h2"), 5).leaves(Some("h3"));
    assert!(!before.stayed_at_tree(Some(&commit("h1")), &trees));
}

#[test]
fn an_unreadable_tree_counts_by_its_commit_id() {
    // h2's tree was not read: it is at h1 only when it is h1.
    let trees = trees(&[("h1", "t1")]);
    let run = run("fix", Some("h1"), 5).leaves(Some("h2"));
    assert!(!run.stayed_at_tree(Some(&commit("h1")), &trees));
    assert!(!run.stayed_at_tree(Some(&commit("h1")), &Trees::default()));
    let same = self::run("fix", Some("h2"), 5).leaves(Some("h2"));
    assert!(same.stayed_at_tree(Some(&commit("h2")), &trees));
}

#[test]
fn checks_are_awaited_only_by_a_list_with_ci_and_a_head() {
    let mut facts = facts(pack());
    assert_eq!(facts.awaited_checks(), None);
    facts.pull = Some(pull("h1", 20));
    assert_eq!(facts.awaited_checks(), Some(&commit("h1")));
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![passed(30)];
    }
    assert_eq!(facts.awaited_checks(), None);
    let mut no_ci = facts.clone();
    no_ci.phases.remove(1);
    if let Some(pull) = no_ci.pull.as_mut() {
        pull.checks.clear();
    }
    assert_eq!(no_ci.awaited_checks(), None);
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks.clear();
        pull.head = None;
    }
    assert_eq!(facts.awaited_checks(), None);
}

#[test]
fn the_latest_ruled_marker_of_the_action_counts() {
    let mut facts = facts(pack());
    facts.ruled = vec![
        RuledMarker {
            action: name("fix"),
            last_run: time(30),
        },
        RuledMarker {
            action: name("fix"),
            last_run: time(50),
        },
        RuledMarker {
            action: name("implement"),
            last_run: time(70),
        },
    ];
    assert_eq!(facts.ruled_through(&name("fix")), Some(time(50)));
    assert_eq!(facts.ruled_through(&name("merge")), None);
}

#[test]
fn the_earliest_live_run_is_the_live_one_and_merged_needs_a_merge_time() {
    let mut facts = facts(pack());
    assert!(!facts.merged());
    facts.runs = vec![
        run("review", Some("h1"), 9).lives(),
        run("implement", None, 5).lives(),
    ];
    assert_eq!(facts.live_run().map(|run| run.started_at), Some(time(5)));
    facts.pull = Some(PullFacts {
        merged_at: Some(time(60)),
        ..pull("h1", 20)
    });
    assert!(facts.merged());
    assert_eq!(facts.head(), Some(&commit("h1")));
}

#[test]
fn not_run_is_125() {
    assert_eq!(ExitCode::NOT_RUN.get(), 125);
    assert!(ExitCode::from(0).is_zero());
}
