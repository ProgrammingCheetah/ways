//! `zaf brief` through the built binary and the fake `gh`: the `## Actions` section the verb
//! writes, and the instance and zaf home it reads from the environment.

mod common;

use std::fs;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const ACTION: &str = "#!/bin/sh\n# zaf description: DESCRIPTION\n# zaf 1: failed\ntrue\n";

fn action(description: &str) -> String {
    ACTION.replace("DESCRIPTION", description)
}

fn project() -> Project {
    let mut project = Project::new(MANIFEST);
    project
        .on(
            "issue view 4 *",
            r#"{"number":4,"title":"A task","body":"","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#,
        )
        .on("issue list *", "[]");
    write(
        &project.root,
        ".zaf/actions/slice-pr",
        &action("open the task's pull request"),
    );
    write(
        &project.home,
        "actions/notify",
        &action("tell the Engineer a task is done"),
    );
    project
}

/// The pack the run printed the path of.
fn pack(stdout: &str) -> String {
    let path = stdout.lines().next().expect("a path");
    fs::read_to_string(path).expect("pack")
}

#[test]
fn the_verb_lists_the_project_and_machine_actions() {
    let project = project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4"]));
    assert_eq!(code, 0, "{stderr}");
    let text = pack(&stdout);
    assert!(
        text.contains(
            "## Actions\n\n\
             - notify (machine): tell the Engineer a task is done\n\
             - slice-pr (project): open the task's pull request\n"
        ),
        "{text}"
    );
}

#[test]
fn the_verb_hides_what_the_instance_may_not_read() {
    let mut project = project();
    write(
        &project.root,
        ".zaf/actions/merge",
        "#!/bin/sh\n# zaf description: merge it\n# zaf mode: 750\n# zaf group: team\n# zaf 1: failed\ntrue\n",
    );
    project.env("ZAF_INSTANCE", "programmer instance 1 on #1");
    let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4"]));
    assert_eq!(code, 0, "{stderr}");
    let text = pack(&stdout);
    assert!(text.contains("- slice-pr (project)"), "{text}");
    assert!(!text.contains("- merge (project)"), "{text}");
}

#[test]
fn a_malformed_instance_fails_the_brief_naming_it() {
    for bad in ["nobody", "  ", ""] {
        let mut project = project();
        project.env("ZAF_INSTANCE", bad);
        let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4"]));
        assert_eq!(code, 1, "{bad:?}: {stdout}{stderr}");
        assert!(stderr.contains("ZAF_INSTANCE"), "{stderr}");
        assert!(stderr.contains(&format!("{bad:?}")), "{stderr}");
        assert_eq!(stdout, "");
    }
}

#[test]
fn without_a_zaf_home_only_the_project_actions_are_listed() {
    let mut project = project();
    // Neither ZAF_HOME nor HOME: the machine has no zaf home.
    project.env("ZAF_HOME", "");
    project.env("HOME", "");
    let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4"]));
    assert_eq!(code, 0, "{stderr}");
    let text = pack(&stdout);
    assert!(
        text.contains("## Actions\n\n- slice-pr (project): open the task's pull request\n\n"),
        "{text}"
    );
    assert!(!text.contains("notify"), "{text}");
}

/// A project whose Task type lists two phases, and whose task 4 is of that type.
fn phased_project() -> Project {
    let manifest = format!("{MANIFEST}\n[phases]\nTask = [\"build\", \"check\"]\n");
    let mut project = Project::new(&manifest);
    project
        .on(
            "issue view 4 *",
            r#"{"number":4,"title":"A task","body":"","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1},"state":"OPEN","comments":[],"issueType":{"name":"Task","id":"IT_1"}}"#,
        )
        .on("issue list *", "[]");
    project
}

#[test]
fn brief_refuses_an_unlisted_phase() {
    let project = phased_project();
    for phase in ["review", "nonsense"] {
        let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4", "--phase", phase]));
        assert_eq!(code, 1, "{stdout}{stderr}");
        assert!(
            stderr.contains(&format!("`{phase}` is not a phase of")),
            "{stderr}"
        );
        assert!(stderr.contains("build, check"), "{stderr}");
        assert_eq!(stdout, "");
    }
}

#[test]
fn brief_accepts_an_on_fail_phase() {
    let project = phased_project();
    write(
        &project.root,
        ".zaf/actions/check",
        "#!/bin/sh\n# zaf description: wait on CI\n# zaf 1: failed\n# zaf kind: wait\n# zaf ends: ci-done\n# zaf on-fail: fix\ntrue\n",
    );
    let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4", "--phase", "fix"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(pack(&stdout).contains("## Ticket"));
    // An action no listed phase names as its on-fail is still refused.
    let (code, _, stderr) = outcome(&project.zaf(&["brief", "4", "--phase", "nonsense"]));
    assert_eq!(code, 1, "{stderr}");
}

#[test]
fn a_listed_phase_of_a_task_with_no_pr_gets_todays_pack() {
    let project = phased_project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["brief", "4", "--phase", "check"]));
    assert_eq!(code, 0, "{stderr}");
    let text = pack(&stdout);
    for heading in ["## Diff", "## Findings", "## Hand-off"] {
        assert!(!text.contains(heading), "{text}");
    }
    assert!(text.contains("## Ticket"), "{text}");
}
