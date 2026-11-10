//! The teams pack's six role briefs open with the exact bytes of zaf's interface page, right
//! after their frontmatter, so every instance's prompt shares one cached prefix
//! (`plugins/teams/README.md`). A change to the page that is not pasted into all six fails
//! here. Outside the ways repository, where the pack is not beside the crate, nothing runs.

use std::path::Path;

const ROLES: [&str; 6] = [
    "team",
    "programmer",
    "keeper",
    "planning",
    "challenger",
    "release",
];

#[test]
fn every_role_brief_opens_with_the_interface_page() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = crate_dir.join("../plugins/teams/agents");
    if !agents.is_dir() {
        return;
    }
    let interface = std::fs::read_to_string(crate_dir.join("assets/interface.md"))
        .expect("the interface page reads");
    for role in ROLES {
        let path = agents.join(format!("{role}.md"));
        let brief = std::fs::read_to_string(&path).expect("the brief reads");
        let body = brief
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .map(|(_, body)| body)
            .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
        assert!(
            body.starts_with(&interface),
            "{} does not open with zaf/assets/interface.md; paste the page in again",
            path.display()
        );
    }
}
