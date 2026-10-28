use std::fs;

use super::*;
use crate::app::doctor::fake::FakeCommands;

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{HEAD}{extra}").parse().expect("a valid manifest")
}

#[test]
fn the_layout_exists_and_setup_is_idempotent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest("");
    let commands = FakeCommands::new();
    let first = setup(dir.path(), &state, &manifest, &commands).expect("sets up");
    assert!(first.is_ready());
    for path in layout_paths(&state) {
        assert!(path.exists(), "{}", path.display());
    }
    assert_eq!(
        fs::read_to_string(state.join(IGNORE_FILE)).expect("ignore"),
        "*\n"
    );
    assert_eq!(
        fs::read_to_string(state.join(ASKLOG_FILE)).expect("log"),
        ""
    );

    fs::write(state.join(ASKLOG_FILE), "{\"q\":1}\n").expect("a log line");
    fs::write(state.join("roles/keep.md"), "mine").expect("a role file");
    let second = setup(dir.path(), &state, &manifest, &commands).expect("sets up again");
    assert_eq!(first, second);
    assert_eq!(
        fs::read_to_string(state.join(ASKLOG_FILE)).expect("log"),
        "{\"q\":1}\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("roles/keep.md")).expect("role"),
        "mine"
    );
}

#[test]
fn the_worksheet_names_each_missing_tool() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest("[requires]\njj = \"*\"\ngh = \"*\"\nnodes = \">=1\"\nagy = \"*\"\n");
    let commands = FakeCommands::new()
        .with("nodes", "nodes 0.9.0")
        .with("agy", "agy dev");
    let report = setup(dir.path(), &state, &manifest, &commands).expect("sets up");
    assert!(!report.is_ready());
    let lines = report.worksheet_lines().join("\n");
    assert_eq!(report.worksheet.len(), 4, "{lines}");
    assert!(lines.contains("2. Install `gh`"), "{lines}");
    assert!(lines.contains("3. Install `jj`"), "{lines}");
    assert!(lines.contains("`nodes` from 0.9.0"), "{lines}");
    assert!(lines.contains("`agy --version`"), "{lines}");
    assert!(lines.contains("Done when: `jj --version`"), "{lines}");
    assert!(
        lines.ends_with("Tell me: the output of `zaf doctor`."),
        "{lines}"
    );
    assert!(
        commands
            .calls()
            .iter()
            .all(|call| call[1..] == ["--version"]),
        "setup only asks for versions, it installs nothing: {:?}",
        commands.calls()
    );
}

#[test]
fn a_declared_sibling_design_repo_is_cloned_when_missing() {
    let parent = tempfile::tempdir().expect("temp dir");
    let root = parent.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"b-design\"\n",
    );
    let commands = FakeCommands::new().with("gh", "ok");
    let report = setup(&root, &root.join(".zaf"), &manifest, &commands).expect("sets up");
    assert_eq!(report.clone, Some(CloneOutcome::Cloned));
    let clone_call = commands.calls().into_iter().next().expect("a call");
    let target = parent
        .path()
        .join("b-design")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        clone_call,
        ["gh", "repo", "clone", "example/b-design", target.as_str()]
    );

    fs::create_dir(parent.path().join("b-design")).expect("the clone");
    let again = FakeCommands::new();
    let report = setup(&root, &root.join(".zaf"), &manifest, &again).expect("sets up");
    assert_eq!(report.clone, Some(CloneOutcome::Present));
    assert!(again.calls().is_empty(), "nothing to clone, nothing run");
    assert!(report.is_ready());
}

#[test]
fn a_failed_clone_becomes_a_worksheet_step() {
    let parent = tempfile::tempdir().expect("temp dir");
    let root = parent.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"b-design\"\n",
    );
    let commands = FakeCommands::new().failing("gh", "not logged in");
    let report = setup(&root, &root.join(".zaf"), &manifest, &commands).expect("sets up");
    assert_eq!(
        report.clone,
        Some(CloneOutcome::Failed("not logged in".into()))
    );
    let lines = report.worksheet_lines().join("\n");
    assert!(lines.contains("gh repo clone example/b-design"), "{lines}");
    assert!(lines.contains("not logged in"), "{lines}");
}

#[test]
fn other_design_kinds_clone_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    fs::create_dir(dir.path().join("docs")).expect("docs");
    let commands = FakeCommands::new();
    let report =
        setup(dir.path(), &dir.path().join(".zaf"), &manifest, &commands).expect("sets up");
    assert_eq!(report.clone, None);
    assert!(commands.calls().is_empty());
}
