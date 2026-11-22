use super::*;

/// [`super::checks_expected`] for a pull request whose branch is not known.
fn checks_expected(workflows: &[WorkflowFilter], changed: &[RepoPath], cut_short: bool) -> bool {
    super::checks_expected(workflows, changed, cut_short, None)
}

/// [`super::checks_expected`] for a pull request from `branch` that changes a doc.
fn expected_from(filter: PullRequestFilter, branch: &str) -> bool {
    super::checks_expected(
        &[workflow(filter)],
        &changed(&["docs/a.md"]),
        false,
        Some(branch),
    )
}

fn branches(patterns: &[&str]) -> PullRequestFilter {
    PullRequestFilter::Branches(patterns.iter().map(|p| (*p).to_owned()).collect())
}

#[test]
fn a_push_filter_expects_a_check_for_a_matching_branch_only() {
    let filter = branches(&["main", "feature/**", "release/*"]);
    assert!(expected_from(filter.clone(), "main"));
    assert!(expected_from(filter.clone(), "feature/0.5.3-963-973"));
    assert!(!expected_from(filter.clone(), "release/1/2"));
    assert!(!expected_from(filter, "wip"));
}

#[test]
fn a_push_filter_with_no_known_branch_or_an_unsure_pattern_expects_a_check() {
    let flows = [workflow(branches(&["main"]))];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
    assert!(expected_from(branches(&["!main"]), "other"));
    assert!(expected_from(branches(&["{a,b}"]), "other"));
}

#[test]
fn either_expects_a_check_when_any_of_its_filters_lets_the_change_through() {
    let either = PullRequestFilter::Either(vec![
        branches(&["main"]),
        PullRequestFilter::Paths(vec!["zaf/**".to_owned()]),
    ]);
    assert!(expected_from(either.clone(), "main"));
    assert!(!expected_from(either, "wip"));
}

fn workflow(filter: PullRequestFilter) -> WorkflowFilter {
    WorkflowFilter {
        path: RepoPath::from(".forgejo/workflows/zaf.yml"),
        filter,
    }
}

fn paths(patterns: &[&str]) -> WorkflowFilter {
    workflow(PullRequestFilter::Paths(
        patterns.iter().map(|p| (*p).to_owned()).collect(),
    ))
}

fn changed(files: &[&str]) -> Vec<RepoPath> {
    files.iter().map(|f| RepoPath::from(*f)).collect()
}

#[test]
fn expects_a_check_when_a_path_matches() {
    let flows = [paths(&["zaf/**", ".zaf.toml"])];
    assert!(checks_expected(
        &flows,
        &changed(&["zaf/src/lib.rs"]),
        false
    ));
    assert!(checks_expected(&flows, &changed(&[".zaf.toml"]), false));
}

#[test]
fn expects_none_when_no_path_matches() {
    let flows = [paths(&["zaf/**", ".zaf.toml"])];
    assert!(!checks_expected(
        &flows,
        &changed(&["docs/index.md", "README.md"]),
        false
    ));
    // `*` stops at a slash.
    assert!(!checks_expected(
        &[paths(&["zaf/*.rs"])],
        &changed(&["zaf/src/lib.rs"]),
        false
    ));
    assert!(!checks_expected(&[], &changed(&["a.md"]), false));
}

#[test]
fn expects_a_check_when_a_workflow_file_changed() {
    let flows = [paths(&["zaf/**"])];
    assert!(checks_expected(
        &flows,
        &changed(&[".forgejo/workflows/zaf.yml"]),
        false
    ));
    assert!(checks_expected(
        &flows,
        &changed(&[".github/workflows/release.yml"]),
        false
    ));
}

#[test]
fn expects_a_check_when_a_yaml_workflow_changed() {
    let flows = [paths(&["zaf/**"])];
    assert!(checks_expected(
        &flows,
        &changed(&[".forgejo/workflows/new.yaml"]),
        false
    ));
    // Not a workflow: another extension, or another folder.
    assert!(!checks_expected(
        &flows,
        &changed(&[".forgejo/workflows/README.md", "docs/.github/workflows.yml"]),
        false
    ));
}

#[test]
fn expects_a_check_when_the_list_is_cut_short() {
    let flows = [paths(&["zaf/**"])];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), true));
    assert!(checks_expected(&[], &[], true));
}

#[test]
fn expects_a_check_when_a_workflow_is_unreadable() {
    let flows = [workflow(PullRequestFilter::Unsure)];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
}

#[test]
fn expects_a_check_for_paths_ignore() {
    // The adapter reads `paths-ignore` as unsure, beside a workflow that would not run.
    let flows = [workflow(PullRequestFilter::Unsure), paths(&["zaf/**"])];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
}

#[test]
fn expects_a_check_for_a_negated_pattern() {
    let flows = [paths(&["zaf/**", "!zaf/docs/**"])];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
}

#[test]
fn expects_a_check_for_a_branch_filter() {
    // The adapter reads a branch filter as unsure.
    let flows = [workflow(PullRequestFilter::Unsure)];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
}

#[test]
fn expects_a_check_with_no_paths_filter() {
    let flows = [workflow(PullRequestFilter::AnyChange)];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
}

#[test]
fn expects_none_with_no_pull_request_trigger() {
    let flows = [workflow(PullRequestFilter::NoTrigger)];
    assert!(!checks_expected(
        &flows,
        &changed(&["zaf/src/lib.rs"]),
        false
    ));
}

#[test]
fn expects_a_check_for_a_rename_from_a_matched_path() {
    // `zaf/notes.md` moved into `docs/`: the forge lists both names.
    let flows = [paths(&["zaf/**"])];
    assert!(checks_expected(
        &flows,
        &changed(&["docs/notes.md", "zaf/notes.md"]),
        false
    ));
}

#[test]
fn a_pattern_that_does_not_compile_or_uses_braces_expects_a_check() {
    let flows = [paths(&["zaf/[**"])];
    assert!(checks_expected(&flows, &changed(&["docs/a.md"]), false));
    let flows = [paths(&["{zaf,docs}/**"])];
    assert!(checks_expected(&flows, &changed(&["README.md"]), false));
}
