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
    super::setup_with(
        root,
        home,
        manifest,
        commands,
        &RecordingShell::new(),
        None,
        None,
    )
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
        None,
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
    assert!(report.is_ready(), "{:?}", report.fixes());
    assert_eq!(read(&root.join(".zaf/.gitignore")), ignore_text());

    fs::write(root.join(".zaf/.gitignore"), "# mine\n*\n").expect("a foreign file");
    let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up again");
    assert_eq!(report.homes.ignore, Some(IgnoreState::Foreign));
    assert_eq!(read(&root.join(".zaf/.gitignore")), "# mine\n*\n");
    let lines = format!("{:?}", report.fixes());
    assert!(lines.contains("merge these lines"), "{lines}");
    assert!(lines.contains("!actions/**"), "{lines}");
}

#[test]
fn a_root_gitignore_of_the_whole_folder_is_a_fix_but_a_star_is_not() {
    let (_dir, root, home) = places();
    let commands = FakeCommands::new();
    for ignoring in [".zaf/\n", "/.zaf\n", "target/\n.zaf\n"] {
        fs::write(root.join(".gitignore"), ignoring).expect("a root ignore file");
        let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up");
        let lines = format!("{:?}", report.fixes());
        assert!(lines.contains("root `.gitignore`"), "{ignoring:?} {lines}");
        assert!(!report.is_ready());
    }
    for fine in [".zaf/*\n", "target/\n", ".zaf/runs/\n"] {
        fs::write(root.join(".gitignore"), fine).expect("a root ignore file");
        let report = setup(&root, Some(&home), &manifest(""), &commands).expect("sets up");
        assert!(report.pending.is_empty(), "{fine:?}");
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
    assert_eq!(first.fixes(), second.fixes());
    assert_eq!(
        read(&root.join(".zaf/roles/keep.md")),
        "mine",
        "a file already there is never rewritten"
    );
}

#[test]
fn the_fixes_name_each_failing_tool() {
    let (_dir, root, home) = places();
    let manifest = manifest("[requires]\njj = \"*\"\ngh = \"*\"\nnodes = \">=1\"\nagy = \"*\"\n");
    let commands = FakeCommands::new()
        .with("nodes", "nodes 0.9.0")
        .with("agy", "agy dev");
    let report = setup(&root, Some(&home), &manifest, &commands).expect("sets up");
    assert!(!report.is_ready());
    let fixes = report.fixes();
    let lines = format!("{fixes:?}");
    assert_eq!(fixes.len(), 4, "{lines}");
    assert!(lines.contains("`gh` is not installed"), "{lines}");
    assert!(lines.contains("`jj` is not installed"), "{lines}");
    assert!(lines.contains("`nodes` is 0.9.0"), "{lines}");
    assert!(lines.contains("`agy --version`"), "{lines}");
    assert!(
        lines.contains("zaf opens pull requests through gh"),
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
fn a_failed_clone_becomes_a_fix() {
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
    let lines = format!("{:?}", report.fixes());
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

    let report = super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None)
        .expect("sets up");

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
    super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None).expect("first");
    let issued = shell.lines().len();
    assert!(issued > 0, "the first run installed the pack");
    let before = snapshot(&root.join(".zaf/profiles"));

    let second = super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None)
        .expect("again");

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
fn setup_pack_refusal_is_a_fix() {
    let (_dir, root, home) = places();
    let manifest = manifest(TAGGED);
    let commands = FakeCommands::new();
    let shell = RecordingShell::installing("2.0.3");

    let report = super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None)
        .expect("a refusal is a fix, not an error");

    assert!(report.profiles.is_none());
    assert!(!report.is_ready(), "setup exits 1");
    let fixes = report.fixes();
    assert!(
        fixes[0]
            .problem
            .as_str()
            .contains("pack teams is 2.0.3, manifest asks teams-v2.1.0"),
        "the fix names what it said: {fixes:?}"
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
    super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None).expect("first");
    let edited = home.join("actions/action-03");
    fs::write(&edited, "#!/bin/sh\n# the Engineer's own\n").expect("edit");
    fs::remove_file(home.join("actions/action-04")).expect("remove one");
    let before = snapshot(&home.join("actions"));

    let second = super::setup_with(&root, Some(&home), &manifest, &commands, &shell, None, None)
        .expect("again");

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
    assert_eq!(second.actions.differs, ["action-03"]);
    assert_eq!(
        second.actions.lines().len(),
        3,
        "one line each: installed, kept and differs"
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
        None,
    )
    .expect_err("a failed backend");
    assert!(matches!(error, SetupError::Profile(_)), "{error}");
}

/// Two action files, the installed and the pack's, with the same bytes and the mode `0o755`.
fn two_actions() -> (tempfile::TempDir, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temp dir");
    let installed = dir.path().join("installed");
    let pack = dir.path().join("pack");
    for path in [&installed, &pack] {
        fs::write(path, "#!/bin/sh\necho hi\n").expect("an action");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("a mode");
    }
    (dir, installed, pack)
}

#[test]
fn equal_actions_do_not_differ() {
    let (_dir, installed, pack) = two_actions();
    assert!(!super::differs(&installed, &pack).expect("compares"));
}

#[test]
fn changed_bytes_differ() {
    let (_dir, installed, pack) = two_actions();
    fs::write(&installed, "#!/bin/sh\necho edited\n").expect("an edit");
    assert!(super::differs(&installed, &pack).expect("compares"));
}

#[test]
fn changed_mode_differs() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, installed, pack) = two_actions();
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o750)).expect("a mode");
    assert!(super::differs(&installed, &pack).expect("compares"));
}

#[test]
fn difference_returns_the_git_diff() {
    let (_dir, installed, pack) = two_actions();
    let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    // `git diff --no-index` exits 1 when the files differ, so the fake fails.
    let commands = FakeCommands::new().failing("git", text);
    assert_eq!(
        super::difference(&commands, &installed, &pack).as_deref(),
        Some(text)
    );
    let call = &commands.calls()[0];
    assert_eq!(
        call[..6],
        [
            "git",
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-color",
            "--"
        ]
    );
    assert_eq!(call[6], installed.to_string_lossy());
    assert_eq!(call[7], pack.to_string_lossy());
}

#[test]
fn difference_is_none_without_git() {
    let (_dir, installed, pack) = two_actions();
    assert_eq!(
        super::difference(&FakeCommands::new(), &installed, &pack),
        None
    );
}

#[test]
fn difference_is_none_for_output_that_is_not_a_diff() {
    let (_dir, installed, pack) = two_actions();
    for text in [
        "",
        "fatal: not a git repository\n",
        "warning\ndiff --git a b\n",
    ] {
        let commands = FakeCommands::new().failing("git", text);
        assert_eq!(
            super::difference(&commands, &installed, &pack),
            None,
            "{text:?}"
        );
        let commands = FakeCommands::new().with("git", text);
        assert_eq!(
            super::difference(&commands, &installed, &pack),
            None,
            "{text:?}"
        );
    }
}

const DIFF: &str = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";

/// An asker that answers every question with `answer` and records each one.
struct Asker {
    answer: bool,
    asked: Vec<(String, String)>,
}

impl Asker {
    fn new(answer: bool) -> Self {
        Self {
            answer,
            asked: Vec::new(),
        }
    }
}

impl ReplaceAction for Asker {
    fn replace(&mut self, name: &str, _why: Option<&Text>, difference: &str) -> bool {
        self.asked.push((name.to_owned(), difference.to_owned()));
        self.answer
    }
}

/// Sets up once, so the pack's actions are installed, then edits `action-03` and returns what a
/// second setup needs: the places, the pack and its shell.
fn installed_then_edited() -> (tempfile::TempDir, PathBuf, PathBuf, RecordingShell) {
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
        None,
    )
    .expect("first");
    fs::write(home.join("actions/action-03"), "#!/bin/sh\n# own\n").expect("edit");
    (dir, root, home, shell)
}

fn setup_asking(
    root: &Path,
    home: &Path,
    shell: &RecordingShell,
    commands: &FakeCommands,
    asker: Option<&mut dyn ReplaceAction>,
) -> SetupReport {
    super::setup_with(
        root,
        Some(home),
        &manifest(TAGGED),
        commands,
        shell,
        None,
        asker,
    )
    .expect("again")
}

#[test]
fn setup_replaces_a_differing_action_on_yes() {
    let (dir, root, home, shell) = installed_then_edited();
    let pack = dir.path().join("pack/actions/action-03");
    let mut asker = Asker::new(true);

    let report = setup_asking(
        &root,
        &home,
        &shell,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
    );

    assert_eq!(asker.asked, [("action-03".to_owned(), DIFF.to_owned())]);
    let installed = home.join("actions/action-03");
    assert_eq!(read(&installed), read(&pack));
    assert_eq!(mode_of(&installed), mode_of(&pack));
    assert_eq!(report.actions.replaced.len(), 1);
    let (name, old) = &report.actions.replaced[0];
    assert_eq!(name, "action-03");
    assert_eq!(
        report.actions.lines()[0],
        format!("replaced action-03 (the old one is at {})", old.display())
    );
    assert!(report.actions.differs.is_empty());
    assert!(!report.actions.kept.contains(&"action-03".to_owned()));
    assert!(
        fs::read_dir(home.join("actions"))
            .expect("tier")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("zaf-installing")),
        "no temporary file is left"
    );
}

#[test]
fn setup_keeps_the_replaced_copy_under_actions_replaced() {
    let (_dir, root, home, shell) = installed_then_edited();
    let mut asker = Asker::new(true);

    let report = setup_asking(
        &root,
        &home,
        &shell,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
    );

    let (_, old) = &report.actions.replaced[0];
    assert_eq!(old.parent(), Some(home.join("actions-replaced").as_path()));
    let file = old.file_name().expect("a name").to_string_lossy();
    let time = file
        .strip_prefix("action-03.")
        .expect("name, a dot, the time");
    assert!(
        time.len() == 16 && time.ends_with('Z') && time.as_bytes()[8] == b'T',
        "UTC time as YYYYMMDDTHHMMSSZ: {time}"
    );
    assert_eq!(read(old), "#!/bin/sh\n# own\n");
    assert_eq!(mode_of(old), 0o755);
}

#[test]
fn a_failure_after_a_replacement_still_names_what_was_replaced() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, root, home, shell) = installed_then_edited();
    // action-07 differs by its mode, and a folder sits where its temporary file goes, so writing
    // it fails whoever runs the test (a mode of 0 would not stop root, as in CI).
    let installed = home.join("actions/action-07");
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o600)).expect("mode");
    fs::create_dir(home.join("actions/.action-07.zaf-installing")).expect("blocker");
    let mut asker = Asker::new(true);

    let error = super::setup_with(
        &root,
        Some(&home),
        &manifest(TAGGED),
        &FakeCommands::new().failing("git", DIFF),
        &shell,
        None,
        Some(&mut asker),
    )
    .expect_err("the second action fails");

    let super::SetupError::Actions(failed) = error else {
        panic!("not the actions' failure: {error}");
    };
    assert_eq!(failed.replaced.len(), 1);
    let (name, old) = &failed.replaced[0];
    assert_eq!(name, "action-03");
    assert_eq!(read(old), "#!/bin/sh\n# own\n");
    assert_eq!(
        failed.lines(),
        [format!(
            "replaced action-03 (the old one is at {})",
            old.display()
        )]
    );
}

#[test]
fn setup_keeps_a_differing_action_on_no() {
    let (_dir, root, home, shell) = installed_then_edited();
    let mut asker = Asker::new(false);

    let report = setup_asking(
        &root,
        &home,
        &shell,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
    );

    assert_eq!(asker.asked.len(), 1);
    assert_eq!(read(&home.join("actions/action-03")), "#!/bin/sh\n# own\n");
    assert!(
        !home.join("actions-replaced").exists(),
        "nothing is written there"
    );
    assert!(report.actions.replaced.is_empty());
    assert!(report.actions.kept.contains(&"action-03".to_owned()));
}

#[test]
fn setup_without_an_asker_lists_differing_actions() {
    let (_dir, root, home, shell) = installed_then_edited();
    fs::write(home.join("actions/action-07"), "#!/bin/sh\n# own too\n").expect("edit");

    let report = setup_asking(&root, &home, &shell, &FakeCommands::new(), None);

    assert_eq!(read(&home.join("actions/action-03")), "#!/bin/sh\n# own\n");
    assert!(!home.join("actions-replaced").exists());
    assert_eq!(report.actions.differs, ["action-03", "action-07"]);
    assert!(
        report.actions.lines().contains(
            &"differs from the pack, kept: action-03, action-07; run zaf setup in a terminal to review"
                .to_owned()
        ),
        "{:?}",
        report.actions.lines()
    );
}

#[test]
fn setup_never_offers_an_equal_action() {
    let (_dir, root, home, shell) = installed_then_edited();
    let mut asker = Asker::new(true);

    setup_asking(
        &root,
        &home,
        &shell,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
    );

    let names: Vec<&str> = asker.asked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["action-03"], "the other 16 are equal");
}

#[test]
fn setup_never_offers_a_linked_action() {
    let (dir, root, home, shell) = installed_then_edited();
    let own = dir.path().join("own-action");
    fs::write(&own, "#!/bin/sh\n# linked\n").expect("a file to link to");
    let linked = home.join("actions/action-05");
    fs::remove_file(&linked).expect("remove");
    std::os::unix::fs::symlink(&own, &linked).expect("link");
    let folder = home.join("actions/action-06");
    fs::remove_file(&folder).expect("remove");
    fs::create_dir(&folder).expect("a folder");
    let mut asker = Asker::new(true);

    let report = setup_asking(
        &root,
        &home,
        &shell,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
    );

    let names: Vec<&str> = asker.asked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["action-03"]);
    assert_eq!(fs::read_link(&linked).expect("still a link"), own);
    assert!(folder.is_dir());
    assert!(report.actions.kept.contains(&"action-05".to_owned()));
    assert!(report.actions.kept.contains(&"action-06".to_owned()));
}

#[test]
fn setup_keeps_an_action_whose_difference_cannot_be_shown() {
    let (_dir, root, home, shell) = installed_then_edited();
    let mut asker = Asker::new(true);

    // No `git`: the difference cannot be shown.
    let report = setup_asking(&root, &home, &shell, &FakeCommands::new(), Some(&mut asker));

    assert!(
        asker.asked.is_empty(),
        "no one is asked what cannot be shown"
    );
    assert_eq!(read(&home.join("actions/action-03")), "#!/bin/sh\n# own\n");
    assert!(!home.join("actions-replaced").exists());
    assert_eq!(report.actions.differs, ["action-03"]);
    assert_eq!(report.actions.unshown, ["action-03"]);
    assert!(
        report
            .actions
            .lines()
            .iter()
            .any(|line| line.contains("the difference could not be shown: action-03")),
        "{:?}",
        report.actions.lines()
    );
}

/// An asker that records the why it was given with each question.
struct WhyAsker {
    answer: bool,
    asked: Vec<(String, Option<Text>, String)>,
}

impl ReplaceAction for WhyAsker {
    fn replace(&mut self, name: &str, why: Option<&Text>, difference: &str) -> bool {
        self.asked
            .push((name.to_owned(), why.cloned(), difference.to_owned()));
        self.answer
    }
}

const NEW: &str = "#!/bin/sh\n# zaf description: new\n# zaf 1: failed\ntrue\n";
const EQUAL: &str = "#!/bin/sh\n# zaf description: equal\n# zaf 1: failed\ntrue\n";
const PACKS_CHANGED: &str =
    "#!/bin/sh\n# zaf description: moved\n# zaf 1: failed\n# changed: it waits now\ntrue\n";
const OLD_CHANGED: &str = "#!/bin/sh\n# zaf description: moved\n# zaf 1: failed\ntrue\n";

/// A pack folder of three actions (one new, one equal, one differing with a `# changed:` line)
/// and a zaf home that holds the last two as the machine had them.
fn machine_and_pack() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let pack = dir.path().join("pack-actions");
    let home = dir.path().join("zaf-home");
    fs::create_dir_all(&pack).expect("pack");
    fs::create_dir_all(home.join("actions")).expect("home");
    for (name, text) in [
        ("brand-new", NEW),
        ("equal", EQUAL),
        ("moved", PACKS_CHANGED),
    ] {
        fs::write(pack.join(name), text).expect("pack action");
    }
    fs::write(home.join("actions/equal"), EQUAL).expect("equal");
    fs::write(home.join("actions/moved"), OLD_CHANGED).expect("moved");
    (dir, pack, home)
}

#[test]
fn changed_actions_counts_without_changing() {
    let (_dir, pack, home) = machine_and_pack();

    let (names, total) = changed_actions(&home, &pack);

    assert_eq!(names, ["brand-new", "moved"]);
    assert_eq!(total, 3);
    assert!(!home.join("actions/brand-new").exists());
    assert_eq!(read(&home.join("actions/moved")), OLD_CHANGED);
    assert!(!home.join("actions-replaced").exists());
}

#[test]
fn machine_actions_offer_the_why_and_difference() {
    let (_dir, pack, home) = machine_and_pack();
    let mut asker = WhyAsker {
        answer: true,
        asked: Vec::new(),
    };

    let done = install_machine_actions(
        &home,
        &pack,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
        SystemTime::now(),
    )
    .expect("installed");

    assert_eq!(done.installed, ["brand-new"]);
    assert_eq!(done.kept, ["equal"]);
    assert_eq!(
        asker.asked,
        [(
            "moved".to_owned(),
            Some(Text::from("it waits now")),
            DIFF.to_owned()
        )]
    );
    let (name, old) = &done.replaced[0];
    assert_eq!(name, "moved");
    assert_eq!(old.parent(), Some(home.join("actions-replaced").as_path()));
    assert_eq!(read(old), OLD_CHANGED);
    assert_eq!(read(&home.join("actions/moved")), PACKS_CHANGED);
    assert_eq!(read(&home.join("actions/brand-new")), NEW);
}

#[test]
fn a_refused_replacement_keeps_the_file() {
    let (_dir, pack, home) = machine_and_pack();
    let mut asker = WhyAsker {
        answer: false,
        asked: Vec::new(),
    };

    let done = install_machine_actions(
        &home,
        &pack,
        &FakeCommands::new().failing("git", DIFF),
        Some(&mut asker),
        SystemTime::now(),
    )
    .expect("installed");

    assert_eq!(asker.asked.len(), 1);
    assert!(done.replaced.is_empty());
    assert_eq!(done.kept, ["equal", "moved"]);
    assert_eq!(read(&home.join("actions/moved")), OLD_CHANGED);
    assert!(!home.join("actions-replaced").exists());
}

#[test]
fn setup_install_still_reads_the_profiles_pack() {
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
        None,
    )
    .expect("setup");

    assert_eq!(report.actions.installed.len(), PACK_ACTIONS);
    assert!(home.join("actions/action-00").is_file());
}

fn verbs(rows: &[PlanRow]) -> Vec<&str> {
    rows.iter().map(|row| row.verb.as_str()).collect()
}

#[test]
fn an_empty_folder_plans_five_steps() {
    let (_dir, root, home) = places();
    let rows = plan(&root, Some(&home), false);
    let shown: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row.verb.as_str(), row.noun.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Ask", "4 questions"),
            ("Write", ".zaf.toml"),
            ("Make", "~/.zaf and .zaf/"),
            ("Install", "the standard actions"),
            ("Check", "your tools and access"),
        ]
    );
    assert!(rows.iter().all(|row| !row.why.as_str().is_empty()));
    assert!(writes(&rows));
}

#[test]
fn a_set_up_folder_plans_only_install_and_check() {
    let (_dir, root, home) = places();
    fs::write(root.join(".zaf.toml"), HEAD).expect("manifest");
    ensure_homes(Some(&root), Some(&home)).expect("homes");
    let rows = plan(&root, Some(&home), false);
    assert_eq!(verbs(&rows), ["Install", "Check"]);
    assert!(!writes(&rows));

    // Only what is missing is made: the home alone, then the project's folder alone.
    fs::remove_dir_all(&home).expect("home gone");
    let rows = plan(&root, Some(&home), false);
    assert_eq!(rows[0].noun.as_str(), "~/.zaf");
    ensure_homes(None, Some(&home)).expect("home back");
    fs::remove_dir_all(root.join(".zaf/logs")).expect("logs gone");
    let rows = plan(&root, Some(&home), false);
    assert_eq!(rows[0].noun.as_str(), ".zaf/");
    assert_eq!(verbs(&rows), ["Make", "Install", "Check"]);
}

#[test]
fn a_legacy_manifest_plans_its_move_outside_an_instance_only() {
    let (_dir, root, home) = places();
    fs::write(root.join("ZAF.toml"), HEAD).expect("legacy");
    ensure_homes(Some(&root), Some(&home)).expect("homes");
    let rows = plan(&root, Some(&home), false);
    assert_eq!(verbs(&rows), ["Move", "Install", "Check"]);
    assert_eq!(rows[0].noun.as_str(), "ZAF.toml to .zaf.toml");
    assert!(writes(&rows));
    assert_eq!(verbs(&plan(&root, Some(&home), true)), ["Install", "Check"]);
}

#[test]
fn an_old_star_gitignore_is_part_of_what_is_made() {
    let (_dir, root, home) = places();
    fs::write(root.join(".zaf.toml"), HEAD).expect("manifest");
    ensure_homes(Some(&root), Some(&home)).expect("homes");
    fs::write(root.join(".zaf/.gitignore"), "*\n").expect("old file");
    assert_eq!(verbs(&plan(&root, Some(&home), false))[0], "Make");
}
