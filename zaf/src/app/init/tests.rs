use std::fs;
use std::io::Cursor;
use std::process::Command;

use super::*;

fn run(dir: &Path, answers: &str) -> (Result<PathBuf, InitError>, String) {
    let mut output = Vec::new();
    let mut learn = || Ok(TAG.to_owned());
    let result = init(
        dir,
        &mut learn,
        &mut Cursor::new(answers.to_owned()),
        &mut output,
    );
    (result, String::from_utf8(output).expect("utf-8 prompts"))
}

const TAG: &str = "teams-v2.0.0";

fn run_standard(dir: &Path) -> PathBuf {
    run(dir, "bookshelf\nexample/shelf\n\ncargo test\n")
        .0
        .expect("written")
}

#[test]
fn init_writes_standard_manifest() {
    use crate::domain::Profile;
    use crate::domain::RoleName;
    use crate::manifest::{CheckTarget, Effort, RunnerName};

    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = Manifest::load(&run_standard(dir.path())).expect("loads");

    let runner = manifest
        .runners
        .get(&RunnerName::from("claude"))
        .expect("a claude runner");
    assert_eq!(
        runner.command.as_ref(),
        "claude -p {prompt} --model {model} --add-dir {root}"
    );
    assert_eq!(runner.timeout_minutes, 45);

    for (name, model, effort, profile, turns) in [
        ("programmer", "sonnet", Effort::High, Profile::Lean, 150),
        ("implement", "sonnet", Effort::High, Profile::Lean, 150),
        ("fix", "sonnet", Effort::High, Profile::Lean, 150),
        ("merge", "sonnet", Effort::High, Profile::Lean, 1),
        ("release", "opus", Effort::Xhigh, Profile::Build, 200),
    ] {
        let role = manifest
            .roles
            .get(&RoleName::from(name))
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(role.model.as_ref(), model, "{name}");
        assert_eq!(role.effort, effort, "{name}");
        assert_eq!(role.profile, profile, "{name}");
        assert_eq!(role.turns, turns, "{name}");
    }

    let pr = manifest
        .check
        .targets
        .get(&CheckTarget::Pr)
        .expect("[check.pr]");
    assert_eq!(pr.steps, ["ci"]);
    let phases = ["implement", "ci", "review", "merge"];
    for task_type in ["Task", "Bug"] {
        let list = manifest.phases.get(task_type).expect(task_type);
        let names: Vec<&str> = list.iter().map(|name| name.as_str()).collect();
        assert_eq!(names, phases, "{task_type}");
    }
    let pack = manifest.pack.expect("[pack]");
    assert_eq!(pack.tag.as_deref(), Some(TAG));
    assert!(pack.path.is_none());
    assert!(manifest.gate.is_some(), "the gate answer is still kept");
}

#[test]
fn a_tag_that_cannot_be_learned_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut learn = || {
        Err(ProfileError::Install {
            line: "claude plugin install".to_owned(),
            output: "offline".to_owned(),
        })
    };
    let result = init(
        dir.path(),
        &mut learn,
        &mut Cursor::new("a\nb/c\n\n\n".to_owned()),
        &mut Vec::new(),
    );
    assert!(matches!(result, Err(InitError::Pack(_))));
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}

/// Writes an action named `name` into the project's tier with `extra` metadata lines.
fn write_action(root: &Path, name: &str, extra: &str) {
    use std::os::unix::fs::PermissionsExt;

    let dir = root.join(".zaf/actions");
    fs::create_dir_all(&dir).expect("actions dir");
    let file = dir.join(name);
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: {name}\n# zaf 1: failed\n{extra}true\n"),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

#[test]
fn standard_manifest_passes_doctor_manifest_checks() {
    use crate::app::doctor::{doctor_with, fake::FakeCommands};
    use std::time::SystemTime;

    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = Manifest::load(&run_standard(dir.path())).expect("loads");

    // Actions that lean on the standard roles and phase lists, and nothing else.
    let groups = "# zaf group: implement, fix, merge, release, programmer\n";
    for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
        write_action(dir.path(), name, groups);
    }
    write_action(
        dir.path(),
        "implement",
        "# zaf kind: model\n# zaf ends: pr-open\n",
    );
    write_action(dir.path(), "fix", "# zaf kind: model\n# zaf ends: exit\n");
    write_action(dir.path(), "ci", "# zaf kind: wait\n# zaf ends: ci-done\n");
    write_action(
        dir.path(),
        "review",
        "# zaf kind: wait\n# zaf ends: review-posted\n",
    );
    write_action(dir.path(), "merge", "# zaf kind: zaf\n# zaf ends: merged\n");

    let commands = FakeCommands::new().with("env", "{\"loggedIn\": true}\n");
    let report = doctor_with(
        dir.path(),
        None,
        &manifest,
        &commands,
        None,
        SystemTime::now(),
    );
    let text = report.lines().join("\n");
    assert!(!text.contains("names no role"), "{text}");
    assert!(!text.contains("no runner"), "{text}");
    assert!(report.actions.iter().all(|check| check.ok), "{text}");
    assert_eq!(report.phases.len(), 8, "{text}");
    assert!(report.phases.iter().all(|check| check.ok), "{text}");
}

#[test]
fn scripted_answers_write_a_manifest_the_loader_accepts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, prompts) = run(dir.path(), "bookshelf\nexample/shelf\n\ncargo test --all\n");
    let path = result.expect("written");
    assert_eq!(path, dir.path().join(MANIFEST_FILE));
    assert!(prompts.contains("project name ["), "{prompts}");
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "bookshelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(
        forge.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf"),
        "a blank forge answer takes the board repo"
    );
    assert!(manifest.vcs.is_some());
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "cargo test --all"
    );
}

#[test]
fn a_separate_forge_repo_and_a_blank_gate_are_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(dir.path(), "bookshelf\nexample/board\nexample/code\n\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("example/code"));
    assert!(manifest.gate.is_none());
}

#[test]
fn quotes_in_an_answer_survive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(
        dir.path(),
        "the \"shelf\"\nexample/b\n\necho \"hi\" \\ done\n",
    );
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "the \"shelf\"");
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "echo \"hi\" \\ done"
    );
}

#[test]
fn an_existing_manifest_is_refused_untouched_and_unasked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(MANIFEST_FILE);
    fs::write(&path, "keep me").expect("existing manifest");
    let (result, prompts) = run(dir.path(), "a\nb\n\n\n");
    assert!(matches!(result, Err(InitError::Exists(_))));
    assert_eq!(fs::read_to_string(&path).expect("read"), "keep me");
    assert_eq!(prompts, "", "refused before the first question");
}

#[test]
fn a_blank_name_or_board_and_a_short_input_write_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        run(dir.path(), "\n\n").0,
        Err(InitError::Blank(_))
    ));
    assert!(matches!(
        run(dir.path(), "a\n\n").0,
        Err(InitError::Blank(_))
    ));
    assert!(matches!(
        run(dir.path(), "a\nb/c\n").0,
        Err(InitError::Closed(_))
    ));
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}

/// A folder `zabo` holding a git repo, with `origin` at `url` when given.
fn repo_folder(url: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temp dir");
    let dir = root.path().join("zabo");
    fs::create_dir(&dir).expect("folder");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    if let Some(url) = url {
        git(&["remote", "add", "origin", url]);
    }
    (root, dir)
}

#[test]
fn init_defaults_from_folder_and_remote() {
    for url in ["git@github.com:o/zabo.git", "https://github.com/o/zabo.git"] {
        let (_root, dir) = repo_folder(Some(url));
        let (result, prompts) = run(&dir, "\n\n\n\n");
        let manifest = Manifest::load(&result.expect("written")).expect("loads");
        assert_eq!(manifest.project.name.as_ref(), "zabo", "{url}");
        assert_eq!(
            manifest.board.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        let forge = manifest.forge.expect("a forge section");
        assert_eq!(
            forge.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        assert!(manifest.gate.is_none(), "{url}");
        assert!(prompts.contains("project name [zabo]: "), "{prompts}");
        assert!(
            prompts.contains("board repo (owner/name) [o/zabo]: "),
            "{prompts}"
        );
    }
}

#[test]
fn init_typed_answer_wins() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let (result, _) = run(&dir, "shelf\nx/board\nx/code\ncargo test\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "shelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("x/board")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("x/code"));
    assert_eq!(manifest.gate.expect("a gate").full.as_ref(), "cargo test");
}

#[test]
fn init_no_remote_requires_repo() {
    let (_root, dir) = repo_folder(None);
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank(question)) if question.starts_with("board repo")),
        "a blank board repo is refused by name"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());

    let (_root, dir) = repo_folder(Some("git@gitlab.com:o/zabo.git"));
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank(_))),
        "not GitHub: no default"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());
}

#[test]
fn github_urls_read_as_owner_name() {
    for url in [
        "git@github.com:o/n.git",
        "git@github.com:o/n",
        "ssh://git@github.com/o/n.git",
        "https://github.com/o/n.git",
        "https://github.com/o/n/",
        "https://user:tok@github.com/o/n.git\n",
    ] {
        assert_eq!(github_repo(url).as_deref(), Some("o/n"), "{url}");
    }
    for url in [
        "git@gitlab.com:o/n.git",
        "https://github.com/o",
        "/srv/repo.git",
        "",
    ] {
        assert_eq!(github_repo(url), None, "{url}");
    }
}
