use std::fs;
use std::sync::Mutex;

use super::*;
use crate::app::doctor::fake::FakeCommands;
use crate::manifest::CommandLine;
use crate::ports::{GateRun, GateVerdict, PortError};

/// A shell that records each `claude plugin` line and answers it; an install records the
/// plugin at `version` in the profile's `plugins/installed_plugins.json`, as Claude does.
struct RecordingShell {
    version: &'static str,
    install_path: Option<PathBuf>,
    lines: Mutex<Vec<String>>,
}

impl RecordingShell {
    fn new() -> Self {
        Self::installing("2.1.0")
    }

    fn installing(version: &'static str) -> Self {
        Self {
            version,
            install_path: None,
            lines: Mutex::new(Vec::new()),
        }
    }

    /// An install that records the pack at `path`, as Claude records its `installPath`.
    fn installing_at(path: &Path) -> Self {
        Self {
            install_path: Some(path.to_owned()),
            ..Self::new()
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lock").clone()
    }
}

impl Shell for RecordingShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.lines.lock().expect("lock").push(line.to_string());
        let text = line.as_ref();
        if text.contains(" claude plugin install ") {
            let mut quoted = text.split('\'');
            if let (Some(home), Some(id)) = (quoted.nth(1), quoted.nth(1)) {
                let home = Path::new(home);
                fs::create_dir_all(home.join("plugins")).expect("plugins dir");
                let mut entry = serde_json::json!({ "scope": "user", "version": self.version });
                if let Some(path) = &self.install_path {
                    entry["installPath"] = path.display().to_string().into();
                }
                let record = serde_json::json!({ "version": 2, "plugins": { id: [entry] } });
                fs::write(
                    home.join(profile::INSTALLED_PLUGINS_FILE),
                    record.to_string(),
                )
                .expect("records the install");
            }
        }
        let output = if text.ends_with("claude plugin marketplace list") {
            "No marketplaces configured\n"
        } else if text.ends_with("claude plugin list") {
            "No plugins installed\n"
        } else {
            ""
        };
        Ok(GateRun {
            verdict: GateVerdict::Passed,
            output: output.to_owned(),
        })
    }
}

/// Whether a doctor call is the login check of a generated profile.
fn is_login_check(call: &[String]) -> bool {
    call.len() == 5 && call[0] == "env" && call[2..] == ["claude", "auth", "status"]
}

/// [`super::setup`] with no Claude home and a fresh shell: nothing is linked.
fn setup(
    root: &Path,
    home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
) -> Result<SetupReport, SetupError> {
    super::setup_with(root, home, manifest, commands, &RecordingShell::new(), None)
}

/// [`super::setup_with`] through a fresh shell.
fn setup_with(
    root: &Path,
    home: Option<&Path>,
    manifest: &Manifest,
    commands: &dyn Commands,
    claude_home: Option<&Path>,
) -> Result<SetupReport, SetupError> {
    super::setup_with(
        root,
        home,
        manifest,
        commands,
        &RecordingShell::new(),
        claude_home,
    )
}

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

/// Writes the four `slice-*` actions every project needs into the project's tier, so doctor finds
/// nothing missing.
fn provide_slice_actions(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let dir = root.join(".zaf/actions");
    fs::create_dir_all(&dir).expect("actions dir");
    for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
        let file = dir.join(name);
        fs::write(
            &file,
            "#!/bin/sh\n# zaf description: a slice step\n# zaf 1: failed\ntrue\n",
        )
        .expect("action");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    }
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
    provide_slice_actions(&root);
    profile::write_stamps(&root.join(STATE_DIR), &manifest(""));
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
    provide_slice_actions(&root);
    let manifest = manifest("");
    profile::write_stamps(&root.join(STATE_DIR), &manifest);
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
            .all(|call| call[1..] == ["--version"] || is_login_check(call)),
        "setup only asks for versions and logins, it installs nothing: {:?}",
        commands.calls()
    );
}

#[test]
fn a_declared_sibling_design_repo_is_cloned_when_missing() {
    let (dir, root, home) = places();
    provide_slice_actions(&root);
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    profile::write_stamps(&root.join(STATE_DIR), &manifest);
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
    assert!(
        again.calls().iter().all(|call| call[0] != "gh"),
        "nothing to clone, nothing cloned: {:?}",
        again.calls()
    );
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
    assert!(
        commands.calls().iter().all(|call| is_login_check(call)),
        "no clone: {:?}",
        commands.calls()
    );
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
    assert_eq!(
        fs::read_link(
            root.join(".zaf/profiles/lean")
                .join(profile::CREDENTIALS_FILE)
        )
        .expect("a symlink"),
        source,
        "a profile setup generates is linked too"
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

const TAGGED: &str = "[pack]\ntag = \"teams-v2.1.0\"\n";

type Files = std::collections::BTreeMap<PathBuf, Vec<u8>>;

/// Every file under `dir`, by relative path, with its bytes; a symlink counts as its target path.
fn snapshot(dir: &Path) -> Files {
    fn walk(base: &Path, dir: &Path, into: &mut Files) {
        for entry in fs::read_dir(dir).expect("a folder") {
            let path = entry.expect("an entry").path();
            let relative = path.strip_prefix(base).expect("under base").to_owned();
            if fs::symlink_metadata(&path).expect("there").is_symlink() {
                let target = fs::read_link(&path).expect("a link");
                into.insert(relative, target.display().to_string().into_bytes());
            } else if path.is_dir() {
                walk(base, &path, into);
            } else {
                into.insert(relative, fs::read(&path).expect("a file"));
            }
        }
    }
    let mut files = Files::new();
    walk(dir, dir, &mut files);
    files
}

#[test]
fn setup_generates_profiles() {
    let (_dir, root, home) = places();
    let manifest = manifest(TAGGED);
    let commands = FakeCommands::new();
    let shell = RecordingShell::new();

    let report =
        super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None).expect("sets up");

    for name in ["build", "lean"] {
        let profile = root.join(".zaf/profiles").join(name);
        for file in ["settings.json", "CLAUDE.md", profile::STAMP_FILE] {
            assert!(profile.join(file).is_file(), "{name} has {file}");
        }
    }
    let written = report.profiles.expect("the generation ran");
    assert!(!written.unchanged());
    let mut generated = written.profiles.clone();
    generated.sort();
    assert_eq!(generated, ["build", "lean"]);
    assert!(
        shell
            .lines()
            .iter()
            .any(|line| line.contains(" install 'teams@ways'")),
        "the pack is installed: {:?}",
        shell.lines()
    );
}

#[test]
fn setup_rerun_issues_no_plugin_command() {
    let (_dir, root, home) = places();
    let manifest = manifest(TAGGED);
    let commands = FakeCommands::new();
    let shell = RecordingShell::new();
    super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None).expect("first");
    let issued = shell.lines().len();
    assert!(issued > 0, "the first run installed the pack");
    let before = snapshot(&root.join(".zaf/profiles"));

    let second =
        super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None).expect("again");

    assert_eq!(
        shell.lines().len(),
        issued,
        "no plugin command: {:?}",
        shell.lines()
    );
    assert_eq!(
        snapshot(&root.join(".zaf/profiles")),
        before,
        "no profile file changed"
    );
    assert!(second.profiles.expect("ran").unchanged());
}

#[test]
fn setup_pack_refusal_is_a_step() {
    let (_dir, root, home) = places();
    let manifest = manifest(TAGGED);
    let commands = FakeCommands::new();
    let shell = RecordingShell::installing("2.0.3");

    let report = super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None)
        .expect("a refusal is a step, not an error");

    assert!(report.profiles.is_none());
    assert!(!report.is_ready(), "setup exits 1");
    let lines = report.worksheet_lines();
    assert!(lines[0].starts_with("1. "), "{lines:?}");
    assert!(
        lines[0].contains("pack teams is 2.0.3, manifest asks teams-v2.1.0"),
        "the step names what it said: {lines:?}"
    );
}

const PACK_ACTIONS: usize = 17;

/// A pack folder with `PACK_ACTIONS` action files (`action-00` and so on, the first `750`), a
/// link and a folder that must not be copied.
fn pack_with_actions(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let pack = dir.join("pack");
    let actions = pack.join("actions");
    fs::create_dir_all(actions.join("hooks")).expect("pack folders");
    for number in 0..PACK_ACTIONS {
        let file = actions.join(format!("action-{number:02}"));
        fs::write(
            &file,
            format!("#!/bin/sh\n# zaf description: pack action {number}\n# zaf 1: failed\ntrue\n"),
        )
        .expect("action");
        let mode = if number == 0 { 0o750 } else { 0o755 };
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).expect("mode");
    }
    std::os::unix::fs::symlink("/etc/passwd", actions.join("linked")).expect("link");
    fs::write(actions.join("hooks/pre-tick"), "#!/bin/sh\n").expect("hook");
    pack
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).expect("a file").permissions().mode() & 0o777
}

#[test]
fn setup_installs_pack_actions() {
    let (dir, root, home) = places();
    let pack = pack_with_actions(dir.path());
    let shell = RecordingShell::installing_at(&pack);

    let report = super::setup_with(
        &root,
        Some(&home),
        &manifest(TAGGED),
        &FakeCommands::new(),
        &shell,
        None,
    )
    .expect("sets up");

    let installed = home.join("actions");
    let mut names: Vec<String> = fs::read_dir(&installed)
        .expect("the machine tier")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    names.sort();
    let expected: Vec<String> = (0..PACK_ACTIONS)
        .map(|n| format!("action-{n:02}"))
        .collect();
    assert_eq!(
        names, expected,
        "the 17 and nothing else, no temporary file"
    );
    assert_eq!(report.actions.installed, expected);
    assert!(report.actions.kept.is_empty());
    for name in &expected {
        let want = if name == "action-00" { 0o750 } else { 0o755 };
        assert_eq!(
            mode_of(&installed.join(name)),
            want,
            "{name} keeps its mode"
        );
        assert_eq!(
            read(&installed.join(name)),
            read(&pack.join("actions").join(name))
        );
    }
    let lines = report.actions.lines();
    assert_eq!(lines.len(), 1, "one line: {lines:?}");
    assert!(
        lines[0].starts_with("installed 17 pack actions"),
        "{lines:?}"
    );
}

#[test]
fn setup_keeps_edited_machine_action() {
    let (dir, root, home) = places();
    let pack = pack_with_actions(dir.path());
    let shell = RecordingShell::installing_at(&pack);
    let manifest = manifest(TAGGED);
    let commands = FakeCommands::new();
    super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None).expect("first");
    let edited = home.join("actions/action-03");
    fs::write(&edited, "#!/bin/sh\n# the Engineer's own\n").expect("edit");
    fs::remove_file(home.join("actions/action-04")).expect("remove one");
    let before = snapshot(&home.join("actions"));

    let second =
        super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None).expect("again");

    assert_eq!(read(&edited), "#!/bin/sh\n# the Engineer's own\n");
    let after = snapshot(&home.join("actions"));
    assert_eq!(
        after.len(),
        PACK_ACTIONS,
        "the removed one is back, nothing else is"
    );
    for (name, bytes) in &before {
        assert_eq!(&after[name], bytes, "{name:?} is as it was");
    }
    assert_eq!(second.actions.installed, ["action-04"]);
    assert_eq!(second.actions.kept.len(), PACK_ACTIONS - 1);
    assert!(second.actions.kept.contains(&"action-03".to_owned()));
    assert_eq!(
        second.actions.lines().len(),
        2,
        "one line each, installed and kept"
    );
}

#[test]
fn setup_installs_nothing_without_a_pack_or_its_actions() {
    let (dir, root, home) = places();
    let commands = FakeCommands::new();
    let none = super::setup_with(
        &root,
        Some(&home),
        &manifest(""),
        &commands,
        &RecordingShell::new(),
        None,
    )
    .expect("no pack");
    assert_eq!(none.actions, PackActions::default());
    assert!(none.actions.lines().is_empty());

    let bare = dir.path().join("bare-pack");
    fs::create_dir(&bare).expect("a pack with no actions");
    let shell = RecordingShell::installing_at(&bare);
    let report = super::setup_with(
        &root,
        Some(&home),
        &manifest(TAGGED),
        &commands,
        &shell,
        None,
    )
    .expect("no actions folder");
    assert_eq!(report.actions, PackActions::default());
    assert!(
        fs::read_dir(home.join("actions"))
            .expect("tier")
            .next()
            .is_none()
    );
}

#[test]
fn project_action_shadows_installed() {
    let (dir, root, home) = places();
    let pack = pack_with_actions(dir.path());
    let shell = RecordingShell::installing_at(&pack);
    super::setup_with(
        &root,
        Some(&home),
        &manifest(TAGGED),
        &FakeCommands::new(),
        &shell,
        None,
    )
    .expect("sets up");

    let found = crate::app::actions::resolve(&root, &home, "action-05")
        .expect("resolves")
        .expect("found");
    assert_eq!(found.tier, crate::app::actions::Tier::Machine);
    assert_eq!(found.path, home.join("actions/action-05"));

    fs::create_dir_all(root.join(".zaf/actions")).expect("project actions");
    fs::write(root.join(".zaf/actions/action-05"), "#!/bin/sh\n").expect("own action");
    let found = crate::app::actions::resolve(&root, &home, "action-05")
        .expect("resolves")
        .expect("found");
    assert_eq!(found.tier, crate::app::actions::Tier::Project);
    assert_eq!(found.shadows, Some(crate::app::actions::Tier::Machine));
}

/// A shell whose every command fails.
struct Failing;

impl Shell for Failing {
    fn run(&self, _line: &CommandLine) -> Result<GateRun, PortError> {
        Ok(GateRun {
            verdict: GateVerdict::Failed,
            output: "no network".to_owned(),
        })
    }
}

#[test]
fn setup_fails_when_a_plugin_command_fails() {
    let (_dir, root, home) = places();
    let error = super::setup_with(
        &root,
        Some(&home),
        &manifest(TAGGED),
        &FakeCommands::new(),
        &Failing,
        None,
    )
    .expect_err("a failed backend");
    assert!(matches!(error, SetupError::Profile(_)), "{error}");
}
