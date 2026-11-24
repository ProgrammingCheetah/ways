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

const MCP_RULE: &str = "zaf's verbs are MCP tools. Wherever a page says `zaf <verb> <words>`, call the tool `<verb>` (`mcp__zaf__<verb>`) with `args` set to those words. Never type `zaf` in Bash; it is denied.";

#[test]
fn interface_page_opens_with_the_mcp_rule() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let interface = std::fs::read_to_string(crate_dir.join("assets/interface.md"))
        .expect("the interface page reads");
    assert!(
        interface.starts_with(MCP_RULE),
        "the interface page must open with D15's rule, word for word"
    );
    let rest = &interface[MCP_RULE.len()..];
    for line in rest.lines() {
        let line = line.trim_start().trim_start_matches(['|', '-', ' ']);
        let line = line.trim_start_matches('`');
        assert!(
            !line.starts_with("zaf "),
            "a code span or line begins with `zaf <verb>`: {line}"
        );
    }
    for needle in ["exit <n>", "`64`", "`125`", "`2`", "`1`", "`0`"] {
        assert!(interface.contains(needle), "the page lacks {needle}");
    }
}

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

/// Every role zaf runs tells its model to declare its phases (`plan`) and report each change
/// (`at`) through the `progress` tool: the five role files and the two standing briefs. A role
/// that works an epic names no task, so the ticket comes from `ZAF_INSTANCE` (D17).
#[test]
fn every_role_zaf_runs_names_progress() {
    const PLAN: &str = "`[\"plan\", \"";
    const AT: &str = "`[\"at\", \"";
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = crate_dir.join("../plugins/teams/agents");
    if !agents.is_dir() {
        return;
    }
    let roles = [
        "programmer",
        "planning",
        "challenger",
        "release",
        "designer",
    ]
    .map(|role| agents.join(format!("{role}.md")));
    let briefs = ["planning-brief", "challenger-brief"]
        .map(|brief| crate_dir.join(format!("assets/spawn/{brief}.md")));
    for path in roles.iter().chain(&briefs) {
        let text = std::fs::read_to_string(path).expect("the file reads");
        let name = path.display();
        assert!(
            text.contains("`progress`"),
            "{name} does not name the `progress` tool"
        );
        assert!(
            text.contains(PLAN) && text.contains(AT),
            "{name} lacks a `plan` call and an `at` call of the `progress` tool"
        );
    }
    for path in &briefs {
        let text = std::fs::read_to_string(path).expect("the brief reads");
        for line in text
            .lines()
            .filter(|line| line.contains(PLAN) || line.contains(AT))
        {
            assert!(
                !line.contains("<task>"),
                "{} names a task in a `progress` call: {line}",
                path.display()
            );
        }
    }
}

/// A planner and a challenger settle a question of ownership, placement or an interface's shape
/// by the design page on how code is divided, and raise it only in the three cases the page lists
/// (D28). The page is named as every project has it: the pack's standard page by the id `setup`
/// installs it under, or the project's own page on it from the design index; never by a path or
/// an id only zaf's own repository holds.
const OWN_REPOSITORY_ONLY: [&str; 2] = ["docs/solid-how-zafs-code-is-divided.md", "`solid`"];
const SOLID_CASES: [&str; 3] = [
    "one-way door",
    "two principles disagree",
    "no principle decides",
];

fn assert_names_the_solid_page(path: &Path) {
    let text = std::fs::read_to_string(path).expect("the file reads");
    let name = path.display();
    let standard = standard_page_id();
    assert!(
        text.contains(&format!("`{standard}`")),
        "{name} does not name the standard design page `{standard}`"
    );
    assert!(
        text.contains("the project's own page") && text.contains("design index"),
        "{name} does not allow for the project's own page in the design index"
    );
    for only_here in OWN_REPOSITORY_ONLY {
        assert!(
            !text.contains(only_here),
            "{name} names {only_here}, which only zaf's own repository holds"
        );
    }
    for case in SOLID_CASES {
        assert!(text.contains(case), "{name} lacks the case: {case}");
    }
}

/// The id of the pack's standard page on how code is divided, read from its file name where the
/// pack sits beside the crate (`setup` installs a page under the id its file name starts with).
fn standard_page_id() -> String {
    let pages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/design");
    let Ok(entries) = std::fs::read_dir(pages) else {
        return "divide".to_owned();
    };
    let ids: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with("-how-code-is-divided.md"))
        .filter_map(|name| name.split_once('-').map(|(id, _)| id.to_owned()))
        .collect();
    assert_eq!(ids.len(), 1, "one standard page on how code is divided");
    ids.into_iter().next().expect("one id")
}

/// The standing brief always; the role file only where the pack sits beside the crate.
fn solid_files(role: &str) -> Vec<std::path::PathBuf> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![crate_dir.join(format!("assets/spawn/{role}-brief.md"))];
    let agents = crate_dir.join("../plugins/teams/agents");
    if agents.is_dir() {
        files.push(agents.join(format!("{role}.md")));
    }
    files
}

#[test]
fn pack_briefs_planning_names_the_solid_page() {
    for path in solid_files("planning") {
        assert_names_the_solid_page(&path);
        let text = std::fs::read_to_string(&path).expect("the file reads");
        assert!(
            text.contains("defines or changes an interface") && text.contains("Design pages"),
            "{} does not say a ticket that defines or changes an interface names the page in its Design pages",
            path.display()
        );
    }
}

#[test]
fn pack_briefs_challenger_names_the_solid_page() {
    for path in solid_files("challenger") {
        assert_names_the_solid_page(&path);
    }
}
