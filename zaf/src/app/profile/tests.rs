use std::fs;
use std::sync::Mutex;

use super::*;
use crate::ports::{GateRun, PortError};

/// A shell that records each line and answers with a fixed verdict; it never runs anything.
///
/// `claude plugin list` and `claude plugin marketplace list` answer with the listings it was given.
struct RecordingShell {
    verdict: GateVerdict,
    plugins: String,
    marketplaces: String,
    lines: Mutex<Vec<String>>,
}

impl RecordingShell {
    fn answering(verdict: GateVerdict) -> Self {
        Self {
            verdict,
            plugins: NO_PLUGINS.to_owned(),
            marketplaces: NO_MARKETPLACES.to_owned(),
            lines: Mutex::new(Vec::new()),
        }
    }

    /// A passing shell whose profile already shows `plugins` and `marketplaces`.
    fn listing(plugins: &str, marketplaces: &str) -> Self {
        Self {
            plugins: plugins.to_owned(),
            marketplaces: marketplaces.to_owned(),
            ..Self::answering(GateVerdict::Passed)
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lock").clone()
    }
}

impl Shell for RecordingShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.lines.lock().expect("lock").push(line.to_string());
        let output = if line.as_ref().ends_with("claude plugin list") {
            self.plugins.clone()
        } else if line.as_ref().ends_with("claude plugin marketplace list") {
            self.marketplaces.clone()
        } else {
            "said no".to_owned()
        };
        Ok(GateRun {
            verdict: self.verdict,
            output,
        })
    }
}

const NO_PLUGINS: &str = "No plugins installed\n";
const NO_MARKETPLACES: &str = "No marketplaces configured\n";
/// What `claude plugin list` shows once the pack is in.
const KIT_LISTED: &str = "Installed plugins:\n\n  \u{276f} other@shelf\n    Version: 9.9.9\n    Scope: user\n    Status: enabled\n\n  \u{276f} kit@shelf\n    Version: 1.2.3\n    Scope: user\n    Status: enabled\n";
const SHELF_ADDED: &str =
    "Configured marketplaces:\n\n  \u{276f} shelf\n    Source: Directory (/somewhere)\n";

/// `init` for a project rooted at the state folder's parent, with a shell that must stay unused.
fn init_here(state: &Path, manifest: &Manifest) -> Result<Report, ProfileError> {
    let shell = RecordingShell::answering(GateVerdict::Failed);
    let root = state.parent().expect("a parent");
    let report = init_with(root, state, manifest, &shell, None);
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
        let deny = strings(&permissions["deny"]);
        for wanted in [
            "Bash(git push *)",
            "Bash(git push)",
            "Bash(git -* push *)",
            "Bash(git -* push)",
            "Bash(rm -rf *)",
            "Bash(rm -rf)",
            "Bash(rm -* -rf *)",
            "Bash(rm -* -rf)",
        ] {
            assert!(deny.contains(&wanted), "{profile} denies {wanted}");
        }
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
        assert!(settings.get("mcpServers").is_none());
    }
}

#[test]
fn profiles_carry_the_hook() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let hooks = settings["hooks"].as_object().expect("hooks");
        assert_eq!(
            hooks.keys().collect::<Vec<_>>(),
            ["PostToolUse"],
            "{profile}: the report hook is the only one"
        );
        let entries = hooks["PostToolUse"].as_array().expect("entries");
        assert_eq!(entries.len(), 1, "{profile}");
        assert_eq!(entries[0]["matcher"], "*", "{profile}: every tool");
        assert_eq!(
            entries[0]["hooks"],
            json!([{ "type": "command", "command": "zaf hub report" }]),
            "{profile}"
        );
    }
}

#[test]
fn the_reports_file_is_git_ignored() {
    use crate::app::setup::IGNORE_LINES;
    assert_eq!(
        IGNORE_LINES[0], "*",
        "the state folder ignores what it does not list"
    );
    let reports = "hub/reports";
    for line in &IGNORE_LINES[1..] {
        let listed = line
            .strip_prefix('!')
            .expect("every line after the first lists a path")
            .trim_end_matches('*')
            .trim_end_matches('/');
        assert!(
            listed != reports && !reports.starts_with(&format!("{listed}/")),
            "{line} would track .zaf/{reports}"
        );
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
            with_read_only(&["Bash(zaf *)", "Bash(jj *)", "Bash(make *)"], true),
            "{profile}: zaf once, each required tool, then the read-only rules"
        );
    }
}

/// The rules a profile's allow list ends with: the read-only basics, and the `git` reads
/// when the repo is colocated.
fn read_only_tail(colocated: bool) -> Vec<String> {
    let mut tail: Vec<String> = READ_ONLY
        .iter()
        .map(|word| format!("Bash({word} *)"))
        .collect();
    if colocated {
        tail.extend(
            ["log", "show", "diff", "status", "blame"]
                .iter()
                .map(|verb| format!("Bash(git {verb} *)")),
        );
    }
    tail
}

/// `head`, the rules the manifest names, followed by the read-only tail.
fn with_read_only(head: &[&str], colocated: bool) -> Vec<String> {
    head.iter()
        .map(|rule| (*rule).to_owned())
        .chain(read_only_tail(colocated))
        .collect()
}

#[test]
fn every_profile_allows_the_read_only_shell_basics() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let allow = strings(&settings["permissions"]["allow"]);
        for word in [
            "ls", "cat", "head", "tail", "wc", "diff", "rg", "grep", "fd", "pwd", "echo", "printf",
            "for", "while", "test", "sort", "uniq", "cut", "stat", "du", "which", "printenv",
        ] {
            let rule = format!("Bash({word} *)");
            assert!(allow.contains(&rule.as_str()), "{profile} allows {rule}");
        }
        assert!(
            !allow.iter().any(|rule| rule.starts_with("Bash(sh ")),
            "{profile}: no shell run from a string"
        );
    }
}

#[test]
fn no_allow_entry_can_write_or_run_arbitrary_commands() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("")).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let allow = strings(&settings["permissions"]["allow"]);
        for rule in ["Bash(env *)", "Bash(find *)", "Bash(sed *)", "Bash(awk *)"] {
            assert!(!allow.contains(&rule), "{profile} allows {rule}");
        }
        for rule in ["Bash(fd *)", "Bash(rg *)", "Bash(cat *)", "Bash(for *)"] {
            assert!(allow.contains(&rule), "{profile} lacks {rule}");
        }
    }
}

#[test]
fn a_colocated_repo_allows_the_git_read_verbs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("[requires]\njj = \"*\"\n")).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let allow = strings(&settings["permissions"]["allow"]);
        for verb in ["log", "show", "diff", "status", "blame"] {
            let rule = format!("Bash(git {verb} *)");
            assert!(allow.contains(&rule.as_str()), "{profile} allows {rule}");
        }
    }
}

#[test]
fn no_writing_git_verb_is_allowed_and_without_jj_no_git_at_all() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("[requires]\njj = \"*\"\n")).expect("generates");
    let plain = dir.path().join("plain").join(".zaf");
    init_here(&plain, &manifest("")).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        for rule in strings(&settings["permissions"]["allow"]) {
            if let Some(verb) = rule.strip_prefix("Bash(git ") {
                let verb = verb.split([' ', ')']).next().expect("a verb");
                assert!(
                    ["log", "show", "diff", "status", "blame"].contains(&verb),
                    "{profile} allows the writing verb {rule}"
                );
            }
        }
        let allow = strings(&settings["permissions"]["allow"]);
        for verb in [
            "push", "commit", "checkout", "reset", "add", "rebase", "merge", "switch", "restore",
            "clean", "stash", "tag", "branch", "fetch", "pull",
        ] {
            let rule = format!("Bash(git {verb} *)");
            assert!(!allow.contains(&rule.as_str()), "{profile} allows {rule}");
        }
        assert!(
            !allow.contains(&"Bash(git *)"),
            "{profile} allows all of git"
        );
        let plain_settings = self::settings(&plain, profile);
        assert!(
            !strings(&plain_settings["permissions"]["allow"])
                .iter()
                .any(|rule| rule.starts_with("Bash(git")),
            "{profile}: no jj, no git"
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
        with_read_only(&["Bash(zaf *)", "Bash(make check)", "Bash(make *)"], false)
    );
}

#[test]
fn a_chained_gate_adds_exactly_one_rule_for_its_one_tool() {
    let gate = "cd zaf && cargo fmt --check && cargo build && cargo clippy --all-targets -- -D warnings && cargo test";
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest(&format!("[gate]\nfull = \"{gate}\"\n"))).expect("generates");
    let verbatim = format!("Bash({gate})");
    for profile in ["lean", "build"] {
        assert_eq!(
            strings(&settings(&state, profile)["permissions"]["allow"]),
            with_read_only(&["Bash(zaf *)", verbatim.as_str(), "Bash(cargo *)"], false),
            "{profile}"
        );
    }
}

#[test]
fn a_gate_and_its_tests_allow_each_tool_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(
        &state,
        &manifest("[gate]\nfull = \"just gate\"\ntests = \"cargo test -p {crate} {name}\"\n"),
    )
    .expect("generates");
    for profile in ["lean", "build"] {
        assert_eq!(
            strings(&settings(&state, profile)["permissions"]["allow"]),
            with_read_only(
                &[
                    "Bash(zaf *)",
                    "Bash(just gate)",
                    "Bash(just *)",
                    "Bash(cargo test -p * *)",
                    "Bash(cargo *)"
                ],
                false
            ),
            "{profile}"
        );
    }
}

#[test]
fn tools_are_found_across_every_separator_and_builtins_are_skipped() {
    assert_eq!(
        tool_rules(&CommandLine::from("cd a; b | c || d && FOO=1 e")),
        ["Bash(b *)", "Bash(c *)", "Bash(d *)", "Bash(e *)"]
    );
}

#[test]
fn shell_tools_get_no_wildcard_rule() {
    for line in [
        "sh scripts/zaf-tests.sh {name}",
        "bash -c make",
        "zsh x",
        "dash x",
        "env X=1 b",
        "FOO=1 sh x",
        "xargs b",
        "nohup b",
        "timeout 5 b",
        "time b",
        "nice b",
        "sudo b",
        "doas b",
        "/bin/sh x",
        "cd a && sh x | bash y",
    ] {
        assert_eq!(
            tool_rules(&CommandLine::from(line)),
            Vec::<String>::new(),
            "{line}"
        );
    }
    assert_eq!(
        tool_rules(&CommandLine::from("cd a && sh x && cargo test")),
        ["Bash(cargo *)"]
    );
}

#[test]
fn a_shell_gate_command_is_allowed_exactly_and_never_as_a_wildcard_shell() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(
        &state,
        &manifest("[gate]\nfull = \"make check\"\ntests = \"sh scripts/zaf-tests.sh {name}\"\n"),
    )
    .expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let allow = strings(&settings["permissions"]["allow"]);
        assert!(
            allow.contains(&"Bash(sh scripts/zaf-tests.sh *)"),
            "{profile}"
        );
        assert!(!allow.contains(&"Bash(sh *)"), "{profile}");
    }
}

#[test]
fn a_required_shell_tool_is_still_allowed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, &manifest("[requires]\nsh = \"*\"\n")).expect("generates");
    let settings = settings(&state, "build");
    assert!(strings(&settings["permissions"]["allow"]).contains(&"Bash(sh *)"));
}

#[test]
fn a_forbidden_tool_is_not_allowed_by_the_gate_rules() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(
        &state,
        &manifest("[gate]\nfull = \"cd x && gh pr\"\n[forbidden]\ncommands = [\"gh\"]\n"),
    )
    .expect("generates");
    let settings = settings(&state, "build");
    let allow = strings(&settings["permissions"]["allow"]);
    assert!(!allow.contains(&"Bash(gh *)"));
    assert!(allow.contains(&"Bash(cd x && gh pr)"));
}

/// The project root's marketplace file: marketplace `shelf` listing plugin `kit` at `source`.
fn write_marketplace(root: &Path, source: &str) {
    fs::create_dir_all(root.join(".claude-plugin")).expect("marketplace dir");
    let catalog = json!({
        "name": "shelf",
        "plugins": [
            { "name": "other", "source": "./plugins/other" },
            { "name": "kit", "source": source },
        ],
    });
    fs::write(root.join(MARKETPLACE_FILE), catalog.to_string()).expect("writes");
}

/// `init` for a project rooted at the state folder's parent, through `shell`.
fn init_through(
    state: &Path,
    manifest: &Manifest,
    shell: &RecordingShell,
) -> Result<Report, ProfileError> {
    let root = state.parent().expect("a parent");
    init_with(root, state, manifest, shell, None)
}

const PATH_PACK: &str = "[pack]\npath = \"vendor/kit\"\n";

#[test]
fn a_pack_from_a_path_is_registered_through_claude_under_the_profile_home() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit/");
    let shell = RecordingShell::listing(NO_PLUGINS, NO_MARKETPLACES);
    let report = init_through(&state, &manifest(PATH_PACK), &shell).expect("generates");
    let home = state.join("profiles/build").display().to_string();
    let env = format!("CLAUDE_CONFIG_DIR='{home}' claude plugin");
    assert_eq!(
        shell.lines(),
        [
            format!("{env} list"),
            format!("{env} marketplace list"),
            format!("{env} marketplace add '{}'", dir.path().display()),
            format!("{env} install 'kit@shelf'"),
        ],
        "only the profile that carries plugins (build) is registered, lean runs plugin-free"
    );
    assert!(!report.unchanged());
    assert!(
        report
            .lines()
            .contains(&"installed kit@shelf into profiles/build".to_owned())
    );
    assert!(
        !state.join("profiles/build/plugins/kit").exists(),
        "no raw copy of the pack"
    );
    let build = settings(&state, "build");
    assert_eq!(build["enabledPlugins"], json!({ "kit@shelf": true }));
    assert!(settings(&state, "lean").get("enabledPlugins").is_none());
}

#[test]
fn a_rerun_whose_plugin_list_already_shows_the_pack_installs_nothing_and_changes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    let shell = RecordingShell::listing(KIT_LISTED, SHELF_ADDED);
    let manifest = manifest(PATH_PACK);
    init_through(&state, &manifest, &shell).expect("first");
    let second = init_through(&state, &manifest, &shell).expect("again");
    assert!(second.unchanged(), "{:?}", second.lines());
    assert_eq!(second.lines(), ["nothing changed"]);
    assert!(
        shell
            .lines()
            .iter()
            .all(|line| line.ends_with("claude plugin list")),
        "only the listing is asked: {:?}",
        shell.lines()
    );
}

/// The pack's own manifest at `vendor/kit`, declaring `version`.
fn write_plugin(root: &Path, version: &str) {
    let dir = root.join("vendor/kit/.claude-plugin");
    fs::create_dir_all(&dir).expect("plugin dir");
    let plugin = json!({ "name": "kit", "version": version });
    fs::write(dir.join("plugin.json"), plugin.to_string()).expect("writes");
}

#[test]
fn profile_init_updates_a_changed_pack() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    write_plugin(dir.path(), "1.3.0");
    // The profile holds 1.2.3 (KIT_LISTED); the pack now declares 1.3.0.
    let shell = RecordingShell::listing(KIT_LISTED, SHELF_ADDED);
    let report = init_through(&state, &manifest(PATH_PACK), &shell).expect("updates");
    let home = state.join("profiles/build").display().to_string();
    let env = format!("CLAUDE_CONFIG_DIR='{home}' claude plugin");
    assert_eq!(
        shell.lines(),
        [
            format!("{env} list"),
            format!("{env} marketplace update 'shelf'"),
            format!("{env} update 'kit@shelf'"),
        ],
        "only the profile that carries plugins is updated, and nothing is installed again"
    );
    assert!(
        report
            .lines()
            .contains(&"updated kit@shelf to 1.3.0 in profiles/build".to_owned()),
        "{:?}",
        report.lines()
    );
    assert!(!report.unchanged());
}

#[test]
fn a_rerun_with_the_same_pack_version_issues_no_plugin_command() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    write_plugin(dir.path(), "1.2.3");
    let shell = RecordingShell::listing(KIT_LISTED, SHELF_ADDED);
    let report = init_through(&state, &manifest(PATH_PACK), &shell).expect("generates");
    assert!(
        shell
            .lines()
            .iter()
            .all(|line| line.ends_with("claude plugin list")),
        "only the listing is asked: {:?}",
        shell.lines()
    );
    assert!(report.installed.is_empty());
}

#[test]
fn a_failed_pack_update_is_an_error_naming_the_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    write_plugin(dir.path(), "2.0.0");
    let shell = FailingUpdate(RecordingShell::listing(KIT_LISTED, SHELF_ADDED));
    let root = state.parent().expect("a parent");
    let result = init_with(root, &state, &manifest(PATH_PACK), &shell, None);
    let Err(ProfileError::Install { line, .. }) = result else {
        panic!("an install error");
    };
    assert!(line.ends_with("marketplace update 'shelf'"), "{line}");
}

/// A shell that answers like [`RecordingShell`] but fails every `update`.
struct FailingUpdate(RecordingShell);

impl Shell for FailingUpdate {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        let mut ran = self.0.run(line)?;
        if line.as_ref().contains(" update ") {
            ran.verdict = GateVerdict::Failed;
        }
        Ok(ran)
    }
}

#[test]
fn a_marketplace_already_added_is_not_added_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    let shell = RecordingShell::listing(NO_PLUGINS, SHELF_ADDED);
    init_through(&state, &manifest(PATH_PACK), &shell).expect("generates");
    let lines = shell.lines();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].ends_with("claude plugin install 'kit@shelf'"));
    assert!(lines.iter().all(|line| !line.contains("marketplace add")));
}

#[test]
fn a_pack_path_the_marketplace_does_not_list_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./plugins/elsewhere");
    let shell = RecordingShell::answering(GateVerdict::Passed);
    let result = init_through(&state, &manifest(PATH_PACK), &shell);
    let Err(ProfileError::Marketplace { reason, .. }) = result else {
        panic!("a marketplace error");
    };
    assert!(reason.contains("vendor/kit"), "{reason}");
    assert!(shell.lines().is_empty());
}

#[test]
fn a_missing_marketplace_file_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let shell = RecordingShell::answering(GateVerdict::Passed);
    let result = init_through(&state, &manifest(PATH_PACK), &shell);
    assert!(matches!(result, Err(ProfileError::Marketplace { .. })));
}

#[test]
fn a_failed_claude_command_is_an_error_naming_the_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    let shell = RecordingShell::answering(GateVerdict::Failed);
    let result = init_through(&state, &manifest(PATH_PACK), &shell);
    let Err(ProfileError::Install { line, output }) = result else {
        panic!("an install error");
    };
    assert!(line.ends_with("claude plugin list"), "{line}");
    assert_eq!(output, NO_PLUGINS.trim());
}

#[test]
fn the_listing_gives_the_version_of_the_asked_plugin_only() {
    assert_eq!(
        listed_version(KIT_LISTED, "kit@shelf"),
        Some("1.2.3".to_owned())
    );
    assert_eq!(
        listed_version(KIT_LISTED, "other@shelf"),
        Some("9.9.9".to_owned())
    );
    assert_eq!(listed_version(KIT_LISTED, "kit@elsewhere"), None);
    assert_eq!(listed_version(NO_PLUGINS, "kit@shelf"), None);
    assert_eq!(
        listed_version("  kit@shelf\n    Scope: user\n", "kit@shelf"),
        Some("unknown".to_owned()),
        "listed without a version"
    );
    assert_eq!(
        listed_version("kit@shelf\n\nVersion: 3.0.0\n", "kit@shelf"),
        Some("unknown".to_owned()),
        "a blank line ends the entry"
    );
}

#[test]
fn a_marketplace_is_listed_by_its_own_word() {
    assert!(lists_marketplace(SHELF_ADDED, "shelf"));
    assert!(!lists_marketplace(SHELF_ADDED, "shel"));
    assert!(!lists_marketplace(NO_MARKETPLACES, "shelf"));
}

#[test]
fn settings_enable_the_pack_and_never_wait_on_a_prompt() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    write_marketplace(dir.path(), "./vendor/kit");
    init_through(
        &state,
        &manifest(&format!(
            "{PATH_PACK}[forbidden]\ncommands = [\"git push\"]\n"
        )),
        &RecordingShell::listing(KIT_LISTED, SHELF_ADDED),
    )
    .expect("generates");
    let build = settings(&state, "build");
    assert_eq!(build["enabledPlugins"], json!({ "kit@shelf": true }));
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
    let report = init_through(&state, &manifest, &shell).expect("generates");
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
    let again = init_through(&state, &manifest, &shell).expect("again");
    assert_eq!(shell.lines().len(), 1, "the same tag installs nothing");
    assert!(again.unchanged());
    let newer = manifest_with_tag("kit--v1.1.0");
    init_through(&state, &newer, &shell).expect("a new tag");
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
    let result = init_through(&state, &manifest_with_tag("kit--v1.0.0"), &shell);
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

/// A Claude home in `dir`; logged in when `login` is given.
fn a_claude_home(dir: &Path, login: Option<&str>) -> PathBuf {
    let claude_home = dir.join("claude-home");
    fs::create_dir(&claude_home).expect("claude home");
    if let Some(login) = login {
        fs::write(claude_home.join(CREDENTIALS_FILE), login).expect("credentials");
    }
    claude_home
}

/// `init` with no pack, against `claude_home`.
fn init_against(state: &Path, claude_home: &Path) -> Report {
    let shell = RecordingShell::answering(GateVerdict::Failed);
    let root = state.parent().expect("a parent");
    init_with(root, state, &manifest(""), &shell, Some(claude_home)).expect("generates")
}

#[test]
fn profile_init_links_the_home_credentials() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let claude_home = a_claude_home(dir.path(), Some("old"));
    let source = claude_home.join(CREDENTIALS_FILE);
    let report = init_against(&state, &claude_home);
    for profile in ["lean", "build"] {
        let relative = PathBuf::from("profiles")
            .join(profile)
            .join(CREDENTIALS_FILE);
        let link = state.join(&relative);
        assert_eq!(
            fs::read_link(&link).expect("a symlink"),
            source,
            "{profile}"
        );
        assert!(
            report.written.contains(&Written {
                path: relative,
                change: Change::Created
            }),
            "{:?}",
            report.written
        );
        assert!(links_to_claude_home(&link, &claude_home), "{profile}");
    }
    fs::write(
        state.join("profiles/lean").join(CREDENTIALS_FILE),
        "rotated",
    )
    .expect("writes");
    assert_eq!(
        fs::read_to_string(state.join("profiles/build").join(CREDENTIALS_FILE)).expect("reads"),
        "rotated",
        "a token one profile rotates reaches every other profile"
    );
    assert_eq!(fs::read_to_string(&source).expect("reads"), "rotated");
    let again = init_against(&state, &claude_home);
    assert!(again.unchanged(), "{:?}", again.lines());
}

#[test]
fn profile_init_writes_no_credentials_when_the_home_has_none() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let claude_home = a_claude_home(dir.path(), None);
    let report = init_against(&state, &claude_home);
    assert_eq!(report.written.len(), 4, "{:?}", report.written);
    let link = state.join("profiles/build").join(CREDENTIALS_FILE);
    assert!(fs::symlink_metadata(&link).is_err());

    symlink(
        dir.path().join("nowhere"),
        claude_home.join(CREDENTIALS_FILE),
    )
    .expect("a dangling link");
    init_against(&state, &claude_home);
    assert!(
        fs::symlink_metadata(&link).is_err(),
        "a dangling link in the home is not its file"
    );
}

#[test]
fn profile_init_leaves_what_is_already_at_the_path_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let claude_home = a_claude_home(dir.path(), Some("live"));
    let build = state.join("profiles/build");
    let lean = state.join("profiles/lean");
    fs::create_dir_all(&build).expect("build");
    fs::create_dir_all(&lean).expect("lean");
    let copy = build.join(CREDENTIALS_FILE);
    fs::write(&copy, "my copy").expect("a copy");
    let elsewhere = dir.path().join("elsewhere.json");
    let foreign = lean.join(CREDENTIALS_FILE);
    symlink(&elsewhere, &foreign).expect("a dangling link elsewhere");

    let report = init_against(&state, &claude_home);

    assert!(
        report
            .written
            .iter()
            .all(|file| !file.path.ends_with(CREDENTIALS_FILE)),
        "{:?}",
        report.written
    );
    let kind = fs::symlink_metadata(&copy).expect("there").file_type();
    assert!(kind.is_file(), "the copy stays a regular file");
    assert_eq!(fs::read_to_string(&copy).expect("reads"), "my copy");
    assert_eq!(fs::read_link(&foreign).expect("a link"), elsewhere);
    assert!(!links_to_claude_home(&foreign, &claude_home));
}

/// Whether the permission pattern `pattern` matches `line`: `*` matches any text, spaces included.
fn pattern_matches(pattern: &str, line: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, rest) = parts.split_first().expect("a split has a part");
    let Some(mut tail) = line.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return tail.is_empty();
    };
    for part in middle {
        match tail.find(part) {
            Some(at) => tail = &tail[at + part.len()..],
            None => return false,
        }
    }
    tail.ends_with(last) && tail.len() >= last.len()
}

#[test]
fn forbidden_denies_match_flags_before_the_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    let manifest = manifest(
        "[forbidden]\ncommands = [\"gh pr merge\", \"zaf do pr-merge\", \"zaf do land\", \
         \"zaf do --tier machine land\"]\n",
    );
    init_here(&state, &manifest).expect("generates");
    for profile in ["lean", "build"] {
        let settings = settings(&state, profile);
        let deny = strings(&settings["permissions"]["deny"]);
        let denied = |line: &str| {
            deny.iter().any(|rule| {
                let pattern = rule
                    .strip_prefix("Bash(")
                    .and_then(|rest| rest.strip_suffix(')'))
                    .expect("a Bash rule");
                pattern_matches(pattern, line)
            })
        };
        for line in [
            "zaf do land",
            "zaf do land 12",
            "zaf do pr-merge 12",
            "zaf --more do land 12",
            "zaf --json do pr-merge 12",
            "zaf do --json pr-merge 12",
            "zaf do --tier=machine land 12",
            "zaf do --tier machine land 12",
            "zaf --more do --tier machine pr-merge 12",
            "gh pr merge 12",
        ] {
            assert!(denied(line), "{profile} lets `{line}` through: {deny:?}");
        }
        // What is not forbidden stays allowed: other actions, other verbs, the merge actions.
        for line in [
            "zaf do slice-merge 12",
            "zaf --more do slice-merge 12",
            "zaf do release",
            "zaf do --tier machine release",
            "zaf do landing",
            "zaf board ready",
            "zaf --more check 12",
        ] {
            assert!(!denied(line), "{profile} denies `{line}`: {deny:?}");
        }
    }
}

/// The deny patterns of `manifest`'s generated profiles, per profile.
fn denies_of(manifest: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    init_here(&state, manifest).expect("generates");
    ["lean", "build"]
        .into_iter()
        .map(|profile| {
            let settings = settings(&state, profile);
            let patterns = strings(&settings["permissions"]["deny"])
                .into_iter()
                .map(|rule| {
                    rule.strip_prefix("Bash(")
                        .and_then(|rest| rest.strip_suffix(')'))
                        .expect("a Bash rule")
                        .to_owned()
                })
                .collect();
            (profile, patterns)
        })
        .collect()
}

/// Asserts every line matches a deny in every profile, naming the profile and line when not.
fn assert_denied(manifest: &Manifest, lines: &[&str]) {
    for (profile, patterns) in denies_of(manifest) {
        for line in lines {
            assert!(
                patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, line)),
                "{profile} lets `{line}` through: {patterns:?}"
            );
        }
    }
}

#[test]
fn forbidden_entries_get_flag_forms() {
    let forbidding = manifest("[forbidden]\ncommands = [\"jj git push\", \"gh pr merge\"]\n");
    for (profile, patterns) in denies_of(&forbidding) {
        for wanted in [
            "jj git push",
            "jj git push *",
            "jj -* git push",
            "jj -* git push *",
            "gh pr merge",
            "gh pr merge *",
            "gh -* pr merge",
            "gh -* pr merge *",
        ] {
            assert!(patterns.iter().any(|p| p == wanted), "{profile}: {wanted}");
        }
    }
    assert_denied(
        &forbidding,
        &[
            "jj git push",
            "jj git push --bookmark x",
            "jj --no-pager git push",
            "jj --color=never git push",
            "jj --no-pager git push -b x",
            "gh -R owner/repo pr merge 12",
            "gh pr merge 12",
        ],
    );
    // A tool alone is denied by its bare and with-arguments forms; it gets no flag form.
    let whole = manifest("[forbidden]\ncommands = [\"rm\"]\n");
    for (_, patterns) in denies_of(&whole) {
        assert_eq!(patterns, ["rm *", "rm"]);
    }
}

#[test]
fn required_jj_denies_its_hatches() {
    let jj = manifest("[requires]\njj = \"*\"\n");
    for (profile, patterns) in denies_of(&jj) {
        for sub in ["util", "config set", "config edit"] {
            for wanted in [
                format!("jj {sub}"),
                format!("jj {sub} *"),
                format!("jj -* {sub}"),
                format!("jj -* {sub} *"),
            ] {
                assert!(patterns.contains(&wanted), "{profile}: {wanted}");
            }
        }
    }
    assert_denied(
        &jj,
        &[
            "jj util exec -- sh x.sh",
            "jj --no-pager util exec -- sh x.sh",
            "jj config set --user x y",
            "jj config edit --user",
            "jj --config aliases.x=x x",
            "jj --config=aliases.x=x x",
            "jj log --config-file x.toml",
            "jj log --config aliases.x=x",
        ],
    );
    // The rest of jj stays out of the denies.
    for (profile, patterns) in denies_of(&jj) {
        for line in [
            "jj log",
            "jj --no-pager log -r x",
            "jj config list",
            "jj diff",
        ] {
            assert!(
                !patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, line)),
                "{profile} denies `{line}`: {patterns:?}"
            );
        }
    }
}

#[test]
fn required_gh_denies_its_hatches() {
    let gh = manifest("[requires]\ngh = \"*\"\n");
    for (profile, patterns) in denies_of(&gh) {
        for sub in ["alias", "extension"] {
            for wanted in [
                format!("gh {sub}"),
                format!("gh {sub} *"),
                format!("gh -* {sub}"),
                format!("gh -* {sub} *"),
            ] {
                assert!(patterns.contains(&wanted), "{profile}: {wanted}");
            }
        }
    }
    assert_denied(
        &gh,
        &[
            "gh alias set --shell x sh",
            "gh extension install o/r",
            "gh -R o/r alias list",
            "gh --repo o/r extension list",
        ],
    );
    for (profile, patterns) in denies_of(&gh) {
        for line in ["gh pr view 12", "gh -R o/r issue list", "gh api repos/o/r"] {
            assert!(
                !patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, line)),
                "{profile} denies `{line}`: {patterns:?}"
            );
        }
    }
}

#[test]
fn required_yarn_denies_its_hatches() {
    let yarn = manifest("[requires]\nyarn = \"*\"\n");
    for (profile, patterns) in denies_of(&yarn) {
        for sub in ["exec", "node", "dlx"] {
            for wanted in [
                format!("yarn {sub}"),
                format!("yarn {sub} *"),
                format!("yarn -* {sub}"),
                format!("yarn -* {sub} *"),
            ] {
                assert!(patterns.contains(&wanted), "{profile}: {wanted}");
            }
        }
    }
    assert_denied(
        &yarn,
        &[
            "yarn dlx x",
            "yarn exec sh",
            "yarn node x.js",
            "yarn --cwd web dlx x",
        ],
    );
    for (profile, patterns) in denies_of(&yarn) {
        for line in ["yarn install", "yarn test", "yarn --cwd web build"] {
            assert!(
                !patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, line)),
                "{profile} denies `{line}`: {patterns:?}"
            );
        }
    }
    // A gate line that runs yarn allows it, so its hatches are denied too.
    let gated = manifest("[gate]\nfull = \"cd web && yarn test\"\n");
    assert_denied(&gated, &["yarn dlx x", "yarn --cwd web exec sh"]);
}

#[test]
fn a_tool_not_allowed_adds_no_hatch() {
    let without_yarn = manifest("[requires]\njj = \"*\"\ngh = \"*\"\n");
    for (profile, patterns) in denies_of(&without_yarn) {
        assert!(
            !patterns.iter().any(|pattern| pattern.starts_with("yarn")),
            "{profile} denies yarn: {patterns:?}"
        );
    }
    let bare = manifest("");
    for (profile, patterns) in denies_of(&bare) {
        assert!(patterns.is_empty(), "{profile} denies {patterns:?}");
    }
    // Forbidding a command does not allow its tool, so it brings no hatch either.
    let forbidding = manifest("[forbidden]\ncommands = [\"yarn publish\"]\n");
    for (profile, patterns) in denies_of(&forbidding) {
        assert!(
            !patterns.iter().any(|pattern| pattern.contains("dlx")),
            "{profile}: {patterns:?}"
        );
    }
}

#[test]
fn a_forbidden_line_names_its_action_whatever_its_flags() {
    for (line, name) in [
        ("zaf do land", Some("land")),
        ("zaf do --tier machine land", Some("land")),
        ("zaf do --tier=machine land", Some("land")),
        ("zaf --more do land 12", Some("land")),
        ("gh pr merge", None),
        ("zaf do", None),
        ("zaf board ready", None),
        ("zaf do --tier machine", None),
    ] {
        assert_eq!(forbidden_action(line), name, "{line}");
    }
}

#[test]
fn the_claude_home_is_only_ever_under_an_absolute_home() {
    assert_eq!(
        claude_home_under(Some(OsString::from("/home/someone"))),
        Some(PathBuf::from("/home/someone/.claude"))
    );
    assert_eq!(claude_home_under(None), None);
    assert_eq!(claude_home_under(Some(OsString::new())), None);
    assert_eq!(claude_home_under(Some(OsString::from("relative"))), None);
}

#[test]
fn a_link_through_another_spelling_of_the_home_still_leads_to_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let claude_home = a_claude_home(dir.path(), Some("live"));
    let alias = dir.path().join("alias");
    symlink(&claude_home, &alias).expect("an alias of the home");
    let link = dir.path().join(CREDENTIALS_FILE);
    symlink(alias.join(CREDENTIALS_FILE), &link).expect("a link");
    assert!(links_to_claude_home(&link, &claude_home));

    let other = tempfile::tempdir().expect("temp dir");
    let logged_out = a_claude_home(other.path(), None);
    let waiting = other.path().join(CREDENTIALS_FILE);
    symlink(logged_out.join(CREDENTIALS_FILE), &waiting).expect("a link");
    assert!(
        links_to_claude_home(&waiting, &logged_out),
        "the home's file is the target even while the home is logged out"
    );
}
