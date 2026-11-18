//! `zaf begin --version <v>` on the built binary: its stdout is a pipe here, so there is no
//! terminal, and it refuses with one sentence and exit 2 before it reads the board.

mod common;

use common::{Project, outcome};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

#[test]
fn begin_refuses_without_a_terminal() {
    let project = Project::new(MANIFEST);
    // The first run makes the zaf home and says so; the verb under test then runs alone.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["begin", "--version", "0.6.0"]));
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("terminal"), "{stderr}");
    assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");
}

#[test]
fn begin_refuses_a_phase_that_is_not_objectives_or_design() {
    let project = Project::new(MANIFEST);
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["begin", "--version", "0.6.0", "--phase", "review"]));
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}
