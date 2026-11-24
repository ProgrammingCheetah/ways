//! The challenger's agent file takes away the shell and the web, so zaf's tools are its only
//! way to act (design D22, D28). Outside the ways repository, where the pack is not beside the
//! crate, nothing runs.

use std::path::Path;

const DISALLOWED: [&str; 7] = [
    "Agent",
    "Edit",
    "Write",
    "NotebookEdit",
    "Bash",
    "WebFetch",
    "WebSearch",
];

#[test]
fn challenger_agent_disallows_the_shell_and_the_web() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("../plugins/teams/agents/challenger.md");
    if !path.is_file() {
        return;
    }
    let brief = std::fs::read_to_string(&path).expect("the challenger's brief reads");
    let frontmatter = brief
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(front, _)| front)
        .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
    let line = frontmatter
        .lines()
        .find_map(|line| line.strip_prefix("disallowedTools:"))
        .expect("the frontmatter has a disallowedTools line");
    let named: Vec<&str> = line.split(',').map(str::trim).collect();
    for tool in DISALLOWED {
        assert!(
            named.contains(&tool),
            "the challenger's disallowedTools lacks {tool}: {named:?}"
        );
    }
}

/// The role text: what follows the interface page, which ends at the rule before `# The challenger`.
fn role_text() -> Option<String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("../plugins/teams/agents/challenger.md");
    if !path.is_file() {
        return None;
    }
    let brief = std::fs::read_to_string(&path).expect("the challenger's brief reads");
    let (_, role) = brief
        .split_once("\n---\n\n# The challenger")
        .unwrap_or_else(|| {
            panic!(
                "{} has no role text after the interface page",
                path.display()
            )
        });
    Some(role.to_string())
}

#[test]
fn challenger_role_reads_the_graph_before_and_after() {
    let Some(role) = role_text() else { return };
    for word in ["plan", "show", "--before"] {
        assert!(role.contains(word), "the role text does not name {word}");
    }
}

#[test]
fn challenger_role_raises_findings_with_a_proposed_answer() {
    let Some(role) = role_text() else { return };
    for word in ["board", "question", "Proposed answer:"] {
        assert!(role.contains(word), "the role text does not name {word}");
    }
}

#[test]
fn challenger_role_names_no_filing_tool() {
    let Some(role) = role_text() else { return };
    for filing in ["board file", "plan file"] {
        assert!(!role.contains(filing), "the role text names {filing}");
    }
}
