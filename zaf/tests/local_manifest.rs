//! `.zaf/local.toml` as the manifest's top layer, through the built binary: its slots win from
//! the root and from a task workspace, any other manifest setting fails a verb, and its own
//! tables stay with their readers.

mod common;

use std::fs;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[spawn]
slots = 2
"#;

/// A project whose fake board has nothing on it.
fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project.on("issue list *", "[]");
    project
}

#[test]
fn local_toml_is_read_from_the_root_in_a_workspace() {
    let project = project();
    let root = project.root.canonicalize().expect("root");
    let workspace = root.join(".zaf/workspaces/slice-1");
    // The workspace is a checkout of the project, with its own manifest and no local file.
    write(&workspace, ".zaf.toml", MANIFEST);

    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("of 2"), "no local file: {stdout}");

    write(&root, ".zaf/local.toml", "[spawn]\nslots = 4\n");
    let (code, from_root, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{from_root}{stderr}");
    assert!(from_root.contains("of 4"), "from the root: {from_root}");
    let (code, from_workspace, stderr) = outcome(&project.zaf_in(&workspace, &["status"]));
    assert_eq!(code, 0, "{from_workspace}{stderr}");
    assert!(
        from_workspace.contains("of 4"),
        "from the workspace: {from_workspace}"
    );

    // A local file in the workspace is not the one read.
    write(&workspace, ".zaf/local.toml", "[spawn]\nslots = 9\n");
    let (_, again, _) = outcome(&project.zaf_in(&workspace, &["status"]));
    assert!(again.contains("of 4"), "the root's file wins: {again}");
}

#[test]
fn local_toml_refuses_a_loosened_manifest_in_every_verb() {
    let project = project();
    for (setting, key) in [
        ("[forbidden]\ncommands = []\n", "[forbidden]"),
        ("[requires]\njj = \"*\"\n", "[requires]"),
        ("[spawn]\nguard = [90]\n", "[spawn] guard"),
        ("[spawn]\nbudget = 1\n", "[spawn] budget"),
    ] {
        write(&project.root, ".zaf/local.toml", setting);
        // `zaf doctor` exits 1 for any manifest that does not load; the others exit 2.
        for (verb, want) in [
            (&["status"][..], 2),
            (&["doctor"], 1),
            (&["guard"], 2),
            (&["report"], 2),
        ] {
            let (code, stdout, stderr) = outcome(&project.zaf(verb));
            assert_eq!(code, want, "{verb:?} with {key}: {stdout}{stderr}");
            assert!(
                stderr.contains(".zaf/local.toml") && stderr.contains(&format!("it sets {key}")),
                "{verb:?} with {key}: {stderr}"
            );
        }
    }
}

#[test]
fn local_toml_keeps_the_weekly_budget_for_the_guard() {
    let project = project();
    write(
        &project.root,
        ".zaf/local.toml",
        "[budget]\nweekly_usd = 100\n",
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["guard"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    let file = fs::read_to_string(project.root.join(".zaf/guard.json")).expect("guard.json");
    let json: serde_json::Value = serde_json::from_str(&file).expect("json");
    assert_eq!(json["weekly_usd"], 100.0, "the guard read the budget");
}

#[test]
fn local_toml_keeps_the_prices_and_auth_tables() {
    let project = project();
    write(
        &project.root,
        ".zaf/local.toml",
        "[auth]\ntoken_env = \"ZAF_TEST_TOKEN\"\n[report.prices]\nx = [1.0, 2.0, 3.0, 4.0]\n",
    );
    let (code, stdout, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    // The report still reads the table: a malformed one fails it, naming the prices.
    write(
        &project.root,
        ".zaf/local.toml",
        "[auth]\ntoken_env = \"ZAF_TEST_TOKEN\"\n[report.prices]\nx = [1]\n",
    );
    let (code, stdout, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(
        !stderr.contains("it sets"),
        "the manifest did not refuse the table: {stderr}"
    );
}
