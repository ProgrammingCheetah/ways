use std::fs;

use super::*;

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
    let report = init(&state, &manifest("")).expect("generates");
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
    init(&state, &manifest).expect("first");
    let before = fs::read(state.join("profiles/build/settings.json")).expect("reads");
    let second = init(&state, &manifest).expect("second");
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
    init(&state, &manifest).expect("first");
    let edited = state.join("profiles/lean/CLAUDE.md");
    let original = fs::read(&edited).expect("reads");
    fs::write(&edited, "my own rules\n").expect("edits");
    let report = init(&state, &manifest).expect("second");
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
        "[gate]\nfull = \"just gate\"\ntests = \"cargo test -p {crate} {name}\"\n\
         [forbidden]\ncommands = [\"git push\", \"rm -rf\"]\n",
    );
    init(&state, &manifest).expect("generates");
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
    init(&state, &manifest("[forbidden]\ncommands = [\"gh *\"]\n")).expect("generates");
    let settings = settings(&state, "lean");
    assert!(!strings(&settings["permissions"]["allow"]).contains(&"Bash(gh *)"));
}

#[test]
fn model_and_effort_come_from_the_roles_and_only_build_compacts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init(&state, &manifest("")).expect("generates");
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
    init(&state, &manifest("")).expect("generates");
    let path = state.join("profiles/lean/CLAUDE.md");
    let without = fs::read_to_string(&path).expect("reads");
    assert_eq!(without, format!("{}\n", INTERFACE_PAGE.trim_end()));
    fs::create_dir_all(state.join("roles")).expect("roles");
    fs::write(state.join(PROJECT_RULES), "Rule one.\n").expect("rules");
    let report = init(&state, &manifest("")).expect("again");
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
    let report = init(&state, &manifest(extra)).expect("generates");
    assert_eq!(report.skipped, ["design"]);
    assert!(!state.join("profiles/design").exists());
}

#[test]
fn existing_session_folders_are_left_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init(&state, &manifest("")).expect("first");
    let kept = state.join("profiles/lean/projects/session.jsonl");
    fs::write(&kept, "x").expect("writes");
    init(&state, &manifest("")).expect("second");
    assert_eq!(fs::read_to_string(kept).expect("reads"), "x");
}
