use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::fake::FakeCommands;
use super::*;

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{HEAD}{extra}").parse().expect("a valid manifest")
}

/// [`super::doctor`] with no zaf home, so no test reads the machine's own actions, over a project
/// that provides the four `slice-*` names every project needs.
fn doctor(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Report {
    provide_slice_actions(root);
    super::doctor(root, None, manifest, commands)
}

/// Writes the four `slice-*` actions into the project's tier.
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

fn status_of(report: &Report, tool: &str) -> ToolStatus {
    report
        .tools
        .iter()
        .find(|check| check.tool == tool)
        .unwrap_or_else(|| panic!("{tool} is checked"))
        .status
        .clone()
}

#[test]
fn a_tool_that_satisfies_its_requirement_is_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \">=0.20\"\n");
    let commands = FakeCommands::new().with("jj", "jj 0.23.0-a1b2c3\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert_eq!(
        status_of(&report, "jj"),
        ToolStatus::Ok {
            found: "0.23.0".into()
        }
    );
    assert!(report.passed());
    assert_eq!(commands.calls(), [["jj", "--version"]]);
}

#[test]
fn a_tool_that_cannot_start_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \"*\"\n");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert_eq!(status_of(&report, "jj"), ToolStatus::Missing);
    assert!(!report.passed());
}

#[test]
fn an_old_tool_says_found_and_wanted() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\ngh = \">=2.40\"\n");
    let commands = FakeCommands::new().with("gh", "gh version 2.31.0 (2024-01-01)\n");
    let report = doctor(dir.path(), &manifest, &commands);
    let expected = ToolStatus::TooOld {
        found: "2.31.0".into(),
        wanted: ">=2.40".into(),
    };
    assert_eq!(status_of(&report, "gh"), expected);
    assert_eq!(
        report.lines()[0],
        "gh: too old 2.31.0 < >=2.40",
        "{:?}",
        report.lines()
    );
    assert!(!report.passed());
}

#[test]
fn a_star_requirement_takes_any_version() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nnodes = \"*\"\n");
    let commands = FakeCommands::new().with("nodes", "nodes v0.0.1\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert!(status_of(&report, "nodes").is_ok());
    assert_eq!(report.lines()[0], "nodes: ok");
}

#[test]
fn a_version_line_without_semver_is_unknown_and_failing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nagy = \"*\"\n");
    let commands = FakeCommands::new().with("agy", "agy nightly build\n");
    let report = doctor(dir.path(), &manifest, &commands);
    assert_eq!(status_of(&report, "agy"), ToolStatus::Unknown);
    assert!(!report.passed());
}

#[test]
fn a_pinned_release_tag_must_match_exactly() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\nteams = \"teams-v2.0.0\"\n");
    let same = FakeCommands::new().with("teams", "teams 2.0.0");
    assert!(doctor(dir.path(), &manifest, &same).passed());
    let other = FakeCommands::new().with("teams", "teams 2.0.1");
    assert!(!doctor(dir.path(), &manifest, &other).passed());
}

#[test]
fn the_first_semver_skips_words_and_dates() {
    let found = |text: &str| first_semver(text).map(|version| version.to_string());
    assert_eq!(found("jj 0.23.0"), Some("0.23.0".into()));
    assert_eq!(found("v1.2.3"), Some("1.2.3".into()));
    assert_eq!(
        found("gh version 2.40.1 (2024-01-01)"),
        Some("2.40.1".into())
    );
    assert_eq!(found("tool 2024-01-01 then 3.4.5."), Some("3.4.5".into()));
    assert_eq!(found("only 1.2 here"), None);
    assert_eq!(found(""), None);
}

#[test]
fn a_declared_design_dir_must_exist() {
    let parent = tempfile::tempdir().expect("temp dir");
    let root = parent.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let manifest = manifest(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    let before = doctor(&root, &manifest, &FakeCommands::new());
    assert_eq!(before.design.as_ref().map(|check| check.ok), Some(false));
    assert!(!before.passed());
    assert!(before.lines()[0].starts_with("design dir: missing"));
    fs::create_dir(parent.path().join("b-design")).expect("design dir");
    fs::write(parent.path().join("b-design/index.md"), "").expect("index");
    assert!(doctor(&root, &manifest, &FakeCommands::new()).passed());
}

#[test]
fn a_missing_design_index_fails_and_names_the_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(dir.path().join("docs")).expect("design dir");
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert!(!report.passed());
    let missing = format!(
        "design index: missing ({})",
        dir.path().join("docs/index.md").display()
    );
    assert!(report.lines().contains(&missing), "{:?}", report.lines());
    assert!(report.lines()[0].starts_with("design dir: ok"));
}

#[test]
fn a_present_design_index_is_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(dir.path().join("docs")).expect("design dir");
    fs::write(dir.path().join("docs/index.md"), "# Pages\n").expect("index");
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert!(report.passed());
    assert!(
        report.lines()[1].starts_with("design index: ok"),
        "{:?}",
        report.lines()
    );
}

#[test]
fn a_declared_design_index_is_resolved_against_the_design_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join("docs/meta")).expect("design dir");
    let manifest =
        manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\nindex = \"meta/pages.md\"\n");
    let before = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert!(!before.passed());
    assert_eq!(
        before.design_index.as_ref().map(|check| check.path.clone()),
        Some(dir.path().join("docs/meta/pages.md"))
    );
    fs::write(dir.path().join("docs/meta/pages.md"), "").expect("index");
    assert!(doctor(dir.path(), &manifest, &FakeCommands::new()).passed());
}

#[test]
fn a_design_index_that_is_a_folder_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join("docs/index.md")).expect("a folder");
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    assert!(!doctor(dir.path(), &manifest, &FakeCommands::new()).passed());
}

#[test]
fn the_default_index_name_is_the_design_adapters() {
    assert_eq!(
        DEFAULT_DESIGN_INDEX,
        crate::adapters::design::DEFAULT_INDEX_FILE
    );
}

#[test]
fn no_design_kind_checks_no_design_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert_eq!(report.design, None);
    assert_eq!(report.design_index, None);
    assert!(report.passed());
}

#[test]
fn the_workspaces_dir_must_be_creatable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert!(report.workspaces.ok, "a missing dir under a writable root");
    fs::write(dir.path().join("blocker"), "a file").expect("file");
    let blocked = self::manifest("[workspaces]\npath = \"blocker/ws\"\n");
    let report = doctor(dir.path(), &blocked, &FakeCommands::new());
    assert!(!report.workspaces.ok, "a file in the way");
    assert!(
        report
            .lines()
            .last()
            .expect("a line")
            .contains("not creatable")
    );
}

#[test]
fn the_report_serializes_as_a_table() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("[requires]\njj = \"*\"\ngh = \"*\"\n");
    let commands = FakeCommands::new().with("jj", "jj 0.23.0");
    let report = doctor(dir.path(), &manifest, &commands);
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["tools"][0]["tool"], "gh");
    assert_eq!(json["tools"][0]["status"], "missing");
    assert_eq!(json["tools"][1]["status"], "ok");
    assert_eq!(json["tools"][1]["found"], "0.23.0");
    assert_eq!(json["workspaces"]["ok"], true);
}

#[test]
fn a_sibling_design_dir_follows_the_project_root_rule() {
    let root = Path::new("/work/project");
    let design = |text: &str| manifest(text).design;
    let beside = design(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\npath = \"../b-design\"\n",
    );
    assert_eq!(
        design_dir(root, &beside),
        Some(PathBuf::from("/work/project/../b-design"))
    );
    let defaulted = design("[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\n");
    assert_eq!(
        design_dir(root, &defaulted),
        Some(PathBuf::from("/work/b-design"))
    );
    let inside = design("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    assert_eq!(
        design_dir(root, &inside),
        Some(PathBuf::from("/work/project/docs"))
    );
    assert_eq!(design_dir(root, &design("")), None);
}

fn profile_homes(root: &Path) -> (PathBuf, PathBuf) {
    let profiles = root.join(".zaf/profiles");
    let (lean, build) = (profiles.join("lean"), profiles.join("build"));
    fs::create_dir_all(&lean).expect("lean");
    fs::create_dir_all(&build).expect("build");
    (lean, build)
}

#[test]
fn a_profile_whose_auth_status_succeeds_has_its_login() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (lean, build) = profile_homes(dir.path());
    let commands = FakeCommands::new().with("env", "{\"loggedIn\": true}\n");
    let report = doctor(dir.path(), &manifest(""), &commands);
    let lines = report.lines();
    assert!(
        lines.contains(&"profile lean: login ok".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"profile build: login ok".to_owned()),
        "{lines:?}"
    );
    assert_eq!(
        commands.calls(),
        [lean, build].map(|home| {
            vec![
                "env".to_owned(),
                format!("CLAUDE_CONFIG_DIR={}", home.display()),
                "claude".to_owned(),
                "auth".to_owned(),
                "status".to_owned(),
            ]
        })
    );
    assert!(report.passed());
}

#[test]
fn a_profile_without_a_login_is_missing_with_its_fix_and_the_doctor_still_passes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    let commands = FakeCommands::new().failing("env", "{\"loggedIn\": false}\n");
    let report = doctor(dir.path(), &manifest(""), &commands);
    let line = report
        .lines()
        .into_iter()
        .find(|line| line.starts_with("profile build:"))
        .expect("a build line");
    assert!(
        line.starts_with("profile build: login missing (fix: "),
        "{line}"
    );
    assert!(line.contains(&build.display().to_string()), "{line}");
    assert!(line.contains("[auth] token_env"), "{line}");
    assert!(report.passed(), "a missing login is reported, not failed");
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["logins"][1]["profile"], "build");
    assert_eq!(json["logins"][1]["ok"], false);
}

#[test]
fn a_profile_not_generated_yet_is_missing_without_asking() {
    let dir = tempfile::tempdir().expect("temp dir");
    let commands = FakeCommands::new().with("env", "ok");
    let report = doctor(dir.path(), &manifest(""), &commands);
    assert_eq!(report.logins.len(), 2);
    assert!(report.logins.iter().all(|check| !check.ok));
    assert!(commands.calls().is_empty());
}

/// The moment the credentials tests run at, two seconds after the epoch.
const NOW: Duration = Duration::from_secs(2);

/// A credentials file with its secrets and its expiry, in milliseconds since the epoch.
fn credentials_text(expires_at: u64) -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"sk-access-secret","refreshToken":"sk-refresh-secret","expiresAt":{expires_at}}}}}"#
    )
}

/// A Claude home in `root`, logged in.
fn a_claude_home(root: &Path) -> PathBuf {
    let claude_home = root.join("claude-home");
    fs::create_dir(&claude_home).expect("claude home");
    fs::write(claude_home.join(CREDENTIALS_FILE), credentials_text(3_000)).expect("credentials");
    claude_home
}

/// The build profile's doctor line, with `claude auth status` answered by `commands`.
fn build_line(root: &Path, commands: &FakeCommands, claude_home: Option<&Path>) -> String {
    let now = UNIX_EPOCH + NOW;
    doctor_with(root, None, &manifest(""), commands, claude_home, now)
        .lines()
        .into_iter()
        .find(|line| line.starts_with("profile build:"))
        .expect("a build line")
}

fn logged_in() -> FakeCommands {
    FakeCommands::new().with("env", "{\"loggedIn\": true}\n")
}

fn logged_out() -> FakeCommands {
    FakeCommands::new().failing("env", "{\"loggedIn\": false}\n")
}

#[test]
fn doctor_reports_a_stale_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    let copy = build.join(CREDENTIALS_FILE);
    fs::write(&copy, credentials_text(1_000)).expect("a copy");

    let line = build_line(dir.path(), &logged_in(), None);
    assert!(
        line.starts_with("profile build: login ok (copy, stale; fix: remove "),
        "{line}"
    );
    assert!(line.contains(&copy.display().to_string()), "{line}");
    assert!(line.contains("zaf profile init"), "{line}");

    let line = build_line(dir.path(), &logged_out(), None);
    assert!(
        line.starts_with("profile build: login missing (copy, stale; fix: remove "),
        "a failed login still says why: {line}"
    );
    assert!(
        !line.contains("secret"),
        "the token is never printed: {line}"
    );

    provide_slice_actions(dir.path());
    let report = doctor_with(
        dir.path(),
        None,
        &manifest(""),
        &logged_out(),
        None,
        UNIX_EPOCH + NOW,
    );
    assert!(report.passed(), "a stale copy is reported, not failed");
    let json = serde_json::to_string(&report).expect("serializes");
    assert!(json.contains(r#""credentials":"stale""#), "{json}");
    assert!(!json.contains("secret"), "{json}");
    assert_eq!(
        fs::read_to_string(&copy).expect("reads"),
        credentials_text(1_000),
        "doctor leaves the copy alone"
    );
}

#[test]
fn a_copy_not_past_its_expiry_is_only_a_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    fs::write(build.join(CREDENTIALS_FILE), credentials_text(3_000)).expect("a copy");
    assert_eq!(
        build_line(dir.path(), &logged_in(), None),
        "profile build: login ok (copy)"
    );
}

#[test]
fn a_copy_whose_expiry_cannot_be_read_is_only_a_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    for text in [
        "not json",
        "{}",
        r#"{"claudeAiOauth":{}}"#,
        r#"{"claudeAiOauth":{"expiresAt":"soon"}}"#,
    ] {
        fs::write(build.join(CREDENTIALS_FILE), text).expect("a copy");
        assert_eq!(
            build_line(dir.path(), &logged_in(), None),
            "profile build: login ok (copy)",
            "{text}"
        );
    }
}

#[test]
fn an_access_token_is_stale_only_past_its_expiry() {
    let now = UNIX_EPOCH + NOW;
    assert!(is_past(1_999, now));
    assert!(!is_past(2_000, now));
    assert!(!is_past(u64::MAX, now));
}

#[test]
fn doctor_reports_a_shared_login() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    let claude_home = a_claude_home(dir.path());
    symlink(
        claude_home.join(CREDENTIALS_FILE),
        build.join(CREDENTIALS_FILE),
    )
    .expect("the link");
    assert_eq!(
        build_line(dir.path(), &logged_in(), Some(&claude_home)),
        "profile build: login ok (shared)"
    );
    assert_eq!(
        build_line(dir.path(), &logged_out(), Some(&claude_home)),
        "profile build: login missing (shared; fix: log in to your Claude home)"
    );
}

#[test]
fn a_link_outside_the_claude_home_is_never_called_shared() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    let claude_home = a_claude_home(dir.path());
    let elsewhere = dir.path().join("elsewhere.json");
    fs::write(&elsewhere, credentials_text(3_000)).expect("another file");
    symlink(&elsewhere, build.join(CREDENTIALS_FILE)).expect("a foreign link");
    for home in [Some(claude_home.as_path()), None] {
        let line = build_line(dir.path(), &logged_in(), home);
        assert!(
            line.starts_with("profile build: login ok (links outside the Claude home; fix: "),
            "{line}"
        );
        assert!(!line.contains("shared"), "{line}");
    }
}

#[test]
fn the_token_env_path_has_no_credentials_file_and_keeps_its_plain_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    profile_homes(dir.path());
    let claude_home = a_claude_home(dir.path());
    assert_eq!(
        build_line(dir.path(), &logged_in(), Some(&claude_home)),
        "profile build: login ok"
    );
    let line = build_line(dir.path(), &logged_out(), Some(&claude_home));
    assert!(
        line.contains("set [auth] token_env = \"<VAR>\" in .zaf/local.toml and export that token"),
        "{line}"
    );
}

const PACK: &str = "[pack]\npath = \"vendor/kit\"\n";

/// The project's marketplace file: marketplace `shelf` listing plugin `kit` at `./vendor/kit`.
fn write_marketplace(root: &Path) {
    fs::create_dir_all(root.join(".claude-plugin")).expect("marketplace dir");
    fs::write(
        root.join(".claude-plugin/marketplace.json"),
        r#"{"name":"shelf","plugins":[{"name":"kit","source":"./vendor/kit"}]}"#,
    )
    .expect("writes");
}

/// The source pack's own manifest, declaring `version` (or none).
fn write_source_pack(root: &Path, version: Option<&str>) {
    let dir = root.join("vendor/kit/.claude-plugin");
    fs::create_dir_all(&dir).expect("plugin dir");
    let plugin = match version {
        Some(version) => format!(r#"{{"name":"kit","version":"{version}"}}"#),
        None => r#"{"name":"kit"}"#.to_owned(),
    };
    fs::write(dir.join("plugin.json"), plugin).expect("writes");
}

/// A profile's `plugins/installed_plugins.json` recording `entry` for `kit@shelf`.
fn write_installed(home: &Path, entry: &str) {
    let dir = home.join("plugins");
    fs::create_dir_all(&dir).expect("plugins dir");
    fs::write(
        dir.join("installed_plugins.json"),
        format!(r#"{{"version":2,"plugins":{{"kit@shelf":[{entry}]}}}}"#),
    )
    .expect("writes");
}

#[test]
fn doctor_prints_the_installed_pack_version() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    write_marketplace(dir.path());
    write_source_pack(dir.path(), Some("1.2.3"));
    write_installed(&build, r#"{"scope":"user","version":"1.2.3"}"#);
    let commands = FakeCommands::new();
    let report = doctor(dir.path(), &manifest(PACK), &commands);
    let lines = report.lines();
    assert!(
        lines.contains(&"profile build: pack kit@shelf 1.2.3 ok".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.starts_with("profile lean: pack")),
        "lean runs plugin-free: {lines:?}"
    );
    assert!(
        commands
            .calls()
            .iter()
            .all(|call| !call.contains(&"plugin".to_owned())),
        "no `claude plugin` command is run"
    );
    assert!(report.passed());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["packs"][0]["profile"], "build");
    assert_eq!(json["packs"][0]["version"], "1.2.3");
    assert_eq!(json["packs"][0]["source_version"], "1.2.3");
    assert_eq!(json["packs"][0]["ok"], true);
}

#[test]
fn an_unversioned_source_pack_has_nothing_to_compare() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    write_marketplace(dir.path());
    write_source_pack(dir.path(), None);
    write_installed(&build, r#"{"scope":"user","version":"0.9.0"}"#);
    let report = doctor(dir.path(), &manifest(PACK), &FakeCommands::new());
    assert!(
        report
            .lines()
            .contains(&"profile build: pack kit@shelf 0.9.0 ok".to_owned())
    );
    assert!(report.passed());
}

#[test]
fn doctor_fails_on_a_stale_pack() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    write_marketplace(dir.path());
    write_source_pack(dir.path(), Some("1.3.0"));
    write_installed(&build, r#"{"scope":"user","version":"1.2.3"}"#);
    let report = doctor(dir.path(), &manifest(PACK), &FakeCommands::new());
    let lines = report.lines();
    let line = lines
        .iter()
        .find(|line| line.starts_with("profile build: pack"))
        .expect("a pack line");
    assert!(line.contains("1.2.3") && line.contains("1.3.0"), "{line}");
    assert!(line.contains("zaf profile init updates it"), "{line}");
    assert!(!line.ends_with(" ok"), "{line}");
    assert!(!report.passed());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["packs"][0]["ok"], false);
}

#[test]
fn a_profile_whose_installed_file_lacks_the_pack_fails_with_its_fix() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, build) = profile_homes(dir.path());
    write_marketplace(dir.path());
    write_source_pack(dir.path(), Some("1.2.3"));
    fs::create_dir_all(build.join("plugins")).expect("plugins dir");
    fs::write(
        build.join("plugins/installed_plugins.json"),
        r#"{"version":2,"plugins":{"other@shelf":[{"version":"1.2.3"}]}}"#,
    )
    .expect("writes");
    let commands = FakeCommands::new();
    let report = doctor(dir.path(), &manifest(PACK), &commands);
    let lines = report.lines();
    assert!(
        lines.contains(
            &"profile build: pack missing (fix: zaf profile init installs it)".to_owned()
        ),
        "{lines:?}"
    );
    assert!(!report.passed(), "a missing pack fails the report");
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["packs"][0]["ok"], false);
}

#[test]
fn a_profile_not_generated_yet_has_its_pack_missing_without_asking() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_marketplace(dir.path());
    let commands = FakeCommands::new();
    let report = doctor(dir.path(), &manifest(PACK), &commands);
    assert!(
        report.lines().contains(
            &"profile build: pack missing (fix: zaf profile init installs it)".to_owned()
        )
    );
    assert!(!report.passed(), "a missing pack fails the report");
    assert!(commands.calls().is_empty());
}

#[test]
fn a_project_without_a_pack_has_no_pack_lines() {
    let dir = tempfile::tempdir().expect("temp dir");
    profile_homes(dir.path());
    let commands = FakeCommands::new();
    let report = doctor(dir.path(), &manifest(""), &commands);
    assert!(report.packs.is_empty());
    assert!(report.lines().iter().all(|line| !line.contains("pack")));
}
