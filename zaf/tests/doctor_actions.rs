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

/// A project that provides the four `slice-*` actions, so a test sees only the failure it makes.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    project
}

/// The doctor's exit code and what it printed.
fn doctor(project: &Project) -> (i32, String) {
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    (code, format!("{stdout}{stderr}"))
}

#[test]
fn doctor_reports_every_action_with_its_validity_and_mode() {
    let project = project();
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
    let project = project();
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
fn an_unknown_hook_event_is_named() {
    let project = project();
    let hooks = project_actions(&project).join("hooks");
    let machine_hooks = project.home.join("actions/hooks");
    let meta = ["description: a hook", "1: failed"];
    put(&hooks, "pre-tick", &meta, true);
    put(&machine_hooks, "post-phase", &meta, true);
    put(&hooks, "on-merge", &meta, true);

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "{}:1: unknown hook event on-merge",
            hooks.join("on-merge").display()
        )),
        "{text}"
    );
    assert!(
        text.contains("action hooks/on-merge (project): invalid"),
        "{text}"
    );
    // A known event is checked as an action is, and passes.
    assert!(
        text.contains("action hooks/pre-tick (project): ok, mode 755"),
        "{text}"
    );
    assert!(
        text.contains("action hooks/post-phase (machine): ok, mode 755"),
        "{text}"
    );

    // Its metadata is held to an action's: no description, no hook.
    fs::remove_file(hooks.join("on-merge")).expect("remove");
    put(&hooks, "post-tick", &["1: failed"], true);
    let (code, text) = doctor(&project);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!("{}:", hooks.join("post-tick").display())),
        "{text}"
    );
    assert!(!text.contains("unknown hook event"), "{text}");
}

#[test]
fn doctor_fails_on_a_declared_125() {
    let project = project();
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
    let project = project();
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
    let project = project();
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
    let project = project();
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

#[test]
fn doctor_lists_shadowing() {
    let project = project();
    let machine = project.home.join("actions");
    let metadata = ["description: land it", "1: refused", "mode: 750"];
    put(&machine, "land", &metadata, true);
    put(&project_actions(&project), "land", &metadata, true);

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "a shadow does not fail the report: {text}");
    assert!(
        text.contains("action land (project): ok, mode 750"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "shadows the machine file {}",
            machine.join("land").display()
        )),
        "{text}"
    );
    assert!(
        !text.contains("no effect"),
        "same bits widen nothing: {text}"
    );
}

#[test]
fn doctor_says_a_shadow_does_not_widen() {
    let project = project();
    put(
        &project.home.join("actions"),
        "land",
        &["description: land it", "1: refused", "mode: 750"],
        true,
    );
    put(
        &project_actions(&project),
        "land",
        &["description: land it", "1: refused", "mode: 755"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("mode 755 has run bits wider than mode 750"),
        "{text}"
    );
    assert!(text.contains("the extra bits have no effect"), "{text}");
}

#[test]
fn doctor_fails_on_an_unshadowable_name() {
    let project = project();
    let machine = project.home.join("actions");
    put(
        &machine,
        "fixed",
        &["description: fixed", "1: failed", "mode: 555"],
        true,
    );
    put(
        &project_actions(&project),
        "fixed",
        &["description: mine", "1: failed", "mode: 555"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "{} may not shadow {}: its mode 555 has no owner w",
            project_actions(&project).join("fixed").display(),
            machine.join("fixed").display()
        )),
        "{text}"
    );
    assert!(text.contains("action fixed (project): invalid"), "{text}");
}

/// Writes an executable action to `dir/<name>` with a valid header and the given body.
fn put_body(dir: &Path, name: &str, body: &str) {
    write(
        dir,
        name,
        &format!("#!/bin/sh\n# zaf description: {name}\n# zaf 1: failed\n{body}\n"),
    );
    fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).expect("mode");
}

/// The four `slice-*` names.
const SLICE: [&str; 4] = ["slice-open", "slice-pr", "slice-merge", "slice-drop"];

#[test]
fn doctor_lists_needed_names_with_their_tier() {
    let manifest = format!("{MANIFEST}\n[check.pr]\nsteps = [\"ci\", \"action:lint?flagged\"]\n");
    let project = Project::new(&manifest);
    let (project_dir, machine_dir) = (project_actions(&project), project.home.join("actions"));
    for name in &SLICE[..2] {
        put_body(&project_dir, name, "true");
    }
    // `slice-drop` is in both tiers: the project's file is the one that runs.
    for name in &SLICE[2..] {
        put_body(&machine_dir, name, "true");
    }
    put_body(&project_dir, "slice-drop", "true");
    put_body(&machine_dir, "lint", "true");

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "{text}");
    for (name, tier) in [
        ("slice-open", "project"),
        ("slice-pr", "project"),
        ("slice-merge", "machine"),
        ("slice-drop", "project"),
    ] {
        assert!(
            text.contains(&format!(
                "needed {name} ({tier}): ok, needed by the slice verbs"
            )),
            "{text}"
        );
    }
    assert!(
        text.contains("needed lint (machine): ok, needed by [check.pr] step `action:lint`"),
        "an action step names its target and step: {text}"
    );

    let output = project.zaf(&["--json", "doctor"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let rows = report["needed"].as_array().expect("needed");
    let by_name = |name: &str| rows.iter().find(|row| row["name"] == name).expect("a row");
    assert_eq!(by_name("slice-merge")["tier"], "machine");
    assert_eq!(by_name("slice-drop")["tier"], "project");
    assert_eq!(by_name("lint")["ok"], true);
}

#[test]
fn doctor_fails_on_a_missing_needed_name() {
    let manifest = format!("{MANIFEST}\n[check.pr]\nsteps = [\"action:ghost\"]\n");
    let project = Project::new(&manifest);
    let actions = project_actions(&project);
    for name in &SLICE[..3] {
        put_body(&actions, name, "true");
    }

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("needed slice-drop: no provider, needed by the slice verbs"),
        "{text}"
    );
    assert!(
        text.contains("needed ghost: no provider, needed by [check.pr] step `action:ghost`"),
        "{text}"
    );
    assert!(
        text.contains("needed slice-open (project): ok"),
        "a name that is provided still prints: {text}"
    );

    let output = project.zaf(&["--json", "doctor"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let rows = report["needed"].as_array().expect("needed");
    let ghost = rows
        .iter()
        .find(|row| row["name"] == "ghost")
        .expect("ghost");
    assert_eq!(ghost["ok"], false);
    assert!(ghost["tier"].is_null());
}

#[test]
fn doctor_lists_the_missing_slice_actions() {
    // No `.zaf/actions` at all: the four slice names are the verbs' needs, and nothing provides them.
    let project = Project::new(MANIFEST);
    assert!(!project_actions(&project).exists());

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    for name in SLICE {
        assert!(
            text.contains(&format!(
                "needed {name}: no provider, needed by the slice verbs"
            )),
            "{name} is listed as missing: {text}"
        );
    }

    let output = project.zaf(&["--json", "doctor"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let rows = report["needed"].as_array().expect("needed");
    let mut names: Vec<&str> = rows.iter().filter_map(|row| row["name"].as_str()).collect();
    names.sort_unstable();
    let mut expected = SLICE.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected, "only the four slice names are needed");
    assert!(
        rows.iter()
            .all(|row| row["ok"] == false && row["tier"].is_null())
    );
}

#[test]
fn doctor_finds_names_in_gate_lines() {
    let manifest = format!(
        "{MANIFEST}\n[gate]\nfull = \"zaf do gate-full && zaf do gate-more\"\n\
         light = \"cd zaf && zaf do gate-light\"\ntests = \"zaf do gate-tests {{name}}\"\n"
    );
    let project = Project::new(&manifest);
    let (project_dir, machine_dir) = (project_actions(&project), project.home.join("actions"));
    for name in SLICE {
        put_body(&project_dir, name, "true");
    }
    put_body(&project_dir, "gate-full", "true");
    put_body(&machine_dir, "gate-tests", "true");

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("needed gate-full (project): ok, needed by [gate] full `zaf do gate-full`"),
        "{text}"
    );
    assert!(
        text.contains(
            "needed gate-tests (machine): ok, needed by [gate] tests `zaf do gate-tests`"
        ),
        "{text}"
    );
    assert!(
        text.contains("needed gate-light: no provider, needed by [gate] light `zaf do gate-light`"),
        "{text}"
    );
    assert!(
        text.contains("needed gate-more: no provider, needed by [gate] full `zaf do gate-more`"),
        "a second call on one line is found: {text}"
    );

    put_body(&project_dir, "gate-light", "true");
    put_body(&project_dir, "gate-more", "true");
    let (code, text) = doctor(&project);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn doctor_finds_names_in_action_bodies() {
    let project = project();
    let (project_dir, machine_dir) = (project_actions(&project), project.home.join("actions"));
    put_body(
        &project_dir,
        "land",
        "echo start\nzaf do build && zaf do --tier machine push origin\n\
         # later: zaf do commented\nzaf do \"$name\"",
    );
    put_body(&machine_dir, "hello", "zaf do greet");
    put_body(&project_dir, "build", "true");
    put_body(&project_dir, "push", "true");

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "needed build (project): ok, needed by action land (project): `zaf do build`"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "needed push (project): missing, needed by action land (project): \
             `zaf do --tier machine push`"
        ),
        "a machine call needs the machine's file, whatever the project has: {text}"
    );
    assert!(
        text.contains("needs the machine's file, and the machine has none"),
        "{text}"
    );
    assert!(
        text.contains(
            "needed commented: no provider, needed by action land (project): `zaf do commented`"
        ),
        "a commented call is found too: {text}"
    );
    assert!(
        text.contains(
            "needed greet: no provider, needed by action hello (machine): `zaf do greet`"
        ),
        "a machine action's body is scanned too: {text}"
    );
    assert!(!text.contains("needed $"), "a variable is no name: {text}");
    assert!(!text.contains("needed \""), "a variable is no name: {text}");

    put_body(&machine_dir, "push", "true");
    put_body(&project_dir, "commented", "true");
    put_body(&project_dir, "greet", "true");
    let (code, text) = doctor(&project);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("needed push (project): ok"), "{text}");
}

#[test]
fn doctor_lists_machine_dependencies() {
    let manifest = format!(
        "{MANIFEST}\n[check.pr]\nsteps = [\"ci\", \"action:lint\"]\n\
         [gate]\nfull = \"zaf do gate-full\"\n"
    );
    let project = Project::new(&manifest);
    let (project_dir, machine_dir) = (project_actions(&project), project.home.join("actions"));
    // Two slice names come from the machine, two from the project.
    for name in &SLICE[..2] {
        put_body(&machine_dir, name, "true");
    }
    for name in &SLICE[2..] {
        put_body(&project_dir, name, "true");
    }
    put_body(&machine_dir, "lint", "true");
    put_body(&project_dir, "gate-full", "true");
    put_body(
        &project_dir,
        "land",
        "zaf do build && zaf do gate-full && zaf do --tier machine push",
    );
    put_body(&machine_dir, "build", "true");
    // The project's own `push` shadows the machine's; only the named machine call depends on it.
    put_body(&project_dir, "push", "true");
    put_body(&machine_dir, "push", "true");
    // The machine's own steps are the machine's, not the project's dependencies.
    put_body(&machine_dir, "hello", "zaf do greet");
    put_body(&machine_dir, "greet", "true");

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "{text}");
    for (action, step) in [
        ("lint", "[check.pr] step `action:lint`"),
        ("slice-open", "the slice verbs"),
        ("slice-pr", "the slice verbs"),
        ("build", "action land (project): `zaf do build`"),
        (
            "push",
            "action land (project): `zaf do --tier machine push`",
        ),
    ] {
        let at = text
            .find(&format!("action {action} ("))
            .unwrap_or_else(|| panic!("no action {action}: {text}"));
        let section: Vec<&str> = text[at..].lines().take(3).collect();
        assert!(
            section
                .iter()
                .any(|line| *line == format!("  the project depends on this machine: {step}")),
            "{action} lists {step}: {text}"
        );
    }
    assert_eq!(
        text.matches("the project depends on this machine").count(),
        5,
        "a project file, a shadowed name no step reaches and a machine's own steps are no dependency: {text}"
    );

    let output = project.zaf(&["--json", "doctor"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let rows = report["actions"].as_array().expect("actions");
    let row = |name: &str| rows.iter().find(|row| row["name"] == name).expect("a row");
    assert_eq!(
        row("lint")["machine_dependents"],
        serde_json::json!(["[check.pr] step `action:lint`"])
    );
    assert_eq!(row("greet")["machine_dependents"], serde_json::json!([]));
    assert_eq!(
        row("gate-full")["machine_dependents"],
        serde_json::json!([])
    );
}

#[test]
fn doctor_names_a_bad_on_fail() {
    let project = project();
    let actions = project_actions(&project);
    let wait = |name: &str, fix: &str| {
        put(
            &actions,
            name,
            &[
                "description: wait",
                "1: failed",
                "kind: wait",
                "ends: ci-done",
                &format!("on-fail: {fix}"),
            ],
            true,
        );
    };
    put(
        &actions,
        "fix",
        &[
            "description: fix it",
            "1: failed",
            "kind: model",
            "ends: exit",
        ],
        true,
    );
    put(
        &actions,
        "ci",
        &[
            "description: ci",
            "1: failed",
            "kind: wait",
            "ends: ci-done",
        ],
        true,
    );
    put(
        &actions,
        "plain",
        &["description: plain", "1: failed"],
        true,
    );
    wait("good", "fix");
    wait("missing", "nobody");
    wait("to-wait", "ci");
    wait("to-plain", "plain");
    put(
        &actions,
        "no-fail",
        &[
            "description: no fail",
            "1: failed",
            "kind: wait",
            "ends: merged",
        ],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    let named = |name: &str| actions.join(name).display().to_string();
    for (name, reason) in [
        ("missing", "on-fail `nobody` names no action"),
        (
            "to-wait",
            "on-fail `ci` names an action of kind wait, not model",
        ),
        (
            "to-plain",
            "on-fail `plain` names an action with no kind, not model",
        ),
    ] {
        assert!(
            text.contains(&format!("{}: {reason}", named(name))),
            "{name}: {text}"
        );
        assert!(
            text.contains(&format!("action {name} (project): invalid")),
            "{name}: {text}"
        );
    }
    for name in ["good", "no-fail", "fix", "ci"] {
        assert!(
            text.contains(&format!("action {name} (project): ok")),
            "{name}: {text}"
        );
    }
}

/// A project whose manifest lists `names` as the phases of task type `Task`, with the four
/// `slice-*` actions provided.
fn phased(names: &[&str]) -> Project {
    let list = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let project = Project::new(&format!("{MANIFEST}\n[phases]\nTask = [{list}]\n"));
    project.provide_slice_actions();
    project
}

#[test]
fn doctor_names_a_missing_phase_action() {
    let project = phased(&["implement", "nobody"]);
    put(
        &project_actions(&project),
        "implement",
        &[
            "description: build",
            "1: failed",
            "kind: model",
            "ends: exit",
        ],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("[phases] Task: action `nobody` is provided by no tier"),
        "{text}"
    );
    assert!(
        text.contains("phase Task implement (project): ok"),
        "{text}"
    );
    assert!(text.contains("phase Task nobody: no provider"), "{text}");
}

#[test]
fn doctor_names_a_phase_action_without_kind() {
    let project = phased(&["plain"]);
    put(
        &project_actions(&project),
        "plain",
        &["description: plain", "1: failed"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("[phases] Task: action `plain` has no `kind`"),
        "{text}"
    );
    assert!(
        text.contains("phase Task plain (project): invalid"),
        "{text}"
    );
}

#[test]
fn doctor_names_a_phase_action_without_ends() {
    let project = phased(&["open-ended"]);
    put(
        &project_actions(&project),
        "open-ended",
        &["description: open ended", "1: failed", "kind: wait"],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("[phases] Task: action `open-ended` is invalid"),
        "{text}"
    );
    assert!(text.contains("no `ends`"), "{text}");
}

#[test]
fn doctor_passes_a_phase_list_of_whole_actions() {
    let project = phased(&["implement", "ci"]);
    let actions = project_actions(&project);
    put(
        &actions,
        "implement",
        &[
            "description: build",
            "1: failed",
            "kind: model",
            "ends: exit",
        ],
        true,
    );
    put(
        &project.home.join("actions"),
        "ci",
        &[
            "description: ci",
            "1: failed",
            "kind: wait",
            "ends: ci-done",
        ],
        true,
    );

    let (code, text) = doctor(&project);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("phase Task implement (project): ok"),
        "{text}"
    );
    assert!(text.contains("phase Task ci (machine): ok"), "{text}");
}
