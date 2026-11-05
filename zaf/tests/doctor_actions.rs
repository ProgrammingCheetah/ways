//! `zaf doctor` and the actions: every action is reported with its validity and its mode, and an
//! invalid file fails the report, naming the file and, where there is one, the line.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{Project, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

/// An action file with the given metadata lines (without the comment marker) and a body.
fn action_text(metadata: &[&str]) -> String {
    let mut text = String::from("#!/bin/sh\n");
    for line in metadata {
        text.push_str(&format!("# zaf {line}\n"));
    }
    text.push_str("echo done\n");
    text
}

/// Writes an action to `dir/<name>`, executable or not.
fn put(dir: &Path, name: &str, metadata: &[&str], executable: bool) {
    write(dir, name, &action_text(metadata));
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(dir.join(name), fs::Permissions::from_mode(mode)).expect("mode");
}

fn project_actions(project: &Project) -> std::path::PathBuf {
    project.root.join(".zaf/actions")
}

/// The doctor's exit code and what it printed.
fn doctor(project: &Project) -> (i32, String) {
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    (code, format!("{stdout}{stderr}"))
}

#[test]
fn doctor_reports_every_action_with_its_validity_and_mode() {
    let project = Project::new(MANIFEST);
    put(
        &project_actions(&project),
        "land",
        &["description: land it", "1: refused", "mode: 750"],
        true,
    );
    put(
        &project.home.join("actions"),
        "hello",
        &["description: say hi", "1: failed"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("action land (project): ok, mode 750"),
        "{text}"
    );
    assert!(
        text.contains("action hello (machine): ok, mode 755"),
        "{text}"
    );
}

#[test]
fn doctor_fails_on_bad_metadata() {
    let project = Project::new(MANIFEST);
    let actions = project_actions(&project);
    write(&actions, "nothing", "#!/bin/sh\necho no metadata\n");
    fs::set_permissions(actions.join("nothing"), fs::Permissions::from_mode(0o755)).expect("mode");
    put(&actions, "no-code", &["description: no code one"], true);
    put(
        &actions,
        "slow",
        &["description: slow", "1: failed", "timeout: soon"],
        false,
    );
    put(&actions, "fine", &["description: fine", "1: failed"], true);

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    let nothing = actions.join("nothing");
    assert!(
        text.contains(&format!("{}:2: no metadata", nothing.display())),
        "{text}"
    );
    assert!(
        text.contains("action nothing (project): invalid, mode unknown"),
        "{text}"
    );
    assert!(
        text.contains(&format!("{}:", actions.join("no-code").display())),
        "{text}"
    );
    assert!(text.contains("code 1 is not declared"), "{text}");
    let slow = actions.join("slow");
    assert!(
        text.contains(&format!("{}:4: `timeout: soon`", slow.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("{}:1: not executable", slow.display())),
        "a file that is not executable is named with its line: {text}"
    );
    assert!(text.contains("action fine (project): ok"), "{text}");
}

#[test]
fn doctor_fails_on_a_declared_125() {
    let project = Project::new(MANIFEST);
    let actions = project_actions(&project);
    put(
        &actions,
        "refuse",
        &["description: refuses", "1: failed", "125: not run"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "{}:4: code 125 is reserved",
            actions.join("refuse").display()
        )),
        "{text}"
    );
}

#[test]
fn doctor_fails_on_a_bad_phase() {
    let project = Project::new(MANIFEST);
    let actions = project_actions(&project);
    put(
        &actions,
        "stage",
        &[
            "description: stage",
            "1: failed",
            "target: task",
            "phases: build, lunch",
        ],
        true,
    );
    put(
        &actions,
        "orphan",
        &["description: orphan", "1: failed", "phases: build"],
        true,
    );
    put(
        &actions,
        "thing",
        &["description: thing", "1: failed", "target: story"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "{}:5: unknown phase `lunch`",
            actions.join("stage").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{}:4: `phases` needs `target: task`",
            actions.join("orphan").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{}:4: `target: story`",
            actions.join("thing").display()
        )),
        "a bad target fails too: {text}"
    );
}

#[test]
fn doctor_fails_on_a_bad_mode() {
    let project = Project::new(MANIFEST);
    let actions = project_actions(&project);
    put(
        &actions,
        "octal",
        &["description: octal", "1: failed", "mode: 758"],
        true,
    );
    put(
        &actions,
        "grouped",
        &[
            "description: grouped",
            "1: failed",
            "mode: 750",
            "group: team, wizard",
        ],
        true,
    );
    put(
        &actions,
        "known",
        &[
            "description: known",
            "1: failed",
            "mode: 750",
            "group: team, programmer",
        ],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "{}:4: `mode: 758`",
            actions.join("octal").display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{}: group names no role: wizard",
            actions.join("grouped").display()
        )),
        "a group that names no role fails, naming the file: {text}"
    );
    assert!(
        text.contains("action grouped (project): invalid, mode 750"),
        "{text}"
    );
    assert!(
        text.contains("action known (project): ok, mode 750"),
        "{text}"
    );
}

#[test]
fn doctor_json_carries_each_action_and_exits_1_on_an_invalid_one() {
    let project = Project::new(MANIFEST);
    let actions = project_actions(&project);
    put(&actions, "fine", &["description: fine", "1: failed"], true);
    put(&actions, "broken", &["description: broken"], true);

    let output = project.zaf(&["--json", "doctor"]);

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let rows = report["actions"].as_array().expect("actions");
    let by_name = |name: &str| rows.iter().find(|row| row["name"] == name).expect("a row");
    assert_eq!(by_name("fine")["ok"], true);
    assert_eq!(by_name("fine")["mode"], "755");
    assert_eq!(by_name("fine")["tier"], "project");
    assert_eq!(by_name("broken")["ok"], false);
    assert!(by_name("broken")["mode"].is_null());
}
