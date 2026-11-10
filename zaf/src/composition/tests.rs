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
    let composition = Composition::load_with_home(&root, None).expect("loads");
    assert_eq!(composition.manifest().project.name.as_ref(), "bookshelf");
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn a_missing_manifest_is_a_read_error() {
    let root = scratch_dir("missing");
    assert!(matches!(
        Composition::load_with_home(&root, None),
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
    let composition = Composition::discover_with_home(&deep, None).expect("finds the root");
    assert_eq!(composition.root(), root.as_path());
    let elsewhere = scratch_dir("nowhere");
    assert!(matches!(
        Composition::discover_with_home(&elsewhere, None),
        Err(CompositionError::NoProject { .. })
    ));
    fs::remove_dir_all(&root).expect("clean up");
    fs::remove_dir_all(&elsewhere).expect("clean up");
}

fn write_at(dir: &Path, name: &str, text: &str) {
    let file = dir.join(name);
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, text).expect("write");
}

#[test]
fn zaf_home_is_not_a_project() {
    let temp = tempfile::tempdir().expect("temp dir");
    let dir = temp.path().join("dir");
    let home = dir.join(".zaf");
    write_at(&home, "config.toml", FULL);
    let below = dir.join("src");
    fs::create_dir_all(&below).expect("mkdir");
    assert!(
        matches!(
            Composition::discover_with_home(&below, Some(&home)),
            Err(CompositionError::NoProject { .. })
        ),
        "the home's config.toml is the machine's, not a project's"
    );
    // The same file with another home is a project's manifest.
    let found = Composition::discover_with_home(&below, None).expect("a project");
    assert_eq!(found.root(), dir.as_path());
    // A real project manifest in that directory still makes it a project.
    write_at(&dir, ".zaf.toml", "[project]\nname = \"p\"\n");
    let found = Composition::discover_with_home(&below, Some(&home)).expect("a project");
    assert_eq!(found.root(), dir.as_path());
}

#[test]
fn workspace_copy_resolves_to_the_root() {
    for file in [".zaf.toml", ".zaf/config.toml", "ZAF.toml"] {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("repo");
        let home = temp.path().join("zaf-home");
        let copy = root.join(".zaf/workspaces/feature/0.2.0-1-2");
        write_at(&root, file, FULL);
        write_at(&copy, file, FULL);
        let inside = copy.join("src");
        fs::create_dir_all(&inside).expect("mkdir");
        let found = Composition::discover_with_home(&inside, Some(&home)).expect("finds a root");
        assert_eq!(found.root(), root.as_path(), "a copy of {file}");
    }
}

#[test]
fn a_project_folder_file_and_a_root_file_are_one_project() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("repo");
    write_at(&root, ".zaf.toml", "[project]\nname = \"p\"\n");
    write_at(&root, ".zaf/config.toml", HEAD);
    let found = Composition::discover_with_home(&root, None).expect("loads");
    assert_eq!(found.root(), root.as_path());
    assert_eq!(found.manifest().project.name.as_ref(), "p");
}

#[test]
fn legacy_beside_new_is_refused_by_discover_and_load() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("repo");
    write_at(&root, "ZAF.toml", FULL);
    write_at(&root, ".zaf.toml", FULL);
    let inside = root.join("src");
    fs::create_dir_all(&inside).expect("mkdir");
    let load = Composition::load_with_home(&root, None).expect_err("refused");
    assert!(matches!(load, ManifestError::LegacyBesideNew { .. }));
    let found = Composition::discover_with_home(&inside, None).expect_err("refused");
    let message = found.to_string();
    assert!(
        message.contains("ZAF.toml") && message.contains(".zaf.toml"),
        "{message}"
    );
}

#[test]
fn no_project_text_names_the_files_it_looked_for() {
    let temp = tempfile::tempdir().expect("temp dir");
    let error = Composition::discover_with_home(temp.path(), None).expect_err("none");
    let message = error.to_string();
    assert!(
        message.contains(".zaf.toml") && message.contains(".zaf/config.toml"),
        "{message}"
    );
}

#[test]
fn verbs_still_resolve_the_home() {
    let os = |text: &str| Some(std::ffi::OsString::from(text));
    assert_eq!(
        zaf_home_from(os("/x/zaf"), os("/u")),
        Some(PathBuf::from("/x/zaf"))
    );
    assert_eq!(
        zaf_home_from(None, os("/u")),
        Some(PathBuf::from("/u/.zaf"))
    );
    assert_eq!(
        zaf_home_from(os(""), os("/u")),
        Some(PathBuf::from("/u/.zaf"))
    );
    assert_eq!(zaf_home_from(None, None), None);
    assert_eq!(zaf_home_from(os(""), os("")), None);
}

#[test]
fn composition_tests_ignore_the_machine_home() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("project");
    let home = temp.path().join("zaf-home");
    fs::create_dir_all(&root).expect("mkdir");
    write_manifest(
        &root,
        "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n",
    );
    // A machine config that changes a manifest value: it adds the vcs the project lacks.
    write_at(&home, "config.toml", "[vcs]\nkind = \"jj\"\n");

    let with_home = Composition::load_with_home(&root, Some(&home)).expect("loads");
    assert!(
        with_home.vcs().is_ok(),
        "the machine layer is read when given"
    );

    // The tests pass no home, so the same file changes nothing for them.
    let without = Composition::load_with_home(&root, None).expect("loads");
    assert!(matches!(
        without.vcs(),
        Err(CompositionError::Absent { port: "Vcs", .. })
    ));
}

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn request(dir: &Path) -> RunRequest {
    RunRequest {
        role: RoleName::from("keeper"),
        profile: Profile::Lean,
        prompt: String::new(),
        dir: dir.to_owned(),
        env: std::collections::BTreeMap::new(),
        holder_file: None,
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
        .process_runner(&claude, Some(&ModelName::from("haiku")), None)
        .expect("builds");
    let expected = ProcessRunner::new(
        CommandLine::from("echo model=haiku"),
        Duration::from_secs(600),
    );
    assert_eq!(built, expected, "timeout_minutes = 10");
    let default = composition
        .process_runner(&RunnerName::from("local"), None, None)
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
    let composition = Composition::load_with_home(&root, None).expect("loads");
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
    let found = Composition::discover_with_home(&nested, None).expect("finds the root");
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
    let found = Composition::discover_with_home(&slice, None).expect("finds the root");
    assert_eq!(found.root(), root.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_slice_dir_joins_the_root_the_workspaces_path_and_the_slice_name() {
    let slice = SliceName::from("feature/0.1.1-37-46");
    let default = scratch_dir("slice-dir-default");
    write_manifest(&default, HEAD);
    let composition = Composition::load_with_home(&default, None).expect("loads");
    assert_eq!(
        composition.slice_dir(&slice),
        default.join(".zaf/workspaces/feature/0.1.1-37-46")
    );
    let custom = scratch_dir("slice-dir-custom");
    write_manifest(&custom, &format!("{HEAD}[workspaces]\npath = \"slices\"\n"));
    let composition = Composition::load_with_home(&custom, None).expect("loads");
    assert_eq!(
        composition.slice_dir(&slice),
        custom.join("slices/feature/0.1.1-37-46")
    );
    fs::remove_dir_all(&default).expect("clean up");
    fs::remove_dir_all(&custom).expect("clean up");
}

#[test]
fn a_manifest_outside_the_workspaces_path_is_its_own_root() {
    let parent = scratch_dir("nested-project");
    write_manifest(&parent, HEAD);
    let inner = parent.join("vendor/inner");
    fs::create_dir_all(&inner).expect("inner dir");
    write_manifest(&inner, HEAD);
    let found = Composition::discover_with_home(&inner, None).expect("finds the nearest");
    assert_eq!(found.root(), inner.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_broken_root_manifest_is_an_error_from_a_workspace() {
    // Not TOML at all, and TOML the typed manifest refuses: the default and a custom workspaces
    // path. The root's error comes from the root and from every directory of a workspace below it,
    // never the workspace copy's own healthy manifest standing in for the project.
    let cases = [
        (
            "broken-toml",
            ".zaf/workspaces",
            "[project\nname = \n".to_owned(),
        ),
        (
            "broken-type",
            ".zaf/workspaces",
            "[project]\nname = 7\n".to_owned(),
        ),
        (
            "broken-custom",
            "slices",
            "[project]\nname = 7\n[workspaces]\npath = \"slices\"\n".to_owned(),
        ),
    ];
    for (name, workspaces, broken) in cases {
        let root = scratch_dir(name);
        write_manifest(&root, &broken);
        let slice = root.join(workspaces).join("feature/0.2.0-1-2");
        let nested = slice.join("src/inner");
        fs::create_dir_all(&nested).expect("slice dirs");
        write_manifest(&slice, HEAD);
        for start in [&root, &slice, &nested] {
            let found = Composition::discover_with_home(start, None);
            assert!(
                matches!(found, Err(CompositionError::Manifest(_))),
                "{name} from {}: {:?}",
                start.display(),
                found.map(|found| found.root().to_owned())
            );
        }
        fs::remove_dir_all(&root).expect("clean up");
    }
}

#[test]
fn a_broken_manifest_outside_the_workspaces_path_does_not_break_the_project_below() {
    let parent = scratch_dir("broken-outer");
    write_manifest(&parent, "[project\nname = \n");
    let inner = parent.join("vendor/inner");
    fs::create_dir_all(&inner).expect("inner dir");
    write_manifest(&inner, HEAD);
    let found = Composition::discover_with_home(&inner, None).expect("the inner project");
    assert_eq!(found.root(), inner.as_path());
    fs::remove_dir_all(&parent).expect("clean up");
}

#[test]
fn a_role_runner_carries_its_profile_dir_its_model_and_its_effort() {
    let root = scratch_dir("role");
    // A stand-in for the model CLI: it prints what it was given and where its home is.
    write_manifest(
        &root,
        &format!(
            "{HEAD}[runners.claude]\n\
             command = \"printf '%s|%s|%s' {{model}} {{effort}} \\\"$CLAUDE_CONFIG_DIR\\\"\"\n\
             [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\n"
        ),
    );
    let composition = Composition::load_with_home(&root, None).expect("loads");
    let team = RoleName::from("team");
    let home = root.join(".zaf/profiles/build");
    assert_eq!(composition.profile_dir(Profile::Build), home);
    let runner = composition.runner_for_role(&team).expect("builds");
    let request = RunRequest {
        role: team.clone(),
        profile: Profile::Build,
        ..request(&root)
    };
    assert_eq!(
        runner.run(&request).expect("runs").transcript,
        format!("opus|xhigh|{}", home.display())
    );
    let (line, env) = composition.role_launch(&request).expect("describes");
    assert_eq!(line, "printf '%s|%s|%s' opus xhigh \"$CLAUDE_CONFIG_DIR\"");
    assert_eq!(
        env,
        std::collections::BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".to_owned(),
            home.display().to_string()
        )])
    );
    assert!(matches!(
        composition.runner_for_role(&RoleName::from("nobody")),
        Err(CompositionError::NoRole(_))
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn a_role_without_the_claude_runner_has_no_runner() {
    let root = scratch_dir("role-no-runner");
    write_manifest(&root, HEAD);
    let composition = Composition::load_with_home(&root, None).expect("loads");
    assert!(matches!(
        composition.runner_for_role(&RoleName::from("team")),
        Err(CompositionError::NoRunner(name)) if name.as_ref() == ROLE_RUNNER
    ));
    fs::remove_dir_all(&root).expect("clean up");
}

#[test]
fn a_runner_command_carries_the_project_root_single_quoted() {
    let root = scratch_dir("root-placeholder");
    write_manifest(
        &root,
        &format!("{HEAD}[runners.claude]\ncommand = \"printf '%s|%s' {{root}} {{dir}}\"\n"),
    );
    let run_dir = root.join("run");
    fs::create_dir_all(&run_dir).expect("run dir");
    let composition = Composition::load_with_home(&root, None).expect("loads");
    let runner = composition
        .runner(&RunnerName::from("claude"))
        .expect("builds");
    assert_eq!(
        runner.run(&request(&run_dir)).expect("runs").transcript,
        format!("{}|{}", composition.root().display(), run_dir.display())
    );
}

#[test]
fn a_legacy_manifest_has_a_notice_that_says_to_run_setup_and_a_new_one_has_none() {
    let temp = tempfile::tempdir().expect("temp dir");
    let legacy = temp.path().join("legacy");
    write_at(&legacy, "ZAF.toml", FULL);
    let composition = Composition::load_with_home(&legacy, None).expect("loads");
    let notice = composition.legacy_notice().expect("a notice");
    assert!(
        notice.contains("zaf setup") && notice.contains("ZAF.toml") && !notice.contains('\n'),
        "{notice}"
    );
    assert!(legacy.join("ZAF.toml").is_file(), "reading never moves it");

    let current = temp.path().join("current");
    write_at(&current, ".zaf.toml", FULL);
    let composition = Composition::load_with_home(&current, None).expect("loads");
    assert_eq!(composition.legacy_notice(), None);
}
