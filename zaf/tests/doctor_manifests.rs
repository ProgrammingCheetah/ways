//! `zaf doctor` and the manifest files: the retired `ZAF.toml` beside a new manifest file fails
//! the report and names both files.

mod common;

use std::fs;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

#[test]
fn doctor_fails_on_both_manifests() {
    for new in [".zaf.toml", ".zaf/config.toml"] {
        let project = Project::new(MANIFEST);
        project.provide_slice_actions();
        if new != ".zaf.toml" {
            fs::remove_file(project.root.join(".zaf.toml")).expect("remove the root manifest");
            write(&project.root, new, MANIFEST);
        }
        write(&project.root, "ZAF.toml", MANIFEST);

        let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));

        assert_eq!(code, 1, "{new}: {stdout}{stderr}");
        assert!(
            stderr.contains("ZAF.toml") && stderr.contains(new),
            "{new}: the refusal names both files: {stderr}"
        );
        assert!(
            !stdout.contains("workspaces dir"),
            "{new}: no report is printed for a refused manifest: {stdout}"
        );
    }
}

#[test]
fn doctor_passes_on_the_retired_manifest_alone() {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    fs::remove_file(project.root.join(".zaf.toml")).expect("remove the new manifest");
    write(&project.root, "ZAF.toml", MANIFEST);

    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("workspaces dir"), "{stdout}");
}
