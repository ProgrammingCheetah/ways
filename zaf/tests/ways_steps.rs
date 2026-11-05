//! The ways repository's own gate actions (docs/zaf-actions.md §7): `gate`, `gate-light` and
//! `nodes-check` sit in `.zaf/actions/` with valid metadata and the mode §7 gives them, `zaf
//! doctor` reports none of them as a problem, and their bodies run the lines the ticket names.

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
"#;

/// The three actions, each with the one line its body runs.
const STEPS: [(&str, &str); 3] = [
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

/// The file's text and its parsed metadata.
fn parsed(name: &str) -> (String, ParsedAction) {
    let file = action_file(name);
    let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
    let parsed = parse(&file, &text).unwrap_or_else(|error| panic!("{error}"));
    (text, parsed)
}

#[test]
fn ways_steps_have_valid_metadata() {
    for (name, _) in STEPS {
        let (_, action) = parsed(name);
        let metadata = action.metadata;
        // §7: mode 755, no group, no target, no phases.
        assert_eq!(metadata.mode, ActionMode::parse("755").unwrap(), "{name}");
        assert!(metadata.group.is_empty(), "{name} names a group");
        assert!(metadata.target.is_none(), "{name} names a target");
        assert!(metadata.phases.is_empty(), "{name} names phases");
        assert!(
            !metadata.description.is_empty(),
            "{name} has no description"
        );
        assert!(!metadata.codes.is_empty(), "{name} declares no exit code");
        assert!(!metadata.codes.contains_key(&125), "{name} declares 125");

        // The file itself carries the mode §7 gives, not only the metadata line.
        let bits = fs::metadata(action_file(name))
            .expect("the action file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(bits, 0o755, "{name} is mode {bits:o}, not 755");
    }

    // `zaf doctor` reports each of them ok: copy the three into a temp project, which keeps the
    // machine's own actions and tools out of the answer.
    let project = Project::new(MANIFEST);
    let actions = project.root.join(".zaf/actions");
    fs::create_dir_all(&actions).expect("actions folder");
    for (name, _) in STEPS {
        fs::copy(action_file(name), actions.join(name)).expect("copy keeps the mode");
    }
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    let text = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "{text}");
    for (name, _) in STEPS {
        assert!(
            text.contains(&format!("action {name} (project): ok, mode 755")),
            "doctor does not report {name} ok: {text}"
        );
    }
}

#[test]
fn ways_steps_run_the_gate_and_nodes_lines() {
    for (name, line) in STEPS {
        let (text, action) = parsed(name);
        let body: Vec<&str> = text.lines().skip(action.body_line - 1).collect();
        assert!(
            body.iter().any(|body_line| body_line.starts_with(line)),
            "{name} does not run `{line}`: {body:?}"
        );
    }
}
