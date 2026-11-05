use std::fs;

use super::*;
use crate::app::doctor::fake::FakeCommands;

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{HEAD}{extra}").parse().expect("a valid manifest")
}

/// A project root and a zaf home side by side in one temp dir.
fn places() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("project");
    let home = dir.path().join("zaf-home");
    fs::create_dir(&root).expect("project dir");
    (dir, root, home)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("a readable file")
}

#[test]
fn ensure_homes_makes_folders_and_files_and_changes_nothing_the_second_time() {
    let (_dir, root, home) = places();
    let first = ensure_homes(Some(&root), Some(&home)).expect("makes the homes");
    for dir in PROJECT_DIRS {
        assert!(root.join(".zaf").join(dir).is_dir(), "{dir}");
    }
    assert!(home.join("actions").is_dir());
    assert_eq!(read(&home.join("config.toml")), HOME_CONFIG);
    assert_eq!(
        read(&root.join(".zaf/.gitignore")),
        "*\n!.gitignore\n!config.toml\n!actions/\n!actions/**\n!roles/\n!roles/**\n"
    );
    assert_eq!(first.ignore, Some(IgnoreState::Written));
    assert_eq!(first.touched, [root.join(".zaf"), home.clone()]);
    assert!(first.notice().is_some());

    fs::write(home.join("config.toml"), "# mine\n").expect("an edit");
    fs::write(root.join(".zaf/roles/keep.md"), "mine").expect("a role file");
    let second = ensure_homes(Some(&root), Some(&home)).expect("again");
    assert!(second.made.is_empty(), "{:?}", second.made);
    assert_eq!(second.ignore, Some(IgnoreState::Current));
    assert_eq!(second.notice(), None);
    assert_eq!(read(&home.join("config.toml")), "# mine\n");
    assert_eq!(read(&root.join(".zaf/roles/keep.md")), "mine");
}

#[test]
fn the_home_config_is_all_comments_so_it_adds_no_setting() {
    for line in HOME_CONFIG.lines() {
        assert!(line.is_empty() || line.starts_with('#'), "{line}");
    }
}

#[test]
fn outside_a_project_only_the_zaf_home_is_made() {
    let (dir, _root, home) = places();
    let homes = ensure_homes(None, Some(&home)).expect("makes the home");
    assert_eq!(homes.ignore, None);
    assert_eq!(homes.touched, std::slice::from_ref(&home));
    assert!(home.join("actions").is_dir());
    assert!(!dir.path().join(".zaf").exists());
}

#[test]
fn a_missing_home_alone_is_made_and_named() {
    let (_dir, root, home) = places();
    ensure_homes(Some(&root), None).expect("project only");
    assert!(!home.exists());
    let homes = ensure_homes(Some(&root), Some(&home)).expect("home now");
    assert_eq!(homes.touched, [home]);
}

#[test]
fn ensure_homes_keeps_every_kind_of_old_gitignore() {
    for (old, state) in [
        ("*\n", IgnoreState::Star),
        ("target/\n", IgnoreState::Foreign),
        ("*\n!keep\n", IgnoreState::Foreign),
    ] {
        let (_dir, root, home) = places();
        fs::create_dir(root.join(".zaf")).expect("state dir");
        fs::write(root.join(".zaf/.gitignore"), old).expect("an old file");
        let homes = ensure_homes(Some(&root), Some(&home)).expect("makes the homes");
        assert_eq!(homes.ignore, Some(state), "{old:?}");
        assert_eq!(read(&root.join(".zaf/.gitignore")), old);
    }
}

#[test]
fn setup_replaces_an_old_star_gitignore_and_keeps_another() {
    let (_dir, root, home) = places();
    fs::create_dir(root.join(".zaf")).expect("state dir");
    fs::write(root.join(".zaf/.gitignore"), "*\n").expect("an old file");
    let commands = FakeCommands::new();
    let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up");
    assert_eq!(report.homes.ignore, Some(IgnoreState::Replaced));
    assert!(report.is_ready(), "{:?}", report.worksheet);
    assert_eq!(read(&root.join(".zaf/.gitignore")), ignore_text());

    fs::write(root.join(".zaf/.gitignore"), "# mine\n*\n").expect("a foreign file");
    let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up again");
    assert_eq!(report.homes.ignore, Some(IgnoreState::Foreign));
    assert_eq!(read(&root.join(".zaf/.gitignore")), "# mine\n*\n");
    let lines = report.worksheet_lines().join("\n");
    assert!(lines.contains("Merge these lines"), "{lines}");
    assert!(lines.contains("!actions/**"), "{lines}");
}

#[test]
fn a_root_gitignore_of_the_whole_folder_is_a_step_but_a_star_is_not() {
    let (_dir, root, home) = places();
    let commands = FakeCommands::new();
    for ignoring in [".zaf/\n", "/.zaf\n", "target/\n.zaf\n"] {
        fs::write(root.join(".gitignore"), ignoring).expect("a root ignore file");
        let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up");
        let lines = report.worksheet_lines().join("\n");
        assert!(lines.contains("root `.gitignore`"), "{ignoring:?} {lines}");
        assert!(!report.is_ready());
    }
    for fine in [".zaf/*\n", "target/\n", ".zaf/runs/\n"] {
        fs::write(root.join(".gitignore"), fine).expect("a root ignore file");
        let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up");
        assert!(report.worksheet.is_empty(), "{fine:?}");
    }
}

#[test]
fn setup_is_idempotent() {
    let (_dir, root, home) = places();
    let manifest = manifest("");
    let commands = FakeCommands::new();
    let first = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
    assert!(first.is_ready());
    fs::write(root.join(".zaf/roles/keep.md"), "mine").expect("a role file");
    let second = setup(&root, Some(&home), &manifest, &commands).expect("sets up again");
    assert!(second.homes.made.is_empty());
    assert_eq!(first.worksheet, second.worksheet);
    assert_eq!(
        read(&root.join(".zaf/roles/keep.md")),
        "mine",
        "a file already there is never rewritten"
    );
}

#[test]
fn the_worksheet_names_each_missing_tool() {
    let (_dir, root, home) = places();
    let manifest = manifest("[requires]\njj = \"*\"\ngh = \"*\"\nnodes = \">=1\"\nagy = \"*\"\n");
    let commands = FakeCommands::new()
        .with("nodes", "nodes 0.9.0")
        .with("agy", "agy dev");
    let report = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
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
    let (dir, root, home) = places();
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    let commands = FakeCommands::new().with("gh", "ok");
    let report = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
    assert_eq!(report.clone, Some(CloneOutcome::Cloned));
    let clone_call = commands.calls().into_iter().next().expect("a call");
    let target = root.join("../b-design").to_string_lossy().into_owned();
    assert_eq!(
        clone_call,
        ["gh", "repo", "clone", "example/b-design", target.as_str()]
    );

    fs::create_dir(dir.path().join("b-design")).expect("the clone");
    fs::write(dir.path().join("b-design/index.md"), "").expect("the clone's index");
    let again = FakeCommands::new();
    let report = setup(&root, Some(&home), &manifest, &again).expect("sets up");
    assert_eq!(report.clone, Some(CloneOutcome::Present));
    assert!(again.calls().is_empty(), "nothing to clone, nothing run");
    assert!(report.is_ready());
}

#[test]
fn a_failed_clone_becomes_a_worksheet_step() {
    let (_dir, root, home) = places();
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    let commands = FakeCommands::new().failing("gh", "not logged in");
    let report = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
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
    let (_dir, root, home) = places();
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    fs::create_dir(root.join("docs")).expect("docs");
    let commands = FakeCommands::new();
    let report = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
    assert_eq!(report.clone, None);
    assert!(commands.calls().is_empty());
}

#[test]
fn setup_links_the_home_credentials_into_a_profile_already_there() {
    let (dir, root, home) = places();
    let claude_home = dir.path().join("claude-home");
    fs::create_dir(&claude_home).expect("claude home");
    let source = claude_home.join(profile::CREDENTIALS_FILE);
    fs::write(&source, "{}").expect("credentials");
    let build = root.join(".zaf/profiles/build");
    fs::create_dir_all(&build).expect("the build profile");
    let manifest = manifest("");
    let commands = FakeCommands::new().with("env", "{\"loggedIn\": true}\n");

    let first =
        setup_with(&root, Some(&home), &manifest, &commands, Some(&claude_home)).expect("sets up");

    let link = build.join(profile::CREDENTIALS_FILE);
    assert_eq!(fs::read_link(&link).expect("a symlink"), source);
    assert!(
        first
            .doctor
            .lines()
            .contains(&"profile build: login ok (shared)".to_owned()),
        "doctor runs after the link: {:?}",
        first.doctor.lines()
    );
    assert!(
        !root.join(".zaf/profiles/lean").exists(),
        "a profile not generated stays for `zaf profile init`"
    );
    setup_with(&root, Some(&home), &manifest, &commands, Some(&claude_home)).expect("again");
    assert_eq!(fs::read_link(&link).expect("a symlink"), source);
}

#[test]
fn setup_leaves_a_copy_alone_and_links_nothing_without_a_login_at_home() {
    let (dir, root, home) = places();
    let claude_home = dir.path().join("claude-home");
    fs::create_dir(&claude_home).expect("claude home");
    let build = root.join(".zaf/profiles/build");
    let lean = root.join(".zaf/profiles/lean");
    fs::create_dir_all(&build).expect("the build profile");
    fs::create_dir_all(&lean).expect("the lean profile");
    let manifest = manifest("");
    let commands = FakeCommands::new().with("env", "ok");

    setup_with(&root, Some(&home), &manifest, &commands, Some(&claude_home)).expect("sets up");
    assert!(fs::symlink_metadata(build.join(profile::CREDENTIALS_FILE)).is_err());

    fs::write(claude_home.join(profile::CREDENTIALS_FILE), "{}").expect("logs in");
    let copy = build.join(profile::CREDENTIALS_FILE);
    fs::write(&copy, "my copy").expect("a copy");
    setup_with(&root, Some(&home), &manifest, &commands, Some(&claude_home)).expect("again");
    assert_eq!(read(&copy), "my copy");
    assert!(fs::symlink_metadata(&copy).expect("there").is_file());
    assert!(
        fs::symlink_metadata(lean.join(profile::CREDENTIALS_FILE))
            .expect("linked")
            .is_symlink()
    );
}

#[test]
fn the_legacy_manifest_is_renamed_for_the_engineer_and_a_second_call_does_nothing() {
    let (_dir, root, _home) = places();
    fs::write(root.join("ZAF.toml"), HEAD).expect("legacy manifest");

    let moved = move_legacy_manifest(&root, None, false).expect("moves");
    assert_eq!(moved, LegacyMove::Moved);
    assert_eq!(moved.line().as_deref(), Some("moved ZAF.toml to .zaf.toml"));
    assert!(!root.join("ZAF.toml").exists());
    assert_eq!(read(&root.join(".zaf.toml")), HEAD, "the content is kept");

    let again = move_legacy_manifest(&root, None, false).expect("nothing to do");
    assert_eq!(again, LegacyMove::Absent);
    assert_eq!(again.line(), None);
    assert_eq!(read(&root.join(".zaf.toml")), HEAD);
}

#[test]
fn an_instance_moves_nothing_and_says_the_move_is_the_engineers() {
    let (_dir, root, _home) = places();
    fs::write(root.join("ZAF.toml"), HEAD).expect("legacy manifest");

    let outcome = move_legacy_manifest(&root, None, true).expect("leaves it");
    assert_eq!(outcome, LegacyMove::ForEngineer);
    let line = outcome.line().expect("it says so");
    assert!(line.contains("Engineer"), "{line}");
    assert!(root.join("ZAF.toml").is_file());
    assert!(!root.join(".zaf.toml").exists());
}

#[test]
fn the_legacy_manifest_beside_a_new_one_is_refused_naming_both_and_nothing_moves() {
    for new in [".zaf.toml", ".zaf/config.toml"] {
        let (_dir, root, _home) = places();
        fs::create_dir_all(root.join(".zaf")).expect("state dir");
        fs::write(root.join("ZAF.toml"), HEAD).expect("legacy manifest");
        fs::write(root.join(new), "# new\n").expect("new manifest");

        let error = move_legacy_manifest(&root, None, false).expect_err("refused");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{new}");
        let message = error.to_string();
        assert!(
            message.contains("ZAF.toml") && message.contains(new),
            "{message}"
        );
        assert_eq!(read(&root.join("ZAF.toml")), HEAD, "{new}");
        assert_eq!(read(&root.join(new)), "# new\n", "{new}: never overwritten");
    }
}

#[test]
fn a_zaf_home_config_beside_the_legacy_manifest_is_not_a_new_manifest() {
    let (_dir, root, _home) = places();
    let home = root.join(".zaf");
    fs::create_dir_all(&home).expect("home inside the project");
    fs::write(home.join("config.toml"), "# machine\n").expect("machine config");
    fs::write(root.join("ZAF.toml"), HEAD).expect("legacy manifest");

    let moved = move_legacy_manifest(&root, Some(&home), false).expect("moves");
    assert_eq!(moved, LegacyMove::Moved);
}

#[test]
fn ensure_homes_leaves_a_legacy_manifest_where_it_is() {
    let (_dir, root, home) = places();
    fs::write(root.join("ZAF.toml"), HEAD).expect("legacy manifest");
    ensure_homes(Some(&root), Some(&home)).expect("makes the homes");
    assert_eq!(read(&root.join("ZAF.toml")), HEAD);
    assert!(!root.join(".zaf.toml").exists());
}
