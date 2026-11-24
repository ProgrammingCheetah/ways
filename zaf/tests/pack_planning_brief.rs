//! The planning instance's standing brief has the planner hand its whole plan to the `plan`
//! tool: `check` until zaf no longer refuses it, then `file` (design D1, D2, D17, D28, D29).

use zaf::app::spawn::PLANNING_BRIEF;

#[test]
fn planning_brief_plans_through_the_plan_tool() {
    for needle in [
        r#"`["check", "<epic>"]`"#,
        r#"`["file", "<epic>"]`"#,
        "Task:",
        "Node:",
        "Writes",
        "Provides",
        "Needs",
        "New names:",
        "Drop:",
        "vocabulary",
    ] {
        assert!(
            PLANNING_BRIEF.contains(needle),
            "the planning brief lacks {needle}"
        );
    }
}

#[test]
fn planning_brief_names_no_blocked_by() {
    for needle in ["Blocked by", "--blocked", "board file", "gh issue close"] {
        assert!(
            !PLANNING_BRIEF.contains(needle),
            "the planning brief still holds {needle}"
        );
    }
}
