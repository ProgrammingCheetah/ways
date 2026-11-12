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

/// A manifest whose design folder is `docs` and whose index is `map/pages.md` inside it.
const DESIGN_MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[design]
kind = "repo-dir"
path = "docs"
index = "map/pages.md"
"#;

/// The task body of ticket 4, naming `pages` as its design pages.
fn ticket(pages: &str) -> String {
    let body = format!(
        "Goal: Members export their reading list as CSV.\n\
         Owned paths: src/export/**\n\
         Interfaces: none\n\
         Design pages: {pages}\n\
         Architecture: app over ports\n\
         Acceptance:\n  - an empty list exports the header row only\n\
         Tests: beside the use case\n\
         Security trigger: no\n\
         Model: sonnet\n\
         Size: S\n\
         Blocked by: none\n"
    );
    serde_json::json!({
        "number": 4,
        "title": "A task",
        "body": body,
        "labels": [{"name": "task"}, {"name": "ready"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
    })
    .to_string()
}

#[test]
fn doctor_names_a_missing_design_index_by_absolute_path() {
    let project = Project::new(DESIGN_MANIFEST);
    project.provide_slice_actions();
    let root = project.root.canonicalize().expect("root");

    // The folder is there and the index is not.
    fs::create_dir_all(root.join("docs")).expect("design dir");
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    let index = root.join("docs/map/pages.md");
    assert!(
        stdout.contains(&format!("design index not found: {}", index.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("design dir: ok ({})", root.join("docs").display())),
        "{stdout}"
    );

    // The folder is gone too.
    fs::remove_dir(root.join("docs")).expect("remove the design dir");
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stdout.contains(&format!(
            "design dir not found: {}",
            root.join("docs").display()
        )),
        "{stdout}"
    );
    // The verb made the project's actions folder before it ran, so it reads as present.
    assert!(
        stdout.contains(&format!(
            "actions dir: ok ({})",
            root.join(".zaf/actions").display()
        )),
        "{stdout}"
    );
}

#[test]
fn doctor_names_an_uncreatable_workspaces_dir_by_absolute_path() {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    let root = project.root.canonicalize().expect("root");
    // A file where the workspaces folder should be.
    write(&root, ".zaf/workspaces", "not a folder");

    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stdout.contains(&format!(
            "workspaces dir not creatable: {}",
            root.join(".zaf/workspaces").display()
        )),
        "{stdout}"
    );
}

#[test]
fn doctor_ask_and_brief_share_the_design_paths() {
    let mut project = Project::new(DESIGN_MANIFEST);
    project.provide_slice_actions();
    project
        .on("issue view 4 *", &ticket("1001"))
        .on("issue list *", "[]");
    let root = project.root.canonicalize().expect("root");
    let dir = root.join("docs");
    // A page the ticket names, and no index file anywhere.
    write(&dir, "1001-export.md", "On export, a list is written.\n");
    let old = dir.join("map/pages.md");

    // What each of the three says about the design source it could not read.
    let three = |project: &Project| {
        let (_, doctor, _) = outcome(&project.zaf(&["doctor"]));
        let (_, _, ask) = outcome(&project.zaf(&["ask", "-t", "4", "fact: What is a task?"]));
        let (_, _, brief) = outcome(&project.zaf(&["brief", "4"]));
        [("doctor", doctor), ("ask", ask), ("brief", brief)]
    };

    // The index named by `[design] index` is the one all three look for.
    for (who, text) in three(&project) {
        assert!(
            text.contains(&old.display().to_string()),
            "{who} did not use {}: {text}",
            old.display()
        );
        assert!(text.contains(&dir.display().to_string()), "{who}: {text}");
    }

    // Change the index: all three follow it, none still names the old one.
    let moved = DESIGN_MANIFEST.replace("map/pages.md", "other/toc.md");
    fs::write(root.join(".zaf.toml"), moved).expect("manifest");
    let new = dir.join("other/toc.md");
    for (who, text) in three(&project) {
        assert!(
            text.contains(&new.display().to_string()),
            "{who} did not use {}: {text}",
            new.display()
        );
        assert!(
            !text.contains(&old.display().to_string()),
            "{who} still names the old index: {text}"
        );
    }
}

#[test]
fn legacy_beside_new_names_the_file_to_keep() {
    for new in [".zaf.toml", ".zaf/config.toml"] {
        let project = Project::new(MANIFEST);
        project.provide_slice_actions();
        let root = project.root.canonicalize().expect("root");
        if new != ".zaf.toml" {
            fs::remove_file(root.join(".zaf.toml")).expect("remove the root manifest");
            write(&root, new, MANIFEST);
        }
        write(&root, "ZAF.toml", MANIFEST);

        let (code, _, stderr) = outcome(&project.zaf(&["doctor"]));

        assert_eq!(code, 1, "{new}: {stderr}");
        let legacy = root.join("ZAF.toml").display().to_string();
        let kept = root.join(new).display().to_string();
        assert!(stderr.contains(&legacy), "{new}: {stderr}");
        assert!(stderr.contains(&kept), "{new}: {stderr}");
        for want in [
            "the new file is the one to keep",
            "move what ZAF.toml adds into it",
            &format!("then delete {legacy}"),
        ] {
            assert!(stderr.contains(want), "{new}: {want}: {stderr}");
        }
    }
}

#[test]
fn doctor_from_a_task_workspace_checks_the_root_paths() {
    let project = Project::new(DESIGN_MANIFEST);
    project.provide_slice_actions();
    let root = project.root.canonicalize().expect("root");
    fs::create_dir_all(root.join("docs/map")).expect("design dir");
    write(&root, "docs/map/pages.md", "");
    let workspace = root.join(".zaf/workspaces/slice-1");
    // The workspace is a checkout of the project: it holds its own manifest, but the paths
    // checked are the root's.
    write(&workspace, ".zaf.toml", DESIGN_MANIFEST);

    let (code, from_root, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{from_root}{stderr}");
    let (code, from_workspace, stderr) = outcome(&project.zaf_in(&workspace, &["doctor"]));
    assert_eq!(code, 0, "{from_workspace}{stderr}");

    for want in [
        format!("design dir: ok ({})", root.join("docs").display()),
        format!(
            "design index: ok ({})",
            root.join("docs/map/pages.md").display()
        ),
        format!("actions dir: ok ({})", root.join(".zaf/actions").display()),
        root.join(".zaf/workspaces").display().to_string(),
    ] {
        assert!(from_root.contains(&want), "root: {want}: {from_root}");
        assert!(
            from_workspace.contains(&want),
            "workspace: {want}: {from_workspace}"
        );
    }
    assert!(
        !from_workspace.contains(&format!("{}/", workspace.display())),
        "no path under the workspace is checked: {from_workspace}"
    );
    assert!(
        !workspace.join(".zaf").exists(),
        "nothing made in the workspace"
    );
}
