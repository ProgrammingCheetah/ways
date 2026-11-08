//! `zaf ask` through the built binary: the instance it reads from the environment.

mod common;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

#[test]
fn ask_refuses_a_blank_instance() {
    for bad in ["nobody", "  ", ""] {
        let mut project = Project::new(MANIFEST);
        project.env("ZAF_INSTANCE", bad);
        let (code, stdout, stderr) =
            outcome(&project.zaf(&["ask", "-t", "4", "fact: What is a task?"]));
        assert_eq!(code, 1, "{bad:?}: {stdout}{stderr}");
        assert!(stderr.contains("ZAF_INSTANCE"), "{stderr}");
        assert!(stderr.contains(&format!("{bad:?}")), "{stderr}");
        assert_eq!(stdout, "");
        // No keeper ran, and nothing was read from the board either.
        assert_eq!(project.gh_calls(), Vec::<String>::new());
        assert!(
            !project.root.join(".zaf/asklog.jsonl").exists(),
            "{bad:?}: the refused ask was logged"
        );
    }
}

#[test]
fn ask_names_the_design_paths_it_tried() {
    let manifest = format!(
        "{MANIFEST}\n[design]\nkind = \"repo-dir\"\npath = \"docs\"\nindex = \"map/pages.md\"\n"
    );
    let mut project = Project::new(&manifest);
    // A ticket that names no page, so the index is what the ask reads.
    let body = "Goal: Members export their reading list as CSV.\n\
                Owned paths: src/export/**\n\
                Interfaces: none\n\
                Design pages: none\n\
                Architecture: app over ports\n\
                Acceptance:\n  - an empty list exports the header row only\n\
                Tests: beside the use case\n\
                Security trigger: no\n\
                Model: sonnet\n\
                Size: S\n\
                Blocked by: none\n";
    let ticket = serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": body,
        "labels": [{"name": "task"}, {"name": "ready"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
    });
    project
        .on("issue view 4 *", &ticket.to_string())
        .on("issue list *", "[]");
    let root = project.root.canonicalize().expect("root");
    write(
        &root,
        "docs/1001-export.md",
        "On export, a list is written.\n",
    );

    let (code, stdout, stderr) =
        outcome(&project.zaf(&["ask", "-t", "4", "fact: What is a task?"]));

    assert_ne!(code, 0, "{stdout}{stderr}");
    let dir = root.join("docs");
    let index = dir.join("map/pages.md");
    assert!(stderr.contains(&dir.display().to_string()), "{stderr}");
    assert!(stderr.contains(&index.display().to_string()), "{stderr}");
}
