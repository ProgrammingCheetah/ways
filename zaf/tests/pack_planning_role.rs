//! The planning agent's role text has the planner write its epic's whole plan, hand it to the
//! `plan` tool, `check` until zaf no longer refuses it, then `file` (design D1, D2, D28, D29).
//! Only the text after the interface page is read: the page's own `board file` row is not the
//! role's. Outside the ways repository, where the pack is not beside the crate, nothing runs.

use std::path::Path;

/// The planning agent's role text: its file after the frontmatter and the interface page.
fn role_text() -> Option<String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("../plugins/teams/agents/planning.md");
    if !path.is_file() {
        return None;
    }
    let interface = std::fs::read_to_string(crate_dir.join("assets/interface.md"))
        .expect("the interface page reads");
    let brief = std::fs::read_to_string(&path).expect("the planning brief reads");
    let body = brief
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
    let role = body
        .strip_prefix(&interface)
        .expect("the planning brief opens with the interface page");
    Some(role.to_string())
}

#[test]
fn planning_role_plans_through_the_plan_tool() {
    let Some(role) = role_text() else { return };
    for needle in [
        "`plan` tool",
        "`check`",
        "`file`",
        "`Task:",
        "`Node`",
        "`Writes`",
        "`Provides`",
        "`Needs`",
        "`New names:`",
        "`Drop:`",
    ] {
        assert!(
            role.contains(needle),
            "the planning role text lacks {needle}"
        );
    }
}

#[test]
fn planning_role_names_no_blocked_by() {
    let Some(role) = role_text() else { return };
    for needle in ["Blocked by", "--blocked", "board file", "gh issue close"] {
        assert!(
            !role.contains(needle),
            "the planning role text still names {needle}"
        );
    }
}
