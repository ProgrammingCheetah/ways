//! The ways repository's CI workflow (docs/zaf-actions.md §7, ruling 39): CI is the only gate on
//! the merge path, so it runs for every change to the actions and the manifest, not only to the
//! crate.

use std::fs;
use std::path::PathBuf;

/// The workflow as the repository ships it.
fn workflow() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".github/workflows/zaf.yml");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The path patterns listed under `trigger:`'s `paths:`, in file order.
fn paths_of(text: &str, trigger: &str) -> Vec<String> {
    let mut lines = text.lines();
    lines
        .by_ref()
        .find(|line| line.trim_end() == format!("  {trigger}:"))
        .unwrap_or_else(|| panic!("no {trigger} trigger"));
    let mut paths = Vec::new();
    let mut in_paths = false;
    for line in lines {
        // The next trigger or top-level key ends this one.
        if !line.starts_with("   ") {
            break;
        }
        if line.trim_end() == "    paths:" {
            in_paths = true;
        } else if in_paths {
            match line.trim().strip_prefix("- ") {
                Some(path) => paths.push(path.trim_matches('"').to_owned()),
                None => break,
            }
        }
    }
    paths
}

#[test]
fn ci_runs_for_the_actions() {
    let text = workflow();
    for trigger in ["pull_request", "push"] {
        let paths = paths_of(&text, trigger);
        for wanted in ["zaf/**", ".zaf/**", ".zaf.toml"] {
            assert!(
                paths.iter().any(|path| path == wanted),
                "{trigger} does not run for {wanted}: {paths:?}"
            );
        }
    }
}
