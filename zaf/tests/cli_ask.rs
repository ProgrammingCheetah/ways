//! `zaf ask` through the built binary: the instance it reads from the environment.

mod common;

use common::{Project, outcome};

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
