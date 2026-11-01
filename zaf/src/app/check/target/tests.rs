use super::*;

#[test]
fn a_number_is_a_task() {
    assert_eq!("26".parse(), Ok(Target::Task(IssueNumber::from(26))));
    assert_eq!("#26".parse(), Ok(Target::Task(IssueNumber::from(26))));
}

#[test]
fn a_pr_prefix_is_a_pull_request_with_an_optional_task() {
    let bare = Target::Pr {
        pr: PrNumber::from(12),
        task: None,
    };
    assert_eq!("pr:12".parse(), Ok(bare));
    let with_task = Target::Pr {
        pr: PrNumber::from(12),
        task: Some(IssueNumber::from(26)),
    };
    assert_eq!("pr:#12:#26".parse(), Ok(with_task));
    assert_eq!(with_task.slug(), "pr-12");
    assert_eq!(Target::Task(IssueNumber::from(26)).slug(), "task-26");
}

#[test]
fn anything_else_is_refused() {
    for text in ["", "pr:", "pr:x", "pr:1:y", "milestone:3", "abc"] {
        assert!(text.parse::<Target>().is_err(), "{text}");
    }
}
