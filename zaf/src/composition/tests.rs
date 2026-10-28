use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::{Profile, RoleName};
use crate::manifest::{ModelName, RunnerName};
use crate::ports::{GateVerdict, RunRequest};

use super::*;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zaf-composition-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create a scratch dir");
    dir
}

#[test]
fn loads_the_manifest_from_the_project_root() {
    let root = scratch_dir("load");
    let manifest_text = "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";
    fs::write(root.join(MANIFEST_FILE), manifest_text).expect("write the manifest");
    let composition = Composition::load(&root).expect("loads");
    assert_eq!(composition.manifest().project.name.as_ref(), "bookshelf");
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn a_missing_manifest_is_a_read_error() {
    let root = scratch_dir("missing");
    assert!(matches!(
        Composition::load(&root),
        Err(ManifestError::Read { .. })
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

fn write_manifest(root: &Path, text: &str) {
    fs::write(root.join(MANIFEST_FILE), text).expect("write the manifest");
}

const FULL: &str = "[project]\nname = \"bookshelf\"\n\
[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n\
[vcs]\nkind = \"jj\"\n\
[forge]\nkind = \"github\"\nrepo = \"example/b\"\n\
[design]\nkind = \"repo-dir\"\npath = \"docs/design\"\n\
[gate]\nfull = \"true\"\n\
[runners.local]\ncommand = \"cat\"\n";

#[test]
fn every_declared_port_is_built_from_its_kind() {
    let root = scratch_dir("full");
    write_manifest(&root, FULL);
    let composition = Composition::load(&root).expect("loads");
    assert!(composition.board().is_ok());
    assert!(composition.vcs().is_ok());
    assert!(
        composition.forge().is_ok(),
        "the forge repo falls back to the board's"
    );
    assert!(composition.design().expect("builds").is_some());
    assert!(composition.gate().is_ok());
    assert!(composition.runner(&RunnerName::from("local")).is_ok());
    assert_eq!(composition.state_dir(), root.join(STATE_DIR));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn an_undeclared_port_is_absent_not_a_crash() {
    let root = scratch_dir("minimal");
    write_manifest(
        &root,
        "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n",
    );
    let composition = Composition::load(&root).expect("loads");
    assert!(matches!(
        composition.vcs(),
        Err(CompositionError::Absent { port: "Vcs", .. })
    ));
    assert!(matches!(
        composition.forge(),
        Err(CompositionError::Absent { port: "Forge", .. })
    ));
    assert!(matches!(
        composition.gate(),
        Err(CompositionError::Absent { port: "Gate", .. })
    ));
    assert!(composition.design().expect("none is fine").is_none());
    assert!(matches!(
        composition.runner(&RunnerName::from("gemini")),
        Err(CompositionError::NoRunner(name)) if name.as_ref() == "gemini"
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn discover_walks_up_to_the_manifest() {
    let root = scratch_dir("discover");
    write_manifest(&root, FULL);
    let deep = root.join("src").join("inner");
    fs::create_dir_all(&deep).expect("create nested dirs");
    let composition = Composition::discover(&deep).expect("finds the root");
    assert_eq!(composition.root(), root.as_path());
    let elsewhere = scratch_dir("nowhere");
    assert!(matches!(
        Composition::discover(&elsewhere),
        Err(CompositionError::NoProject { .. })
    ));
    fs::remove_dir_all(&root).expect("clean up");
    fs::remove_dir_all(&elsewhere).expect("clean up");
}

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn request(dir: &Path) -> RunRequest {
    RunRequest {
        role: RoleName::from("keeper"),
        profile: Profile::Lean,
        prompt: String::new(),
        dir: dir.to_owned(),
    }
}

#[test]
fn a_runner_with_a_model_has_its_command_filled_and_its_timeout_from_the_manifest() {
    let root = scratch_dir("model");
    write_manifest(
        &root,
        &format!(
            "{HEAD}[runners.claude]\ncommand = \"echo model={{model}}\"\ntimeout_minutes = 10\n\
             [runners.local]\ncommand = \"echo {{model}}\"\n"
        ),
    );
    let composition = Composition::load(&root).expect("loads");
    let claude = RunnerName::from("claude");
    let haiku = composition
        .runner_with_model(&claude, &ModelName::from("haiku"))
        .expect("builds");
    assert_eq!(
        haiku.run(&request(&root)).expect("runs").transcript,
        "model=haiku\n"
    );
    let plain = composition
        .runner(&RunnerName::from("local"))
        .expect("builds");
    assert_eq!(
        plain.run(&request(&root)).expect("runs").transcript,
        "{model}\n",
        "`runner` leaves the placeholder alone"
    );
    let built = composition
        .process_runner(&claude, Some(&ModelName::from("haiku")))
        .expect("builds");
    let expected = ProcessRunner::new(
        CommandLine::from("echo model=haiku"),
        Duration::from_secs(600),
    );
    assert_eq!(built, expected, "timeout_minutes = 10");
    let default = composition
        .process_runner(&RunnerName::from("local"), None)
        .expect("builds");
    assert_eq!(
        default,
        ProcessRunner::new(
            CommandLine::from("echo {model}"),
            Duration::from_secs(45 * 60)
        ),
        "the timeout defaults to 45 minutes"
    );
    assert!(matches!(
        composition.runner_with_model(&RunnerName::from("gemini"), &ModelName::from("x")),
        Err(CompositionError::NoRunner(_))
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn the_shell_runs_lines_at_the_project_root() {
    let root = scratch_dir("shell");
    write_manifest(&root, HEAD);
    let composition = Composition::load(&root).expect("loads");
    let shell = composition.shell();
    let run = shell.run(&CommandLine::from("cat ZAF.toml")).expect("runs");
    assert_eq!(run.verdict, GateVerdict::Passed);
    assert!(run.output.starts_with("[project]"), "{}", run.output);
    let failed = shell.run(&CommandLine::from("exit 3")).expect("runs");
    assert_eq!(failed.verdict, GateVerdict::Failed);
    fs::remove_dir_all(&root).expect("clean up");
}

/// A design folder with one decided page, so a read through the port proves where it is.
fn write_design(dir: &Path) {
    fs::create_dir_all(dir).expect("design dir");
    fs::write(dir.join("index.md"), "- 1001 decided — Export\n").expect("index");
    fs::write(dir.join("1001-export.md"), "# Export\n").expect("page");
}

fn reads_page(composition: &Composition) -> bool {
    let design = composition.design().expect("builds").expect("declared");
    design.page(&"1001".parse().expect("page id")).is_ok()
}

#[test]
fn a_sibling_design_path_is_relative_to_the_project_root() {
    let parent = scratch_dir("sibling");
    let root = parent.join("project");
    fs::create_dir_all(&root).expect("project dir");
    write_manifest(
        &root,
        &format!(
            "{HEAD}[design]\nkind = \"git-sibling\"\nrepo = \"example/the-design\"\npath = \"../elsewhere\"\n"
        ),
    );
    let composition = Composition::load(&root).expect("loads");
    assert!(!reads_page(&composition), "nothing there yet");
    write_design(&parent.join("the-design"));
    assert!(
        !reads_page(&composition),
        "the repo name is not used when a path is given"
    );
    write_design(&parent.join("elsewhere"));
    assert!(
        reads_page(&composition),
        "`../elsewhere` lands beside the project"
    );
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_sibling_design_without_a_path_is_beside_the_project_under_the_repo_name() {
    let parent = scratch_dir("sibling-default");
    let root = parent.join("project");
    fs::create_dir_all(&root).expect("project dir");
    write_manifest(
        &root,
        &format!("{HEAD}[design]\nkind = \"git-sibling\"\nrepo = \"example/the-design.git\"\n"),
    );
    let composition = Composition::load(&root).expect("loads");
    assert!(!reads_page(&composition));
    write_design(&parent.join("the-design"));
    assert!(reads_page(&composition));
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_manifest_copied_into_a_slice_workspace_still_resolves_to_the_project_root() {
    let parent = scratch_dir("slice-default");
    write_manifest(&parent, HEAD);
    let slice = parent.join(".zaf/workspaces/slice-7");
    let nested = slice.join("src/inner");
    fs::create_dir_all(&nested).expect("slice dirs");
    write_manifest(&slice, HEAD);
    let found = Composition::discover(&nested).expect("finds the root");
    assert_eq!(found.root(), parent.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_custom_workspaces_path_is_honoured_by_discovery() {
    let parent = scratch_dir("slice-custom");
    let root = parent.join("project");
    let slice = root.join("slices/k");
    fs::create_dir_all(&slice).expect("slice dir");
    write_manifest(&root, &format!("{HEAD}[workspaces]\npath = \"slices\"\n"));
    write_manifest(&slice, HEAD);
    let found = Composition::discover(&slice).expect("finds the root");
    assert_eq!(found.root(), root.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_manifest_outside_the_workspaces_path_is_its_own_root() {
    let parent = scratch_dir("nested-project");
    write_manifest(&parent, HEAD);
    let inner = parent.join("vendor/inner");
    fs::create_dir_all(&inner).expect("inner dir");
    write_manifest(&inner, HEAD);
    let found = Composition::discover(&inner).expect("finds the nearest");
    assert_eq!(found.root(), inner.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}
