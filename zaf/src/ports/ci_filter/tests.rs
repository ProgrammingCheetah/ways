use super::*;
use crate::domain::{PullRequestFilter, RepoPath};
use crate::fakes::FakeCiFilter;

#[test]
fn the_fake_hands_back_only_the_branch_asked_for() {
    let filter = FakeCiFilter::default();
    filter.set_workflow(
        "main",
        ".forgejo/workflows/a.yml",
        PullRequestFilter::AnyChange,
    );
    filter.set_workflow(
        "feature/x",
        ".forgejo/workflows/b.yml",
        PullRequestFilter::NoTrigger,
    );
    assert_eq!(
        filter.workflows(&BranchName::from("main")),
        Ok(vec![WorkflowFilter {
            path: RepoPath::from(".forgejo/workflows/a.yml"),
            filter: PullRequestFilter::AnyChange,
        }])
    );
}

#[test]
fn the_fake_has_no_workflows_for_a_branch_it_has_none_for() {
    let filter = FakeCiFilter::default();
    assert_eq!(filter.workflows(&BranchName::from("main")), Ok(Vec::new()));
}

#[test]
fn the_fake_can_fail_the_list() {
    let filter = FakeCiFilter::default();
    filter.fail("the forge is down");
    assert!(matches!(
        filter.workflows(&BranchName::from("main")),
        Err(PortError::Backend(_))
    ));
}
