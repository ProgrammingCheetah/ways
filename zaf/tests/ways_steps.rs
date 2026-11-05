//! The ways repository's own actions (docs/zaf-actions.md §7): every file in `.zaf/actions/` has
//! valid metadata and the mode §7 gives it, `zaf doctor` reports none of them as a problem, and
//! the gate actions' bodies run the lines the ticket names. The list of actions is the folder
//! itself, read at run time, so a new action is covered with no edit here.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use common::{Project, outcome};
use zaf::app::actions::{ActionMode, ParsedAction, parse};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

# §7 names the release role in a group; the pack's own roles are only programmer and team.
[roles.release]
model = "opus"
effort = "high"
profile = "build"
turns = 100
"#;

/// The actions §7 gives mode 755: they run a check and name no group.
const OPEN: [&str; 3] = ["gate", "gate-light", "nodes-check"];

/// The one action only the release role runs; every other 750 action is the team's and the
/// release's.
const RELEASE_ONLY: &str = "release";

/// The three actions that run one line, each with that line.
const LINES: [(&str, &str); 3] = [
    (
        "gate",
        "cd zaf && cargo fmt --check && cargo build && cargo clippy --all-targets -- -D warnings && cargo test",
    ),
    ("gate-light", "cd zaf && cargo fmt --check && cargo test"),
    ("nodes-check", "nodes check --root \"$ZAF_ROOT\""),
];

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

fn action_file(name: &str) -> PathBuf {
    ways_root().join(".zaf/actions").join(name)
}

/// The name of every file in `.zaf/actions/`, sorted.
fn action_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(ways_root().join(".zaf/actions"))
        .expect("the actions folder")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .filter(|name: &String| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// The file's text and its parsed metadata.
fn parsed(name: &str) -> (String, ParsedAction) {
    let file = action_file(name);
    let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
    let parsed = parse(&file, &text).unwrap_or_else(|error| panic!("{error}"));
    (text, parsed)
}

/// The mode and group §7 gives an action, by its name.
fn row(name: &str) -> (&'static str, &'static [&'static str]) {
    if OPEN.contains(&name) {
        ("755", &[])
    } else if name == RELEASE_ONLY {
        ("750", &["release"])
    } else {
        ("750", &["team", "release"])
    }
}

#[test]
fn ways_steps_cover_every_action_file() {
    let names = action_names();
    // The folder is read, not listed by hand; `pr-merge` is among what it holds.
    assert!(names.iter().any(|name| name == "pr-merge"), "{names:?}");

    for name in &names {
        let (mode, group) = row(name);
        let (_, action) = parsed(name);
        let metadata = action.metadata;
        assert_eq!(metadata.mode, ActionMode::parse(mode).unwrap(), "{name}");
        assert_eq!(metadata.group, group, "{name}");
        // Phases need a target (§3): an action that names no target names no phase.
        if metadata.target.is_none() {
            assert!(metadata.phases.is_empty(), "{name} names phases, no target");
        }
        assert!(
            !metadata.description.is_empty(),
            "{name} has no description"
        );
        assert!(!metadata.codes.is_empty(), "{name} declares no exit code");
        assert!(!metadata.codes.contains_key(&125), "{name} declares 125");

        // The file itself is runnable, not only its metadata line. Git keeps one bit, so a
        // checkout is 755 whatever the metadata says: the owner digit of 750 is metadata, and
        // the file needs only the owner's execute bit for the Engineer's run.
        let bits = fs::metadata(action_file(name))
            .expect("the action file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(bits & 0o100, 0o100, "{name} is mode {bits:o}, not runnable");
        if mode == "755" {
            assert_eq!(bits, 0o755, "{name} is mode {bits:o}, not 755");
        }
    }

    // `zaf doctor` reports each of them ok: copy them into a temp project, which keeps the
    // machine's own actions and tools out of the answer. The stubs stand in for any `slice-*`
    // name the folder does not hold; a real file overwrites its stub.
    let project = Project::new(MANIFEST);
    let actions = project.root.join(".zaf/actions");
    fs::create_dir_all(&actions).expect("actions folder");
    project.provide_slice_actions();
    for name in &names {
        fs::copy(action_file(name), actions.join(name)).expect("copy keeps the mode");
    }
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    let text = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "{text}");
    for name in &names {
        let (mode, _) = row(name);
        assert!(
            text.contains(&format!("action {name} (project): ok, mode {mode}")),
            "doctor does not report {name} ok: {text}"
        );
    }
}

#[test]
fn ways_steps_run_the_gate_and_nodes_lines() {
    for (name, line) in LINES {
        let (text, action) = parsed(name);
        let body: Vec<&str> = text.lines().skip(action.body_line - 1).collect();
        assert!(
            body.iter().any(|body_line| body_line.starts_with(line)),
            "{name} does not run `{line}`: {body:?}"
        );
    }
}
