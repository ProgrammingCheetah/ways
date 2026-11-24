//! The release agent's role text files fix tasks through `plan file` under an epic that has a
//! plan, since `board file` is refused there (design D17, D28, D29). Only the text after the
//! interface page is read: the page's own `board file` row is not the role's. Outside the ways
//! repository, where the pack is not beside the crate, nothing runs.

use std::path::Path;

#[test]
fn release_role_files_fixes_through_plan_file() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("../plugins/teams/agents/release.md");
    if !path.is_file() {
        return;
    }
    let interface = std::fs::read_to_string(crate_dir.join("assets/interface.md"))
        .expect("the interface page reads");
    let brief = std::fs::read_to_string(&path).expect("the release brief reads");
    let body = brief
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
    let role = body
        .strip_prefix(&interface)
        .expect("the release brief opens with the interface page");
    for needle in [
        "`plan file`",
        "`plan check`",
        "`Task:",
        "`Node`",
        "`Writes`",
        "`Provides`",
        "`Needs`",
        "`board file`",
    ] {
        assert!(
            role.contains(needle),
            "the release role text lacks {needle}"
        );
    }
}
