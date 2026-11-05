//! A `[gate]` line may name an action (docs/zaf-actions.md §4): `light = "zaf do gate-light"`
//! and `tests = "zaf do test {name}"` run as any line runs, in the task's workspace, so zaf
//! ships no gate and wraps no port. Each test runs `zaf check` on the built binary against a
//! workspace that holds the project's actions, with `zaf` itself first on PATH.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use common::{Project, outcome, write};
use zaf::app::profile;
use zaf::manifest::{CommandLine, Manifest};
use zaf::ports::{GateRun, PortError, Shell};

const HEAD: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const ACTION_GATE: &str = r#"
[gate]
full = "true"
light = "zaf do gate-light"
tests = "zaf do test {name}"
"#;

const LITERAL_GATE: &str = r#"
[gate]
full = "true"
light = "test -f marker"
tests = "test -f {name}"
"#;

/// Task 2 of epic 1; `tests` is the `Tests:` field of its ticket.
fn ticket(tests: &str) -> String {
    let body = format!(
        "Goal: gate the slice.\nOwned paths: src/lib.rs\nInterfaces: none\nDesign pages: none\n\
         Architecture: none\nAcceptance:\n  - the gate runs\nTests: {tests}\n\
         Security trigger: no\nModel: sonnet\nSize: S\nBlocked by: —\n"
    );
    serde_json::json!({
        "number": 2,
        "title": "Gate by action",
        "body": body,
        "labels": [{"name": "task"}, {"name": "claimed"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [],
    })
    .to_string()
}

/// A project and the workspace its slice record names. The workspace holds the manifest and
/// the project's actions, as a checkout of the repository does.
struct Gated {
    project: Project,
    workspace: PathBuf,
}

impl Gated {
    fn new(gate: &str, tests: &str) -> Self {
        let manifest = format!("{HEAD}{gate}");
        let mut project = Project::new(&manifest);
        let workspace = project.temp.path().join("workspace");
        write(&workspace, ".zaf.toml", &manifest);
        write(&project.root, ".zaf/slices/2", &workspace.to_string_lossy());
        // `zaf` first on PATH, so a gate line can say `zaf do`; the fake gh stays reachable.
        let bin = Path::new(env!("CARGO_BIN_EXE_zaf"))
            .parent()
            .expect("the binary's folder");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            [bin.to_owned(), fixtures]
                .into_iter()
                .chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        project
            .on("issue view 2 *", &ticket(tests))
            .env("PATH", &search_path.to_string_lossy());
        Self { project, workspace }
    }

    /// The project's action `name` in the workspace.
    fn action(&self, name: &str, body: &str) -> &Self {
        executable(&self.workspace.join(".zaf/actions"), name, body);
        self
    }

    /// The machine's action `name`, behind the project's tier.
    fn machine_action(&self, name: &str, body: &str) -> &Self {
        executable(&self.project.home.join("actions"), name, body);
        self
    }

    fn check(&self) -> Output {
        self.project.zaf(&["check", "2"])
    }

    /// The gate's logs, the whole output of each run of the light gate.
    fn gate_logs(&self) -> String {
        let runs = self.workspace.join(".zaf/runs");
        let mut logs: Vec<PathBuf> = fs::read_dir(&runs)
            .map(|entries| {
                entries
                    .filter_map(|entry| Some(entry.ok()?.path()))
                    .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
                    .collect()
            })
            .unwrap_or_default();
        logs.sort();
        logs.iter()
            .map(|log| fs::read_to_string(log).expect("a gate log"))
            .collect()
    }
}

/// An action file: the metadata zaf wants, then `body`.
fn executable(dir: &Path, name: &str, body: &str) {
    let text =
        format!("#!/bin/sh\n# zaf description: the {name} action\n# zaf 1: it failed\n{body}\n");
    write(dir, name, &text);
    fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn said(output: &Output) -> String {
    let (_, stdout, stderr) = outcome(output);
    format!("{stdout}{stderr}")
}

#[test]
fn literal_gate_is_unchanged() {
    let gated = Gated::new(LITERAL_GATE, "marker");
    let run = gated.check();
    let (code, stdout, _) = outcome(&run);
    assert_eq!(code, 1, "no marker, the literal gate fails: {}", said(&run));
    assert!(stdout.contains("gate"), "{stdout}");

    write(&gated.workspace, "marker", "here\n");
    let run = gated.check();
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(
        !gated.gate_logs().contains("zaf do"),
        "a literal line goes through no action"
    );
    assert!(
        !gated.workspace.join(".zaf/logs").exists(),
        "a literal line writes no action log"
    );
}

#[test]
fn gate_line_runs_a_project_action() {
    let gated = Gated::new(ACTION_GATE, "none");
    gated.action("gate-light", "pwd > ran-here\nexit 0");
    let run = gated.check();
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let here = fs::read_to_string(gated.workspace.join("ran-here")).expect("the action ran");
    let workspace = gated.workspace.canonicalize().expect("canonical");
    assert_eq!(
        Path::new(here.trim()).canonicalize().expect("canonical"),
        workspace,
        "the action runs in the workspace"
    );
}

#[test]
fn failed_gate_action_fails_the_gate() {
    let gated = Gated::new(ACTION_GATE, "none");
    gated.action("gate-light", "echo lint broke\nexit 1");
    let run = gated.check();
    let (code, stdout, _) = outcome(&run);
    assert_eq!(code, 1, "{}", said(&run));
    assert!(stdout.contains("gate"), "{stdout}");

    // Any non-zero code fails, declared or not.
    let output = gated.gate_logs();
    assert!(output.contains("FAIL gate-light: exit 1"), "{output}");
    let log = output
        .lines()
        .find_map(|line| line.rsplit_once(", log "))
        .map(|(_, path)| PathBuf::from(path.trim()))
        .unwrap_or_else(|| panic!("the log path is in the gate output: {output}"));
    let kept = fs::read_to_string(&log).expect("the action's own log exists");
    assert!(kept.contains("lint broke"), "{kept}");

    gated.action("gate-light", "exit 7");
    let run = gated.check();
    assert_eq!(outcome(&run).0, 1, "{}", said(&run));
    assert!(gated.gate_logs().contains("exit 7, undeclared"));
}

#[test]
fn tests_line_passes_the_name() {
    let gated = Gated::new(ACTION_GATE, "`alpha_test`, `beta_test`");
    gated
        .action("gate-light", "exit 0")
        .action("test", "echo \"$#:$1\" >> test-names\n[ \"$1\" != bad ]");
    let run = gated.check();
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let names = fs::read_to_string(gated.workspace.join("test-names")).expect("the action ran");
    assert_eq!(names, "1:alpha_test\n1:beta_test\n");

    // The name is the action's argument, so the action decides which test fails.
    let gated = Gated::new(ACTION_GATE, "`bad`");
    gated
        .action("gate-light", "exit 0")
        .action("test", "[ \"$1\" != bad ]");
    let run = gated.check();
    let (code, stdout, _) = outcome(&run);
    assert_eq!(code, 1, "{}", said(&run));
    assert!(stdout.contains("bad"), "{stdout}");
}

#[test]
fn machine_action_runs_from_a_gate_line() {
    let gated = Gated::new(ACTION_GATE, "`alpha_test`");
    gated
        .machine_action("gate-light", "echo machine > ran-machine\nexit 0")
        .machine_action("test", "echo \"$1\" > machine-test\nexit 0");
    let run = gated.check();
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let ran = fs::read_to_string(gated.workspace.join("ran-machine")).expect("the machine's file");
    assert_eq!(ran, "machine\n");
    let test = fs::read_to_string(gated.workspace.join("machine-test")).expect("the test ran");
    assert_eq!(test, "alpha_test\n");
}

#[test]
fn missing_action_fails_the_gate() {
    let gated = Gated::new(ACTION_GATE, "`alpha_test`");
    let run = gated.check();
    let (code, stdout, _) = outcome(&run);
    assert_eq!(code, 1, "{}", said(&run));
    assert!(stdout.contains("gate"), "{stdout}");
    let output = gated.gate_logs();
    assert!(
        output.contains("NOT_RUN gate-light"),
        "`zaf do` speaks, not the gate: {output}"
    );
    assert!(
        stdout.contains("NOT_RUN test"),
        "the test line fails the same way: {stdout}"
    );
}

#[test]
fn gate_line_calling_itself_stops() {
    let gated = Gated::new(ACTION_GATE, "none");
    gated.action("gate-light", "zaf do gate-light\nexit $?");
    let began = Instant::now();
    let run = gated.check();
    assert!(
        began.elapsed() < Duration::from_secs(60),
        "a self-calling gate stops, it does not hang"
    );
    assert_eq!(outcome(&run).0, 1, "{}", said(&run));
    let logs: String = fs::read_dir(gated.workspace.join(".zaf/logs"))
        .expect("action logs")
        .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
        .collect();
    assert!(
        logs.contains("NOT_RUN gate-light: depth 8 reached"),
        "{logs}"
    );
}

/// A shell profile generation must never use: with no `[pack]` it runs no command.
struct NoShell;

impl Shell for NoShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        panic!("profile generation ran a command: {line}");
    }
}

/// Whether a permission rule `Bash(<pattern>)` matches `command`: a trailing ` *` is a prefix.
fn matches(rule: &str, command: &str) -> bool {
    let Some(pattern) = rule
        .strip_prefix("Bash(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    match pattern.strip_suffix(" *") {
        Some(prefix) => command == prefix || command.starts_with(&format!("{prefix} ")),
        None => command == pattern,
    }
}

#[test]
fn gate_line_is_allowed_for_a_programmer() {
    // The ways repository's own forbidden merge paths ride along: the allow rule must hold
    // beside them, and none of them may swallow a gate line.
    let manifest = format!(
        "{HEAD}{ACTION_GATE}\n[roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\n\
         profile = \"lean\"\nturns = 60\n\n[forbidden]\ncommands = [\"gh pr merge\", \
         \"zaf do pr-merge\", \"zaf do land\", \"zaf do --tier machine pr-merge\", \
         \"zaf do --tier machine land\"]\n"
    );
    let manifest: Manifest = manifest.parse().expect("a valid manifest");
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    profile::init(dir.path(), &state, &manifest, &NoShell).expect("profiles generate");
    let text = fs::read_to_string(state.join("profiles/lean/settings.json")).expect("settings");
    let settings: serde_json::Value = serde_json::from_str(&text).expect("settings parse");
    let rules = |kind: &str| -> Vec<String> {
        settings["permissions"][kind]
            .as_array()
            .expect("a rule list")
            .iter()
            .map(|rule| rule.as_str().expect("a rule").to_owned())
            .collect()
    };
    let (allow, deny) = (rules("allow"), rules("deny"));
    assert!(!deny.is_empty(), "the forbidden merge paths are denied");
    for command in ["zaf do gate-light", "zaf do test alpha_test"] {
        assert!(
            allow.iter().any(|rule| matches(rule, command)),
            "a programmer may run `{command}`: {allow:?}"
        );
        assert!(
            !deny.iter().any(|rule| matches(rule, command)),
            "nothing forbidden covers `{command}`: {deny:?}"
        );
    }
    assert!(
        deny.iter().any(|rule| matches(rule, "zaf do pr-merge 7")),
        "the deny rules still hold: {deny:?}"
    );
}
