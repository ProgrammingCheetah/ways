use std::fs;
use std::sync::Mutex;

use super::*;
use crate::ports::{GateRun, PortError};

/// A shell that records each line and answers with a fixed verdict; it never runs anything.
struct RecordingShell {
    verdict: GateVerdict,
    lines: Mutex<Vec<String>>,
}

impl RecordingShell {
    fn answering(verdict: GateVerdict) -> Self {
        Self {
            verdict,
            lines: Mutex::new(Vec::new()),
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lock").clone()
    }
}

impl Shell for RecordingShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.lines.lock().expect("lock").push(line.to_string());
        Ok(GateRun {
            verdict: self.verdict,
            output: "said no".to_owned(),
        })
    }
}

/// `init` for a project rooted at the state folder's parent, with a shell that must stay unused.
fn init_here(state: &Path, manifest: &Manifest) -> Result<Report, ProfileError> {
    let shell = RecordingShell::answering(GateVerdict::Failed);
    let report = init(state.parent().expect("a parent"), state, manifest, &shell);
    assert!(shell.lines().is_empty(), "no pack, no shell");
    report
}

const HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn manifest(extra: &str) -> Manifest {
    format!("{HEAD}{extra}").parse().expect("a valid manifest")
}

fn settings(state: &Path, profile: &str) -> Value {
    let text = fs::read_to_string(state.join("profiles").join(profile).join("settings.json"))
        .expect("settings");
    serde_json::from_str(&text).expect("settings parse")
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|item| item.as_str().expect("a string"))
        .collect()
}

#[test]
fn each_named_profile_gets_its_files_and_folders() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let report = init_here(&state, &manifest("")).expect("generates");
    assert_eq!(report.profiles, ["lean", "build"]);
    for profile in ["lean", "build"] {
        let home = state.join("profiles").join(profile);
        assert!(home.join("settings.json").is_file());
        assert!(home.join("CLAUDE.md").is_file());
        for folder in PROFILE_DIRS {
            let folder = home.join(folder);
            assert!(folder.is_dir(), "{}", folder.display());
            assert_eq!(fs::read_dir(&folder).expect("reads").count(), 0);
        }
    }
    assert_eq!(report.written.len(), 4);
    assert!(
        report
            .written
            .iter()
            .all(|file| file.change == Change::Created)
    );
}

#[test]
fn a_second_run_changes_nothing_and_says_so() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest =
        manifest("[gate]\nfull = \"just gate\"\n[forbidden]\ncommands = [\"git push\"]\n");
    init_here(&state, &manifest).expect("first");
    let before = fs::read(state.join("profiles/build/settings.json")).expect("reads");
    let second = init_here(&state, &manifest).expect("second");
    assert!(second.unchanged());
    assert_eq!(second.lines(), ["nothing changed"]);
    assert_eq!(
        fs::read(state.join("profiles/build/settings.json")).expect("reads"),
        before
    );
}

#[test]
fn a_hand_edited_file_is_rewritten_and_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest("");
    init_here(&state, &manifest).expect("first");
    let edited = state.join("profiles/lean/CLAUDE.md");
    let original = fs::read(&edited).expect("reads");
    fs::write(&edited, "my own rules\n").expect("edits");
    let report = init_here(&state, &manifest).expect("second");
    assert_eq!(
        report.written,
        [Written {
            path: PathBuf::from("profiles/lean/CLAUDE.md"),
            change: Change::Rewritten
        }]
    );
    assert_eq!(report.lines(), ["rewritten profiles/lean/CLAUDE.md"]);
    assert_eq!(fs::read(&edited).expect("reads"), original);
}

#[test]
fn settings_carry_the_forbidden_commands_as_denies_and_the_gate_as_allows() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest(
        "[requires]\njj = \"*\"\ngh = \">=2\"\ncargo = \"*\"\n\
         [gate]\nfull = \"just gate\"\ntests = \"cargo test -p {crate} {name}\"\n\
         [forbidden]\ncommands = [\"git push\", \"rm -rf\"]\n",
    );
    init_here(&state, &manifest).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let permissions = &settings["permissions"];
        assert_eq!(
            strings(&permissions["deny"]),
            [
                "Bash(git push *)",
                "Bash(git push)",
                "Bash(rm -rf *)",
                "Bash(rm -rf)"
            ]
        );
        let allow = strings(&permissions["allow"]);
        for wanted in [
            "Bash(zaf *)",
            "Bash(jj *)",
            "Bash(gh *)",
            "Bash(cargo *)",
            "Bash(just gate)",
            "Bash(cargo test -p * *)",
        ] {
            assert!(allow.contains(&wanted), "{profile} allows {wanted}");
        }
        assert!(settings.get("hooks").is_none());
        assert!(settings.get("mcpServers").is_none());
    }
}

#[test]
fn an_allow_that_is_forbidden_is_dropped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let forbidding = manifest("[requires]\ngh = \"*\"\n[forbidden]\ncommands = [\"gh *\"]\n");
    init_here(&state, &forbidding).expect("generates");
    let settings = settings(&state, "lean");
    assert!(!strings(&settings["permissions"]["allow"]).contains(&"Bash(gh *)"));
}

#[test]
fn model_and_effort_come_from_the_roles_and_only_build_compacts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("generates");
    let lean = settings(&state, "lean");
    assert_eq!(lean["model"], "sonnet");
    assert_eq!(lean["effortLevel"], "high");
    assert!(lean.get("autoCompactWindow").is_none());
    let build = settings(&state, "build");
    assert_eq!(build["model"], "opus");
    assert_eq!(build["effortLevel"], "xhigh");
    assert_eq!(build["autoCompactWindow"], 150_000);
}

#[test]
fn max_effort_is_written_as_the_highest_value_the_setting_takes() {
    assert_eq!(effort_level(Effort::Max), "xhigh");
    assert_eq!(effort_level(Effort::Low), "low");
}

#[test]
fn claude_md_is_the_interface_then_the_project_rules_when_present() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("generates");
    let path = state.join("profiles/lean/CLAUDE.md");
    let without = fs::read_to_string(&path).expect("reads");
    assert_eq!(without, format!("{}\n", INTERFACE_PAGE.trim_end()));
    fs::create_dir_all(state.join("roles")).expect("roles");
    fs::write(state.join(PROJECT_RULES), "Rule one.\n").expect("rules");
    let report = init_here(&state, &manifest("")).expect("again");
    assert_eq!(report.written.len(), 2);
    let with = fs::read_to_string(&path).expect("reads");
    assert_eq!(with, format!("{without}\nRule one.\n"));
}

#[test]
fn profiles_without_a_generated_home_are_skipped_and_named() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let extra =
        "[roles.attended]\nmodel = \"opus\"\neffort = \"max\"\nprofile = \"design\"\nturns = 10\n";
    let report = init_here(&state, &manifest(extra)).expect("generates");
    assert_eq!(report.skipped, ["design"]);
    assert!(!state.join("profiles/design").exists());
}

#[test]
fn existing_session_folders_are_left_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("first");
    let kept = state.join("profiles/lean/projects/session.jsonl");
    fs::write(&kept, "x").expect("writes");
    init_here(&state, &manifest("")).expect("second");
    assert_eq!(fs::read_to_string(kept).expect("reads"), "x");
}

#[test]
fn the_allow_list_comes_from_requires_not_from_the_pack() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(
        &state,
        &manifest("[requires]\nmake = \"*\"\njj = \">=0.20\"\nzaf = \"2.0.0\"\n"),
    )
    .expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        assert_eq!(
            strings(&settings["permissions"]["allow"]),
            ["Bash(zaf *)", "Bash(jj *)", "Bash(make *)"],
            "{profile}: zaf once, each required tool, nothing else"
        );
    }
}

#[test]
fn a_project_requiring_nothing_allows_only_zaf_and_its_gate() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("[gate]\nfull = \"make check\"\n")).expect("generates");
    let settings = settings(&state, "lean");
    assert_eq!(
        strings(&settings["permissions"]["allow"]),
        ["Bash(zaf *)", "Bash(make check)"]
    );
}

/// A pack checkout with a manifest, a nested skill and a hidden folder.
fn write_pack(dir: &Path) {
    fs::create_dir_all(dir.join(".claude-plugin")).expect("pack dirs");
    fs::create_dir_all(dir.join("skills/plan")).expect("pack dirs");
    fs::write(
        dir.join(".claude-plugin/plugin.json"),
        "{\"name\":\"kit\"}\n",
    )
    .expect("writes");
    fs::write(dir.join("skills/plan/SKILL.md"), "# Plan\n").expect("writes");
    fs::write(dir.join("README.md"), "kit\n").expect("writes");
}

#[test]
fn a_pack_from_a_path_is_copied_into_the_profiles_that_carry_plugins_and_a_rerun_changes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_pack(&dir.path().join("vendor/kit"));
    let manifest = manifest("[pack]\npath = \"vendor/kit\"\n");
    let first = init_here(&state, &manifest).expect("generates");
    let vendored = state.join("profiles/build/plugins").join(PACK_PLUGIN);
    for file in [
        ".claude-plugin/plugin.json",
        "skills/plan/SKILL.md",
        "README.md",
    ] {
        assert_eq!(
            fs::read(vendored.join(file)).expect("copied"),
            fs::read(dir.path().join("vendor/kit").join(file)).expect("source"),
            "{file}"
        );
    }
    assert!(
        first
            .written
            .iter()
            .any(|file| file.path == Path::new("profiles/build/plugins/teams/README.md"))
    );
    let lean_plugins = state.join("profiles/lean/plugins");
    assert_eq!(
        fs::read_dir(&lean_plugins).expect("reads").count(),
        0,
        "lean runs plugin-free"
    );
    let second = init_here(&state, &manifest).expect("again");
    assert!(second.unchanged(), "{:?}", second.lines());
    assert_eq!(second.lines(), ["nothing changed"]);
}

#[test]
fn a_file_dropped_from_the_pack_source_is_removed_from_the_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let source = dir.path().join("kit");
    write_pack(&source);
    let manifest = manifest("[pack]\npath = \"kit\"\n");
    init_here(&state, &manifest).expect("first");
    fs::remove_file(source.join("README.md")).expect("drops");
    let report = init_here(&state, &manifest).expect("second");
    assert_eq!(
        report.written,
        [Written {
            path: PathBuf::from("profiles/build/plugins/teams/README.md"),
            change: Change::Removed
        }]
    );
    assert!(
        !state
            .join("profiles/build/plugins/teams/README.md")
            .exists()
    );
}

#[test]
fn a_missing_pack_folder_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let result = init_here(&state, &manifest("[pack]\npath = \"nowhere\"\n"));
    assert!(matches!(result, Err(ProfileError::NoPack(_))));
}

#[test]
fn settings_enable_the_pack_and_never_wait_on_a_prompt() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_pack(&dir.path().join("kit"));
    init_here(
        &state,
        &manifest("[pack]\npath = \"kit\"\n[forbidden]\ncommands = [\"git push\"]\n"),
    )
    .expect("generates");
    let build = settings(&state, "build");
    assert_eq!(build["enabledPlugins"], json!({ "teams@ways": true }));
    assert_eq!(build["permissions"]["defaultMode"], "acceptEdits");
    assert!(strings(&build["permissions"]["allow"]).contains(&"Bash(zaf *)"));
    assert!(strings(&build["permissions"]["deny"]).contains(&"Bash(git push)"));
    let lean = settings(&state, "lean");
    assert_eq!(lean["permissions"]["defaultMode"], "acceptEdits");
    assert!(lean.get("enabledPlugins").is_none());
    init_here(&state, &manifest("")).expect("without a pack");
    assert!(settings(&state, "build").get("enabledPlugins").is_none());
}

#[test]
fn a_pack_tag_is_installed_from_the_marketplace_into_the_profile_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest("[pack]\ntag = \"kit--v1.0.0\"\n");
    let shell = RecordingShell::answering(GateVerdict::Passed);
    let report = init(dir.path(), &state, &manifest, &shell).expect("generates");
    let home = state.join("profiles/build");
    assert_eq!(
        shell.lines(),
        [format!(
            "CLAUDE_CONFIG_DIR='{}' claude plugin install 'teams@kit--v1.0.0' --marketplace 'ProgrammingCheetah/ways'",
            home.display()
        )]
    );
    assert!(
        report
            .lines()
            .contains(&"installed teams@kit--v1.0.0 into profiles/build".to_owned())
    );
    assert_eq!(
        settings(&state, "build")["enabledPlugins"]["teams@ways"],
        true
    );
    let again = init(dir.path(), &state, &manifest, &shell).expect("again");
    assert_eq!(shell.lines().len(), 1, "the same tag installs nothing");
    assert!(again.unchanged());
    let newer = manifest_with_tag("kit--v1.1.0");
    init(dir.path(), &state, &newer, &shell).expect("a new tag");
    assert_eq!(shell.lines().len(), 2, "a new tag installs again");
}

fn manifest_with_tag(tag: &str) -> Manifest {
    manifest(&format!("[pack]\ntag = \"{tag}\"\n"))
}

#[test]
fn a_failed_install_is_an_error_naming_the_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let shell = RecordingShell::answering(GateVerdict::Failed);
    let result = init(
        dir.path(),
        &state,
        &manifest_with_tag("kit--v1.0.0"),
        &shell,
    );
    let Err(ProfileError::Install { line, output }) = result else {
        panic!("an install error");
    };
    assert!(line.contains("claude plugin install"), "{line}");
    assert_eq!(output, "said no");
    assert!(
        !state
            .join("profiles/build/plugins")
            .join(INSTALLED_TAG_FILE)
            .exists()
    );
}
