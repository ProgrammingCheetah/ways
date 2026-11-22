use super::*;
use crate::domain::checks_expected;

fn read(text: &str) -> PullRequestFilter {
    read_workflow(".forgejo/workflows/zaf.yml", text).filter
}

fn paths(patterns: &[&str]) -> PullRequestFilter {
    PullRequestFilter::Paths(patterns.iter().map(|p| (*p).to_owned()).collect())
}

fn branches(patterns: &[&str]) -> PullRequestFilter {
    PullRequestFilter::Branches(patterns.iter().map(|p| (*p).to_owned()).collect())
}

/// Whether a check is expected for a pull request from `branch` that changes a doc, the
/// workflow being `text`.
fn expected(text: &str, branch: &str) -> bool {
    let flow = read_workflow(".forgejo/workflows/zaf.yml", text);
    checks_expected(&[flow], &[RepoPath::from("docs/a.md")], false, Some(branch))
}

#[test]
fn reads_the_paths_of_a_pull_request_trigger() {
    let text = "name: gate\non:\n  pull_request:\n    paths:\n      - 'zaf/**'\n      - .zaf.toml\njobs: {}\n";
    assert_eq!(read(text), paths(&["zaf/**", ".zaf.toml"]));
}

#[test]
fn keeps_the_path_it_was_given() {
    let flow = read_workflow(".github/workflows/a.yaml", "on: push\n");
    assert_eq!(flow.path, RepoPath::from(".github/workflows/a.yaml"));
}

#[test]
fn a_pull_request_with_no_paths_filter_runs_on_any_change() {
    assert_eq!(read("on:\n  pull_request:\n"), PullRequestFilter::AnyChange);
    assert_eq!(
        read("on:\n  pull_request:\n    types: [opened]\n"),
        PullRequestFilter::AnyChange
    );
    assert_eq!(read("on: pull_request\n"), PullRequestFilter::AnyChange);
    assert_eq!(
        read("on: [push, pull_request]\n"),
        PullRequestFilter::AnyChange
    );
}

#[test]
fn no_trigger_that_reaches_a_pull_request_runs_on_none() {
    assert_eq!(read("on: schedule\n"), PullRequestFilter::NoTrigger);
    assert_eq!(
        read("on: [workflow_dispatch]\n"),
        PullRequestFilter::NoTrigger
    );
    assert_eq!(
        read("on:\n  workflow_dispatch:\n  schedule:\n    - cron: '0 0 * * *'\n"),
        PullRequestFilter::NoTrigger
    );
    assert_eq!(
        read("on:\n  push:\n    tags: ['v*']\n"),
        PullRequestFilter::NoTrigger
    );
}

#[test]
fn ci_filter_push_no_branches_expected() {
    assert_eq!(read("on: push\n"), PullRequestFilter::AnyChange);
    assert_eq!(read("on: [push]\n"), PullRequestFilter::AnyChange);
    assert_eq!(read("on:\n  push:\n"), PullRequestFilter::AnyChange);
    assert!(expected("on: push\n", "feature/0.5.3-963-973"));
    assert!(expected("on:\n  push:\n", "feature/0.5.3-963-973"));
}

#[test]
fn ci_filter_push_matching_branch_expected() {
    let text = "on:\n  push:\n    branches: [main, 'feature/**']\n";
    assert_eq!(read(text), branches(&["main", "feature/**"]));
    assert!(expected(text, "feature/0.5.3-963-973"));
    assert!(expected(text, "main"));
    // A pattern the reader is not sure of expects a check.
    assert!(expected("on:\n  push:\n    branches: ['!main']\n", "other"));
}

#[test]
fn ci_filter_push_other_branch_not_expected() {
    let text = "on:\n  push:\n    branches: [main, 'release/*']\n";
    assert!(!expected(text, "feature/0.5.3-963-973"));
    // `*` stops at a slash.
    assert!(!expected(text, "release/1/2"));
    // Tags beside branches do not matter to a branch push.
    let tagged = "on:\n  push:\n    branches: [main]\n    tags: ['v*']\n";
    assert!(!expected(tagged, "feature/x"));
    assert!(!expected("on:\n  push:\n    tags: ['v*']\n", "feature/x"));
}

#[test]
fn ci_filter_pull_request_target_expected() {
    assert_eq!(
        read("on: pull_request_target\n"),
        PullRequestFilter::AnyChange
    );
    assert_eq!(
        read("on: [pull_request_target]\n"),
        PullRequestFilter::AnyChange
    );
    assert_eq!(
        read("on:\n  pull_request_target:\n    types: [opened]\n"),
        PullRequestFilter::AnyChange
    );
    assert!(expected("on:\n  pull_request_target:\n", "feature/x"));
    assert_eq!(
        read("on:\n  pull_request_target:\n    paths: ['zaf/**']\n"),
        paths(&["zaf/**"])
    );
}

#[test]
fn ci_filter_unreadable_unsure() {
    for text in [
        "on:\n  push:\n    branches: []\n",
        "on:\n  push:\n    branches: main\n",
        "on:\n  push:\n    branches: [1]\n",
        "on:\n  push:\n    paths: ['a/**']\n",
        "on:\n  push:\n    branches-ignore: [wip]\n",
        "on:\n  push:\n    something: new\n",
        "on:\n  push: yes\n",
        "on:\n  pull_request_target:\n    branches: [main]\n",
        "on: [push, 1]\n",
        "on: [unclosed",
    ] {
        assert_eq!(read(text), PullRequestFilter::Unsure, "{text}");
        assert!(expected(text, "feature/x"), "{text}");
    }
}

#[test]
fn narrowing_triggers_stand_side_by_side() {
    let text = "on:\n  push:\n    branches: [main]\n  pull_request:\n    paths: ['zaf/**']\n";
    assert_eq!(
        read(text),
        PullRequestFilter::Either(vec![branches(&["main"]), paths(&["zaf/**"])])
    );
    // A doc change from another branch runs neither; from main, the push runs.
    assert!(!expected(text, "feature/x"));
    assert!(expected(text, "main"));
    // A trigger that lets every change through settles it.
    assert_eq!(
        read("on:\n  push:\n  pull_request:\n    paths: ['a']\n"),
        PullRequestFilter::AnyChange
    );
}

#[test]
fn a_filter_not_sure_to_read_right_is_unsure() {
    for text in [
        "on:\n  pull_request:\n    paths-ignore: ['docs/**']\n",
        "on:\n  pull_request:\n    paths: ['a/**']\n    paths-ignore: ['b/**']\n",
        "on:\n  pull_request:\n    branches: [main]\n",
        "on:\n  pull_request:\n    branches-ignore: [wip]\n",
        "on:\n  pull_request:\n    paths: []\n",
        "on:\n  pull_request:\n    paths: [1, 2]\n",
        "on:\n  pull_request:\n    paths: a/**\n",
        "on:\n  pull_request:\n    something: new\n",
        "on:\n  pull_request: yes\n",
    ] {
        assert_eq!(read(text), PullRequestFilter::Unsure, "{text}");
    }
}

#[test]
fn a_negated_pattern_is_handed_on_for_the_domain_to_doubt() {
    assert_eq!(
        read("on:\n  pull_request:\n    paths: ['a/**', '!a/b/**']\n"),
        paths(&["a/**", "!a/b/**"])
    );
}

#[test]
fn an_unreadable_workflow_is_unsure() {
    for text in [
        "",
        "on: [unclosed",
        "\t: not yaml: [",
        "- just\n- a list\n",
        "name: no triggers\njobs: {}\n",
        "on: 7\n",
        "on: [1, 2]\n",
    ] {
        assert_eq!(read(text), PullRequestFilter::Unsure, "{text:?}");
    }
}

#[test]
fn a_yaml_alias_bomb_is_read_without_harm() {
    let text = "a: &a [x, x, x, x, x, x, x, x, x]\nb: &b [*a, *a, *a, *a, *a, *a, *a, *a, *a]\nc: &c [*b, *b, *b, *b, *b, *b, *b, *b, *b]\nd: &d [*c, *c, *c, *c, *c, *c, *c, *c, *c]\non: push\n";
    // Either read is safe; it must return, not hang or crash.
    let _ = read(text);
}

#[test]
fn push_with_branches_and_paths_narrows_to_branches() {
    for text in [
        "on:\n  push:\n    branches: [main]\n    paths: ['zaf/**']\n",
        "on:\n  push:\n    paths: ['zaf/**']\n    branches: [main]\n",
        "on:\n  push:\n    branches: [main]\n    paths-ignore: ['docs/**']\n",
        "on:\n  push:\n    branches: [main]\n    tags: ['v*']\n    paths: ['zaf/**']\n",
    ] {
        assert_eq!(read(text), branches(&["main"]), "{text}");
    }
}

#[test]
fn push_with_paths_alone_is_unsure() {
    for text in [
        "on:\n  push:\n    paths: ['a/**']\n",
        "on:\n  push:\n    paths-ignore: ['docs/**']\n",
        "on:\n  push:\n    tags: ['v*']\n    paths: ['a/**']\n",
        "on:\n  push:\n    branches-ignore: [wip]\n    paths: ['a/**']\n",
        "on:\n  push:\n    branches: [main]\n    something: new\n    paths: ['a/**']\n",
        "on:\n  push:\n    branches: [main]\n    branches-ignore: [wip]\n    paths: ['a/**']\n",
    ] {
        assert_eq!(read(text), PullRequestFilter::Unsure, "{text}");
    }
}

#[test]
fn ways_shaped_workflow_expects_no_check_off_main_outside_paths() {
    let text = "on:\n  pull_request:\n    paths: ['zaf/**', '.zaf.toml']\n  push:\n    branches: [main]\n    paths: ['zaf/**', '.zaf.toml']\n";
    // A doc change from a slice branch: the pull request's paths and the push's branch both miss.
    assert!(!expected(text, "feature/0.5.3-993-994"));
    // From main the push still runs for a pull request, as before.
    assert!(expected(text, "main"));
}
