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
///
/// Every generated profile is stamped current first (which makes its home), so a test gets no
/// stale-profile failure it did not ask for; see [`doctor_unstamped`] for the profiles not made.
fn doctor(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Report {
    profile::write_stamps(&root.join(STATE_DIR), manifest);
    doctor_unstamped(root, manifest, commands)
}

/// [`doctor`] over profiles nobody generated: no homes, so none is named stale (a profile never
/// generated is the pack check's and the worksheet's to say).
///
/// A home that exists is given the settings `zaf profile init` writes, and its `claude mcp get zaf`
/// shows zaf's entry, so the profile's MCP row is `ok` unless a test used [`raw`].
fn doctor_unstamped(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Report {
    provide_slice_actions(root);
    write_good_settings(root, manifest);
    super::doctor(root, None, manifest, &WithProfileMcp(commands))
}

/// Gives every generated profile whose home exists the settings `zaf profile init` writes.
fn write_good_settings(root: &Path, manifest: &Manifest) {
    for name in profile::generated(manifest) {
        let home = profile::home(&root.join(STATE_DIR), name);
        if home.is_dir() {
            fs::write(home.join("settings.json"), good_settings().to_string()).expect("settings");
        }
    }
}

/// The settings of a profile with every MCP part in place, for a project with no `.mcp.json`.
fn good_settings() -> serde_json::Value {
    serde_json::json!({
        "permissions": {
            "allow": ["mcp__zaf__*", "Bash(jj *)"],
            "deny": ["Bash(zaf)", "Bash(zaf *)"],
        },
        "disableClaudeAiConnectors": true,
        "disabledMcpjsonServers": [],
    })
}

/// A [`Commands`] that answers a profile's `claude mcp get zaf` with zaf's entry and hands every
/// other call to `inner`.
struct WithProfileMcp<'a>(&'a dyn Commands);

impl Commands for WithProfileMcp<'_> {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        if program == "env" && args.ends_with(&["claude", "mcp", "get", "zaf"]) {
            return Ok(CommandOutput {
                text: profile::GET_SHOWS_OURS.to_owned(),
                success: true,
            });
        }
        self.0.run(program, args)
    }
}

/// [`super::doctor`] with the profiles stamped and nothing else done for them: no settings, and
/// the commands as given, so a test sets the profile's MCP row itself.
fn raw(root: &Path, manifest: &Manifest, commands: &dyn Commands) -> Report {
    profile::write_stamps(&root.join(STATE_DIR), manifest);
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

/// The calls the profiles' checks made: every call but the Engineer's own `claude mcp get zaf`.
fn profile_calls(commands: &FakeCommands) -> Vec<Vec<String>> {
    commands
        .calls()
        .into_iter()
        .filter(|call| call != &["claude", "mcp", "get", "zaf"])
        .collect()
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
fn doctor_prints_one_line_for_stale_profiles() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(STATE_DIR);
    let roles = "[runners.claude]\ncommand = \"true\"\n\n\
        [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
        [roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\nprofile = \"lean\"\nturns = 5\nbatch = 2\n";
    let before = manifest(&format!("{roles}[forbidden]\ncommands = [\"git push\"]\n"));
    let after = manifest(&format!(
        "{roles}[forbidden]\ncommands = [\"gh pr merge\"]\n"
    ));
    profile::write_stamps(&state, &before);
    provide_slice_actions(dir.path());

    let report = super::doctor(dir.path(), None, &after, &logged_in());
    let lines = report.lines();
    let stale: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("older than the manifest"))
        .collect();
    assert_eq!(
        stale,
        ["profiles lean and build are older than the manifest (forbidden changed)"],
        "{lines:?}"
    );
}

#[test]
fn doctor_names_a_stale_profile() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(STATE_DIR);
    profile::write_stamps(
        &state,
        &manifest("[forbidden]\ncommands = [\"git push\"]\n"),
    );
    let edited = manifest("[forbidden]\ncommands = [\"gh pr merge\"]\n");
    provide_slice_actions(dir.path());
    let report = super::doctor(dir.path(), None, &edited, &logged_in());
    assert!(!report.passed());
    let lines = report.lines();
    assert!(
        lines.contains(
            &"profiles lean and build are older than the manifest (forbidden changed)".to_owned()
        ),
        "{lines:?}"
    );

    fs::remove_file(state.join("profiles/build").join(profile::STAMP_FILE)).expect("remove");
    let report = super::doctor(dir.path(), None, &edited, &logged_in());
    assert!(!report.passed());
    assert!(
        report.lines().contains(
            &"profile lean is older than the manifest (forbidden changed); \
                  profile build has no stamp"
                .to_owned()
        )
    );

    profile::write_stamps(&state, &edited);
    write_good_settings(dir.path(), &edited);
    let report = super::doctor(dir.path(), None, &edited, &WithProfileMcp(&logged_in()));
    assert!(report.passed(), "{:?}", report.lines());
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
    assert_eq!(
        commands.calls()[0],
        ["jj", "--version"],
        "the tool is asked first; the stamped profiles' logins after"
    );
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
    assert!(before.lines()[0].starts_with("design dir not found: "));
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
        "design index not found: {}",
        dir.path().join("docs/index.md").display()
    );
    assert!(report.lines().contains(&missing), "{:?}", report.lines());
    assert_eq!(report.lines()[0], missing);
    assert!(report.lines()[1].starts_with("design dir: ok"));
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
fn the_design_index_is_the_manifests_own_path() {
    let root = Path::new("/project");
    let named = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\nindex = \"meta/p.md\"\n");
    let default = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    assert_eq!(
        named.design.index_path(root),
        Some(PathBuf::from("/project/docs/meta/p.md"))
    );
    assert_eq!(
        default.design.index_path(root),
        Some(
            root.join("docs")
                .join(crate::manifest::DEFAULT_DESIGN_INDEX)
        )
    );
    assert_eq!(manifest("").design.index_path(root), None);
}

#[test]
fn a_missing_actions_dir_fails_and_names_the_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    // The `doctor` helper provides the actions; the bare one does not.
    let report = super::doctor(dir.path(), None, &manifest(""), &FakeCommands::new());
    assert!(!report.passed());
    let missing = format!(
        "actions dir not found: {}",
        dir.path().join(".zaf/actions").display()
    );
    assert!(report.lines().contains(&missing), "{:?}", report.lines());
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
            .first()
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
        profile_calls(&commands),
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
    let report = doctor_unstamped(dir.path(), &manifest(""), &commands);
    assert_eq!(report.logins.len(), 2);
    assert!(report.logins.iter().all(|check| !check.ok));
    assert!(profile_calls(&commands).is_empty());
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
    write_good_settings(root, &manifest(""));
    doctor_with(
        root,
        None,
        &manifest(""),
        &WithProfileMcp(commands),
        claude_home,
        now,
    )
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
    profile::write_stamps(&dir.path().join(STATE_DIR), &manifest(""));
    write_good_settings(dir.path(), &manifest(""));
    let report = doctor_with(
        dir.path(),
        None,
        &manifest(""),
        &WithProfileMcp(&logged_out()),
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
    let report = doctor_unstamped(dir.path(), &manifest(PACK), &commands);
    assert!(
        report.lines().contains(
            &"profile build: pack missing (fix: zaf profile init installs it)".to_owned()
        )
    );
    assert!(!report.passed(), "a missing pack fails the report");
    assert!(profile_calls(&commands).is_empty());
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

/// A project whose lean and build profiles hold zaf's entry and `settings` (when given), checked
/// with `get` as what their `claude mcp get zaf` prints; the profile rows and the report.
fn profile_mcp_rows(
    settings: Option<serde_json::Value>,
    mcp_json: Option<&str>,
    get: Option<&str>,
) -> (Vec<String>, bool) {
    let dir = tempfile::tempdir().expect("temp dir");
    let (lean, build) = profile_homes(dir.path());
    if let Some(text) = mcp_json {
        put(dir.path(), ".mcp.json", text);
    }
    if let Some(settings) = settings {
        for home in [&lean, &build] {
            fs::write(home.join("settings.json"), settings.to_string()).expect("settings");
        }
    }
    let commands = match get {
        Some(text) => FakeCommands::new().with("env", text),
        None => FakeCommands::new().failing("env", "No MCP server found with name: zaf"),
    };
    let report = raw(dir.path(), &manifest(""), &commands);
    let rows = report
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("profile ") && line.contains(": mcp "))
        .collect();
    (rows, report.passed())
}

#[test]
fn doctor_profile_mcp_ok() {
    let (rows, _) = profile_mcp_rows(Some(good_settings()), None, Some(profile::GET_SHOWS_OURS));
    assert_eq!(rows, ["profile lean: mcp ok", "profile build: mcp ok"]);
    let dir = tempfile::tempdir().expect("temp dir");
    profile_homes(dir.path());
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(report.passed(), "{:?}", report.lines());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["profile_mcp"][0]["ok"], true);
}

#[test]
fn doctor_profile_mcp_server_missing_fails() {
    let (rows, passed) = profile_mcp_rows(Some(good_settings()), None, None);
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert!(row.contains("server zaf missing"), "{row}");
        assert!(row.contains("zaf profile init"), "{row}");
    }
    assert!(!passed);
}

#[test]
fn doctor_profile_still_allowing_bash_zaf_fails() {
    let mut settings = good_settings();
    settings["permissions"]["allow"] = serde_json::json!(["mcp__zaf__*", "Bash(zaf *)"]);
    let (rows, passed) = profile_mcp_rows(Some(settings), None, Some(profile::GET_SHOWS_OURS));
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert!(row.contains("Bash(zaf *) still allowed"), "{row}");
        assert!(row.contains("zaf profile init"), "{row}");
    }
    assert!(!passed);
}

#[test]
fn doctor_profile_with_connectors_on_fails() {
    let mut settings = good_settings();
    settings["disableClaudeAiConnectors"] = serde_json::json!(false);
    let (rows, passed) = profile_mcp_rows(Some(settings), None, Some(profile::GET_SHOWS_OURS));
    assert!(
        rows.iter()
            .all(|row| row.contains("disableClaudeAiConnectors")),
        "{rows:?}"
    );
    assert!(!passed);
    let mut settings = good_settings();
    settings
        .as_object_mut()
        .expect("object")
        .remove("disableClaudeAiConnectors");
    let (rows, passed) = profile_mcp_rows(Some(settings), None, Some(profile::GET_SHOWS_OURS));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(!passed);
}

#[test]
fn doctor_profile_missing_mcpjson_name_fails() {
    let mut settings = good_settings();
    settings["disabledMcpjsonServers"] = serde_json::json!(["memory"]);
    let mcp_json = r#"{"mcpServers":{"memory":{},"other":{}}}"#;
    let (rows, passed) = profile_mcp_rows(
        Some(settings),
        Some(mcp_json),
        Some(profile::GET_SHOWS_OURS),
    );
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert!(row.contains("disabledMcpjsonServers lacks other"), "{row}");
        assert!(!row.contains("lacks memory"), "{row}");
    }
    assert!(!passed);
}

#[test]
fn a_profile_without_settings_or_not_generated_is_named_or_left_out() {
    // A home with no settings file fails; a home not made gets no row and asks nothing.
    let (rows, passed) = profile_mcp_rows(None, None, Some(profile::GET_SHOWS_OURS));
    assert!(
        rows.iter().all(|row| row.contains("settings.json")),
        "{rows:?}"
    );
    assert!(!passed);
    let dir = tempfile::tempdir().expect("temp dir");
    let commands = FakeCommands::new();
    let report = doctor_unstamped(dir.path(), &manifest(""), &commands);
    assert!(report.profile_mcp.is_empty());
    assert!(profile_calls(&commands).is_empty());
}

/// Writes `text` to `<root>/<name>`, making its folder.
fn put(root: &Path, name: &str, text: &str) {
    let file = root.join(name);
    fs::create_dir_all(file.parent().expect("parent")).expect("folder");
    fs::write(file, text).expect("file");
}

#[test]
fn doctor_names_the_manifest_files_it_reads() {
    let dir = tempfile::tempdir().expect("temp dir");
    put(dir.path(), ".zaf.toml", HEAD);
    put(dir.path(), ".zaf/config.toml", "[spawn]\nslots = 2\n");
    put(dir.path(), ".zaf/local.toml", "[spawn]\nslots = 4\n");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    let expected = [
        dir.path().join(".zaf/local.toml"),
        dir.path().join(".zaf.toml"),
        dir.path().join(".zaf/config.toml"),
    ];
    assert_eq!(report.manifest, expected, "highest layer first");
    let named: Vec<String> = expected
        .iter()
        .map(|file| file.display().to_string())
        .collect();
    let line = format!("manifest: {}", named.join(", "));
    assert!(report.lines().contains(&line), "{:?}", report.lines());
    // Without a local file it is not named.
    fs::remove_file(dir.path().join(".zaf/local.toml")).expect("remove");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert_eq!(report.manifest, expected[1..]);
}

#[test]
fn doctor_warns_about_an_unread_toml() {
    let dir = tempfile::tempdir().expect("temp dir");
    put(dir.path(), ".zaf.toml", HEAD);
    put(dir.path(), ".zaf/local.toml", "");
    put(dir.path(), ".zaf/extra.toml", "[spawn]\nslots = 9\n");
    // A `.toml` below `.zaf/` is nobody's file to warn about.
    put(dir.path(), ".zaf/profiles/p/settings.toml", "");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    let warnings: Vec<String> = report
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("unread:"))
        .collect();
    assert_eq!(warnings, ["unread: .zaf/extra.toml"]);
    assert!(report.passed(), "a warning does not fail the report");
}

/// The override notes of a report, as lines.
fn override_notes(report: &Report) -> Vec<String> {
    report
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("role override "))
        .collect()
}

#[test]
fn bash_zaf_lines_fire_on_a_code_span_and_the_word_bash() {
    let text = "# Role\n\nrun `zaf check pr` in Bash\nand `zaf board ready`, then stop";
    assert_eq!(bash_zaf_lines(text), [3]);
}

#[test]
fn bash_zaf_lines_stay_quiet_on_a_line_naming_the_tool() {
    let text = "call the tool `check` (`mcp__zaf__check`), not `zaf check` in Bash";
    assert!(bash_zaf_lines(text).is_empty());
}

#[test]
fn bash_zaf_lines_stay_quiet_on_never() {
    for text in [
        "Never type `zaf` in Bash; it is denied.",
        "never run `zaf check` in Bash",
        "NEVER run `zaf check` in Bash",
    ] {
        assert!(bash_zaf_lines(text).is_empty(), "{text}");
    }
}

#[test]
fn doctor_notes_a_role_override_typing_zaf_in_bash() {
    let dir = tempfile::tempdir().expect("temp dir");
    put(
        dir.path(),
        ".zaf/roles/programmer.md",
        "# Programmer\n\nWhen done, run `zaf check pr` in Bash.\n",
    );
    put(dir.path(), ".zaf/roles/planning.md", "Plan well.\n");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert_eq!(
        override_notes(&report),
        [
            "role override .zaf/roles/programmer.md:3 tells a model to type zaf in Bash; \
             instances call zaf's MCP tools (mcp__zaf__<verb>)"
        ]
    );
    assert_eq!(report.role_overrides.len(), 1);
    assert_eq!(report.role_overrides[0].line, 3);
    assert!(report.passed(), "a note does not fail the report");
}

#[test]
fn doctor_quiet_on_an_override_naming_the_tool() {
    let dir = tempfile::tempdir().expect("temp dir");
    put(
        dir.path(),
        ".zaf/roles/programmer.md",
        "Call the tool `check` (`mcp__zaf__check`).\nNever type `zaf` in Bash; it is denied.\n",
    );
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(override_notes(&report).is_empty(), "{:?}", report.lines());
    assert!(report.role_overrides.is_empty());
}

#[test]
fn doctor_quiet_without_role_overrides() {
    let dir = tempfile::tempdir().expect("temp dir");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(override_notes(&report).is_empty());
    fs::create_dir_all(dir.path().join(".zaf/roles")).expect("roles");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(override_notes(&report).is_empty(), "an empty folder too");
}

/// A board whose backend answers the checks it was given, and trusts whom it was told.
struct AnsweringBoard(
    Vec<crate::ports::AccessCheck>,
    Result<Vec<crate::ports::Login>, crate::ports::PortError>,
);

impl AnsweringBoard {
    /// One that answers `checks` and trusts its own login, `zaf-bot`.
    fn answering(checks: Vec<crate::ports::AccessCheck>) -> Self {
        Self(checks, Ok(vec![crate::ports::Login::from("zaf-bot")]))
    }
}

impl Board for AnsweringBoard {
    fn access(&self) -> Vec<crate::ports::AccessCheck> {
        self.0.clone()
    }

    fn trusted_logins(&self) -> Result<Vec<crate::ports::Login>, crate::ports::PortError> {
        self.1.clone()
    }

    fn ready(&self) -> Result<Vec<crate::domain::Ticket>, crate::ports::PortError> {
        Ok(Vec::new())
    }

    fn task(
        &self,
        number: crate::domain::TicketId,
    ) -> Result<crate::domain::Ticket, crate::ports::PortError> {
        Err(crate::ports::PortError::NotFound(number.to_string()))
    }

    fn claim(
        &self,
        _: crate::domain::TicketId,
        _: &crate::domain::Instance,
    ) -> Result<crate::ports::ClaimOutcome, crate::ports::PortError> {
        unreachable!("doctor claims nothing")
    }

    fn heartbeat(
        &self,
        _: crate::domain::TicketId,
        _: &crate::domain::Instance,
    ) -> Result<(), crate::ports::PortError> {
        unreachable!("doctor touches no claim")
    }

    fn claims(&self) -> Result<Vec<crate::ports::ClaimRecord>, crate::ports::PortError> {
        Ok(Vec::new())
    }

    fn release(
        &self,
        _: crate::domain::TicketId,
        _: &crate::domain::Instance,
    ) -> Result<(), crate::ports::PortError> {
        unreachable!("doctor releases nothing")
    }

    fn set_state(
        &self,
        _: crate::domain::TicketId,
        _: crate::domain::TicketState,
    ) -> Result<(), crate::ports::PortError> {
        unreachable!("doctor sets no state")
    }

    fn comment(&self, _: crate::domain::TicketId, _: &str) -> Result<(), crate::ports::PortError> {
        unreachable!("doctor comments nowhere")
    }

    fn report(
        &self,
        _: crate::domain::TicketId,
        _: &crate::domain::Instance,
        _: &str,
    ) -> Result<(), crate::ports::PortError> {
        unreachable!("doctor reports nowhere")
    }

    fn questions(&self) -> Result<Vec<crate::domain::Ticket>, crate::ports::PortError> {
        Ok(Vec::new())
    }

    fn open_question(
        &self,
        _: &crate::ports::NewQuestion,
    ) -> Result<crate::domain::TicketId, crate::ports::PortError> {
        unreachable!("doctor asks nothing")
    }

    fn record_ruling(
        &self,
        _: crate::domain::TicketId,
        _: &str,
    ) -> Result<Option<crate::domain::TicketId>, crate::ports::PortError> {
        unreachable!("doctor rules nothing")
    }
}

fn access(what: &str, ok: bool, detail: &str) -> crate::ports::AccessCheck {
    crate::ports::AccessCheck {
        what: what.to_owned(),
        ok,
        detail: detail.to_owned(),
    }
}

#[test]
fn a_report_that_asked_no_board_has_no_board_line_and_no_json_key() {
    let dir = tempfile::tempdir().expect("temp dir");
    let report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(report.board.is_empty());
    assert!(report.lines().iter().all(|line| !line.starts_with("board")));
    let json = serde_json::to_value(&report).expect("serializes");
    assert!(json.get("board").is_none(), "{json}");
}

#[test]
fn a_board_reached_through_a_tool_is_asked_only_whom_it_trusts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    let board = crate::fakes::FakeBoard::default();
    board.set_trusted_logins(&["alice"]);
    report.board = board_checks(&board);
    assert_eq!(
        report.board,
        [BackendCheck {
            what: "trusted logins".to_owned(),
            ok: true,
            detail: "zaf-bot, alice".to_owned(),
        }]
    );
    assert!(
        report
            .lines()
            .contains(&"board trusted logins: ok (zaf-bot, alice)".to_owned()),
        "{:?}",
        report.lines()
    );
    assert!(report.passed(), "{:?}", report.lines());
}

#[test]
fn a_board_that_cannot_read_its_own_login_fails_the_report_and_names_the_fix() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(report.passed(), "{:?}", report.lines());
    let unread = crate::ports::PortError::Backend(
        "the board cannot read the login its token posts as, whose zaf comments it trusts \
         (Forgejo answered 403): set `[board] trusted_logins` in the manifest, or give the \
         token the right to read its user"
            .to_owned(),
    );
    report.board = board_checks(&AnsweringBoard(
        vec![access(
            "token",
            true,
            "`FORGEJO_TOKEN` reads and writes o/r",
        )],
        Err(unread),
    ));
    let lines = report.lines();
    assert_eq!(
        lines[0],
        "board trusted logins failed: backend failed: the board cannot read the login its token \
         posts as, whose zaf comments it trusts (Forgejo answered 403): set `[board] \
         trusted_logins` in the manifest, or give the token the right to read its user",
        "the failing check comes first: {lines:?}"
    );
    assert!(!report.passed());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["board"][1]["what"], "trusted logins");
    assert_eq!(json["board"][1]["ok"], false);
}

#[test]
fn the_boards_answers_are_lines_and_a_failed_one_fails_the_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    assert!(report.passed(), "{:?}", report.lines());
    report.board = board_checks(&AnsweringBoard::answering(vec![
        access("instance", true, "Forgejo 15.0.11 at http://forgejo.test"),
        access(
            "token",
            false,
            "`FORGEJO_TOKEN` is not set: export a Forgejo access token in it, with read and write on the repository's issues and pull requests",
        ),
    ]));
    let lines = report.lines();
    assert!(
        lines.contains(&"board instance: ok (Forgejo 15.0.11 at http://forgejo.test)".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(
            &"board token failed: `FORGEJO_TOKEN` is not set: export a Forgejo access token in it, with read and write on the repository's issues and pull requests"
                .to_owned()
        ),
        "{lines:?}"
    );
    assert!(!report.passed());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["board"][0]["what"], "instance");
    assert_eq!(json["board"][1]["ok"], false);
}

#[test]
fn the_forges_answers_are_lines_and_a_failed_one_comes_first_and_fails_the_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    report.forge = forge_checks(&crate::fakes::FakeForge::default());
    assert!(
        report.forge.is_empty(),
        "a forge reached through gh answers nothing"
    );
    let json = serde_json::to_value(&report).expect("serializes");
    assert!(json.get("forge").is_none(), "{json}");

    report.board = board_checks(&AnsweringBoard::answering(vec![
        access("instance", true, "Forgejo 15.0.11 at http://forgejo.test"),
        access("token", true, "`FORGEJO_TOKEN` reads and writes o/r"),
    ]));
    report.forge = vec![BackendCheck::from(access(
        "token",
        false,
        "`FORGEJO_TOKEN` cannot write o/r's pull requests: Forgejo answered 403 forbidden",
    ))];
    let lines = report.lines();
    let board = lines
        .iter()
        .position(|line| line == "board token: ok (`FORGEJO_TOKEN` reads and writes o/r)")
        .expect("the board's token line");
    let forge = lines
        .iter()
        .position(|line| {
            line == "forge token failed: `FORGEJO_TOKEN` cannot write o/r's pull requests: \
                     Forgejo answered 403 forbidden"
        })
        .expect("the forge's token line");
    assert!(forge < board, "{lines:?}");
    assert!(!report.passed());
    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["forge"][0]["what"], "token");
    assert_eq!(json["forge"][0]["ok"], false);

    report.forge[0].ok = true;
    assert!(report.passed(), "{:?}", report.lines());
}

#[test]
fn gh_is_not_needed_when_neither_the_board_nor_the_forge_is_on_github() {
    let forgejo = "[project]\nname = \"bookshelf\"\n\
                   [board]\nkind = \"forgejo-issues\"\nrepo = \"o/r\"\nurl = \"http://forgejo.test\"\n\
                   [forge]\nkind = \"forgejo\"\nrepo = \"o/r\"\nurl = \"http://forgejo.test\"\n\
                   [requires]\ngh = \">=2\"\n";
    let manifest: Manifest = forgejo.parse().expect("a valid manifest");
    let dir = tempfile::tempdir().expect("temp dir");

    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert_eq!(
        status_of(&report, "gh"),
        ToolStatus::NotNeeded {
            found: "missing".to_owned()
        }
    );
    assert!(report.passed(), "{:?}", report.lines());
    assert!(
        report.lines().contains(
            &"gh: missing, not needed: neither the board nor the forge is on GitHub".to_owned()
        ),
        "{:?}",
        report.lines()
    );

    // A gh that is there is reported as it is.
    let report = doctor(
        dir.path(),
        &manifest,
        &FakeCommands::new().with("gh", "gh version 2.40.1 (2024-01-01)"),
    );
    assert_eq!(
        status_of(&report, "gh"),
        ToolStatus::Ok {
            found: "2.40.1".to_owned()
        }
    );

    // A forge on GitHub needs it, whatever the board.
    let github_forge = forgejo.replace(
        "[forge]\nkind = \"forgejo\"\nrepo = \"o/r\"\nurl = \"http://forgejo.test\"\n",
        "[forge]\nkind = \"github\"\nrepo = \"o/r\"\n",
    );
    let manifest: Manifest = github_forge.parse().expect("a valid manifest");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    assert_eq!(status_of(&report, "gh"), ToolStatus::Missing);
    assert!(!report.passed());

    // And so does a board on GitHub, with no forge at all.
    let report = doctor(
        dir.path(),
        &self::manifest("[requires]\ngh = \"*\"\n"),
        &FakeCommands::new(),
    );
    assert_eq!(status_of(&report, "gh"), ToolStatus::Missing);
}

fn hub_report(home: Option<&Path>, commands: &FakeCommands) -> Report {
    let dir = tempfile::tempdir().expect("project");
    let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    let before = report.passed();
    report.hub = Some(hub::check(
        home,
        commands,
        std::net::IpAddr::from([192, 168, 1, 5]),
        &|_| true,
    ));
    assert_eq!(report.passed(), before, "the hub line never moves the exit");
    report
}

#[test]
fn hub_running() {
    let home = tempfile::tempdir().expect("home");
    let holder = crate::ports::Holder {
        pid: 10,
        start: "100".to_owned(),
    };
    crate::app::hub::lock::HubLock::take(home.path(), &holder, 8080, &|_| true).expect("lock");
    let commands = FakeCommands::new().with("systemctl", "LoadState=loaded\nActiveState=active\n");
    let lines = hub_report(Some(home.path()), &commands).lines();
    assert!(lines.contains(&"hub service: running on port 8080 (http://192.168.1.5:8080/)".into()));
}

#[test]
fn hub_stopped() {
    let commands =
        FakeCommands::new().with("systemctl", "LoadState=loaded\nActiveState=inactive\n");
    let lines = hub_report(None, &commands).lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("hub service: installed, stopped"))
    );
}

#[test]
fn hub_not_installed() {
    let commands =
        FakeCommands::new().with("systemctl", "LoadState=not-found\nActiveState=inactive\n");
    let report = hub_report(None, &commands);
    assert!(
        report
            .lines()
            .iter()
            .any(|line| line.contains("zaf hub service install"))
    );
}

/// What `claude mcp get zaf` prints when another entry holds the name.
const GET_SHOWS_OTHER: &str = "zaf:\n  Scope: User config (available in all your projects)\n  Status: \u{2713} Connected\n  Type: stdio\n  Command: /opt/other/zaf\n  Args: serve\n  Environment:\n\nTo remove this server, run: claude mcp remove \"zaf\" -s user\n";

fn mcp_lines(commands: &FakeCommands) -> (Vec<String>, bool) {
    let dir = tempfile::tempdir().expect("project");
    let with_claude = doctor(dir.path(), &manifest(""), commands);
    let without = doctor(dir.path(), &manifest(""), &FakeCommands::new());
    let lines = with_claude
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("mcp:"))
        .collect();
    assert_eq!(
        with_claude.passed(),
        without.passed(),
        "the mcp row never moves the exit"
    );
    (lines, with_claude.passed())
}

#[test]
fn doctor_mcp_row_ok_at_user_scope() {
    let commands = FakeCommands::new().with("claude", profile::GET_SHOWS_OURS);
    let (lines, _) = mcp_lines(&commands);
    assert_eq!(lines, ["mcp: ok (user scope)"]);
    assert!(commands.calls().contains(&vec![
        "claude".into(),
        "mcp".into(),
        "get".into(),
        "zaf".into()
    ]));
}

#[test]
fn doctor_mcp_row_missing_names_the_fix() {
    // A failed `get` is how Claude says it knows no such server.
    let failing = FakeCommands::new().failing("claude", "No MCP server found with name: zaf");
    // No `claude` at all gives the same row, not a doctor error.
    for commands in [failing, FakeCommands::new()] {
        let (lines, passed) = mcp_lines(&commands);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("mcp: missing"), "{lines:?}");
        assert!(lines[0].contains("zaf setup"), "{lines:?}");
        assert!(lines[0].contains(&profile::mcp_add_line(None)), "{lines:?}");
        assert!(passed, "the report still passes");
    }
}

#[test]
fn doctor_mcp_row_different_entry() {
    let commands = FakeCommands::new().with("claude", GET_SHOWS_OTHER);
    let (lines, passed) = mcp_lines(&commands);
    assert_eq!(
        lines,
        ["mcp: a different zaf entry (fix: zaf setup asks to replace it)"]
    );
    assert!(passed, "the report still passes");
}

#[test]
fn doctor_mcp_row_never_fails_the_report() {
    for check in [McpCheck::Ok, McpCheck::Missing, McpCheck::Different] {
        let dir = tempfile::tempdir().expect("project");
        let mut report = doctor(dir.path(), &manifest(""), &FakeCommands::new());
        report.mcp = McpCheck::Ok;
        let before = report.passed();
        report.mcp = check;
        assert_eq!(report.passed(), before, "{check:?}");
        assert!(report.lines().contains(&check.line()), "{check:?}");
    }
}

/// A report whose design dir (ok) runs before its design index (missing).
fn passing_then_failing() -> (tempfile::TempDir, Report) {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(dir.path().join("docs")).expect("design dir");
    let manifest = manifest("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n");
    let report = doctor(dir.path(), &manifest, &FakeCommands::new());
    (dir, report)
}

#[test]
fn failures_come_first() {
    let (dir, report) = passing_then_failing();
    let lines = report.lines();
    let missing = format!(
        "design index not found: {}",
        dir.path().join("docs/index.md").display()
    );
    assert_eq!(lines[0], missing, "{lines:?}");
    assert!(lines[1].starts_with("design dir: ok"), "{lines:?}");
}

#[test]
fn a_failed_checks_problems_stay_under_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = manifest("");
    let mut report = doctor(dir.path(), &manifest, &FakeCommands::new());
    report.needed.push(NeededCheck {
        name: "gone".to_owned(),
        tier: None,
        needed_by: vec!["a step".to_owned()],
        ok: false,
        problems: vec!["no file".to_owned()],
    });
    let lines = report.lines();
    assert!(
        lines[0].starts_with("needed gone: no provider"),
        "{lines:?}"
    );
    assert_eq!(lines[1], "  no file", "{lines:?}");
}

#[test]
fn reordering_keeps_every_line() {
    let (_dir, report) = passing_then_failing();
    let mut shown = report.lines();
    let mut ran: Vec<String> = report
        .groups()
        .iter()
        .flat_map(|group| &group.lines)
        .map(plain)
        .collect();
    assert_eq!(shown.len(), ran.len());
    shown.sort();
    ran.sort();
    assert_eq!(shown, ran);
}
