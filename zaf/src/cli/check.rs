//! `zaf check` and `zaf verify`: run a target's checks, print the record, store it
//! under `.zaf/runs/` for `--status`, and exit by its verdict.
//!
//! Exit codes: 0 clean, 1 must-fix present (or a task line failed), 2 a step
//! failed or timed out, or a backend is missing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::Cli;
use super::exit::{self, BACKEND, REFUSED};
use super::slice::VERSION_VAR;
use crate::adapters::gate::RUNS_DIR;
use crate::app::actions::{RunOutcome, SliceRecords};
use crate::app::check::{
    CheckRecord, Engine, EngineSettings, Shell, Step, StepPorts, Target, TaskInputs, check_task,
};
use crate::composition::{Composition, CompositionError};
use crate::domain::{IssueNumber, SecurityTrigger, TaskTemplate};
use crate::manifest::{CheckTarget, CommandLine, RunnerName};
use crate::ports::{Board, Forge, Gate, GateRun, PortError, PrNumber, Runner, Shell as ShellPort};

/// Runs the configured check steps for a target, or reads the last result.
pub fn run(cli: &Cli, target: &str, status: bool) -> u8 {
    match target.parse::<Target>() {
        Ok(target) => check(cli, target, status),
        Err(error) => {
            eprintln!("zaf {}: {error}", cli.verb.path());
            REFUSED
        }
    }
}

/// Checks a task's acceptance lines and declared interfaces (`check <task>` by another name).
pub fn verify(cli: &Cli, task: IssueNumber) -> u8 {
    check(cli, Target::Task(task), false)
}

fn check(cli: &Cli, target: Target, status: bool) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let stored = stored_path(&composition, &target);
    if status {
        return show_stored(cli, &path, &stored);
    }
    // The manifest's `[env]` for every command the check runs. A value that needs a version the
    // caller has not set runs nothing, rather than run without it.
    let version = std::env::var(VERSION_VAR)
        .ok()
        .filter(|version| !version.is_empty());
    let env = match composition.project_env(version.as_deref()) {
        Ok(env) => env,
        Err(error) => {
            eprintln!("zaf {path}: {error}; set {VERSION_VAR}");
            return BACKEND;
        }
    };
    let (record, slice_line) = match target {
        Target::Task(task) => match check_task_target(&path, &composition, &target, task, &env) {
            Ok((record, line)) => (record, Some(line)),
            Err(code) => return code,
        },
        Target::Pr { pr, task } => {
            match check_pr_target(&path, &composition, &target, pr, task, &env) {
                Ok(record) => (record, None),
                Err(code) => return code,
            }
        }
    };
    if let Err(error) = store(&stored, &record) {
        eprintln!("zaf {path}: the result was not stored for --status: {error}");
    }
    if let Some(line) = slice_line.filter(|_| !cli.json) {
        println!("{line}");
    }
    print(cli, &record);
    record.verdict.code()
}

/// Builds the gate to run inside a directory.
type GateIn<'a> = dyn Fn(&Path) -> Result<Box<dyn Gate>, CompositionError> + 'a;

/// What a task check reaches outside the process, so a test can stand in for each.
struct TaskPorts<'a> {
    /// The board, to read the ticket.
    board: &'a dyn Board,
    /// Builds the gate to run inside a directory.
    gate_in: &'a GateIn<'a>,
    /// Builds the shell that runs each named test inside a directory.
    shell_in: &'a dyn Fn(&Path) -> Box<dyn Shell>,
}

/// A port shell serving a task check's named tests.
struct DirShell(Box<dyn ShellPort>);

impl Shell for DirShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.0.run(line)
    }
}

fn check_task_target(
    path: &str,
    composition: &Composition,
    target: &Target,
    task: IssueNumber,
    env: &BTreeMap<String, String>,
) -> Result<(CheckRecord, String), u8> {
    let board = composition
        .board()
        .map_err(|error| exit::fail_composition(path, &error))?;
    check_task_with_board(path, composition, board.as_ref(), target, task, env)
}

/// [`check_task_in_slice`] with the live gate and shell: both run with `env` set.
fn check_task_with_board(
    path: &str,
    composition: &Composition,
    board: &dyn Board,
    target: &Target,
    task: IssueNumber,
    env: &BTreeMap<String, String>,
) -> Result<(CheckRecord, String), u8> {
    let ports = TaskPorts {
        board,
        gate_in: &|dir| composition.gate_in_with_env(dir, env),
        shell_in: &|dir| Box::new(DirShell(composition.shell_in_with_env(dir, env))),
    };
    check_task_in_slice(path, composition, &ports, target, task)
}

/// Checks a task inside its slice's workspace, the one the slice record names: the gate and the
/// named tests run there. A task with no record is refused and nothing runs, at the project root
/// or anywhere else: the workspace is never guessed from the branch patterns or the folder.
fn check_task_in_slice(
    path: &str,
    composition: &Composition,
    ports: &TaskPorts<'_>,
    target: &Target,
    task: IssueNumber,
) -> Result<(CheckRecord, String), u8> {
    let Some(dir) = SliceRecords::new(composition.root()).workspace(task) else {
        eprintln!("zaf {path}: {}", not_open(task));
        return Err(REFUSED);
    };
    let ticket = ports
        .board
        .task(task)
        .map_err(|error| exit::fail(path, &error))?;
    let template = ticket.body.parse::<TaskTemplate>().map_err(|error| {
        eprintln!("zaf {path}: {task} is not a task ticket: {error}");
        REFUSED
    })?;
    let gate = match (ports.gate_in)(&dir) {
        Ok(gate) => Some(gate),
        Err(CompositionError::Absent { .. }) => None,
        Err(error) => return Err(exit::fail_composition(path, &error)),
    };
    let tests = composition
        .manifest()
        .gate
        .as_ref()
        .and_then(|section| section.tests.as_ref());
    let shell = (ports.shell_in)(&dir);
    let inputs = TaskInputs {
        gate: gate.as_deref(),
        tests,
        shell: shell.as_ref(),
        root: &dir,
    };
    let began = SystemTime::now();
    let check = check_task(&template, &inputs);
    let line = format!("workspace {}", dir.display());
    Ok((CheckRecord::from_task(target, &check, began), line))
}

/// Why a task with no slice record is refused.
pub(super) fn not_open(task: IssueNumber) -> String {
    format!(
        "slice for {task} is not open; run zaf do slice-open {}",
        task.get()
    )
}

fn check_pr_target(
    path: &str,
    composition: &Composition,
    target: &Target,
    pr: PrNumber,
    task: Option<IssueNumber>,
    env: &BTreeMap<String, String>,
) -> Result<CheckRecord, u8> {
    let configured = composition
        .manifest()
        .check
        .targets
        .get(&CheckTarget::Pr)
        .map(|section| section.steps.as_slice())
        .unwrap_or_default();
    let steps = configured
        .iter()
        .map(|text| text.parse::<Step>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            eprintln!("zaf {path}: [check.pr] {error}");
            BACKEND
        })?;
    // Without the task, a `?flagged` step runs: a security review is never skipped by omission.
    let flagged = match task {
        Some(task) => {
            task_template(path, composition, task)?.security_trigger != SecurityTrigger::No
        }
        None => true,
    };
    let ports: Arc<dyn StepPorts> = Arc::new(LivePorts {
        composition: composition.clone(),
        env: env.clone(),
    });
    let mut settings = EngineSettings::new(composition.root().to_owned(), runs_dir(composition));
    settings.ci_grace = Duration::from_secs(composition.manifest().check.ci_grace_seconds);
    Ok(run_steps(ports, settings, &steps, target, pr, flagged))
}

/// Runs the steps and builds the pull request's record.
///
/// The head is read before any step starts and stored as read: a head that moves during
/// the run leaves the old one in the record, so the result does not count for the new one.
/// A forge that fails or reports no head leaves none, and nothing else changes.
fn run_steps(
    ports: Arc<dyn StepPorts>,
    settings: EngineSettings,
    steps: &[Step],
    target: &Target,
    pr: PrNumber,
    flagged: bool,
) -> CheckRecord {
    let began = SystemTime::now();
    let head = ports
        .forge()
        .and_then(|forge| forge.head(pr))
        .ok()
        .flatten();
    let report = Engine::new(ports, settings).run(steps, pr, flagged);
    CheckRecord::from_report(target, &report, head, began)
}

fn task_template(
    path: &str,
    composition: &Composition,
    task: IssueNumber,
) -> Result<TaskTemplate, u8> {
    let board = composition
        .board()
        .map_err(|error| exit::fail_composition(path, &error))?;
    let ticket = board.task(task).map_err(|error| exit::fail(path, &error))?;
    ticket.body.parse::<TaskTemplate>().map_err(|error| {
        eprintln!("zaf {path}: {task} is not a task ticket: {error}");
        REFUSED
    })
}

/// The live ports a check reaches, built from the composition inside each step's thread.
struct LivePorts {
    composition: Composition,
    /// The manifest's `[env]`, resolved, set on every line the shell runs.
    env: BTreeMap<String, String>,
}

impl Shell for LivePorts {
    /// Runs the line through the composition's shell, at the project root, with `[env]` set.
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.composition
            .shell_in_with_env(self.composition.root(), &self.env)
            .run(line)
    }
}

impl StepPorts for LivePorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        self.composition.forge().map_err(unavailable)
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        self.composition.runner(name).map_err(unavailable)
    }

    /// Runs the action as `zaf do <name>` would from here: the same tiers, modes and log.
    fn action(&self, name: &str) -> RunOutcome {
        super::do_action::run_named(name, &[], None)
    }
}

fn unavailable(error: CompositionError) -> PortError {
    PortError::Unavailable(error.to_string())
}

fn runs_dir(composition: &Composition) -> PathBuf {
    composition.root().join(RUNS_DIR)
}

/// Where a target's record is stored: `.zaf/runs/check-<slug>.json`, one per task and
/// per pull request.
fn stored_path(composition: &Composition, target: &Target) -> PathBuf {
    runs_dir(composition).join(format!("check-{}.json", target.slug()))
}

fn store(stored: &std::path::Path, record: &CheckRecord) -> std::io::Result<()> {
    if let Some(dir) = stored.parent() {
        std::fs::create_dir_all(dir)?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n")?;
        }
    }
    let text = serde_json::to_string_pretty(record).map_err(std::io::Error::other)?;
    std::fs::write(stored, text)
}

fn show_stored(cli: &Cli, path: &str, stored: &std::path::Path) -> u8 {
    let text = match std::fs::read_to_string(stored) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("zaf {path}: no stored result at {}", stored.display());
            return REFUSED;
        }
        Err(error) => {
            eprintln!("zaf {path}: cannot read {}: {error}", stored.display());
            return BACKEND;
        }
    };
    match serde_json::from_str::<CheckRecord>(&text) {
        Ok(record) => {
            print(cli, &record);
            record.verdict.code()
        }
        Err(error) => {
            eprintln!(
                "zaf {path}: {} is not a check result: {error}",
                stored.display()
            );
            BACKEND
        }
    }
}

fn print(cli: &Cli, record: &CheckRecord) {
    if cli.json {
        match serde_json::to_string(record) {
            Ok(text) => println!("{text}"),
            Err(error) => eprintln!("zaf check: cannot print the result as JSON: {error}"),
        }
    } else {
        println!("{record}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("ZAF.toml"), manifest).expect("manifest");
        dir
    }

    const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
        [board]\nkind = \"github-issues\"\nrepo = \"owner/example\"\n\n\
        [gate]\nfull = \"true\"\ntests = \"test {name} = passes\"\n";

    #[test]
    fn the_live_shell_runs_a_line_at_the_project_root() {
        let dir = project(MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let shell = LivePorts {
            composition,
            env: BTreeMap::new(),
        };
        let passed = shell.run(&CommandLine::from("echo hello")).expect("runs");
        assert_eq!(passed.output, "hello\n");
        let failed = shell.run(&CommandLine::from("exit 4")).expect("runs");
        assert_eq!(failed.verdict, crate::ports::GateVerdict::Failed);
        let at_root = shell.run(&CommandLine::from("cat ZAF.toml")).expect("runs");
        assert!(
            at_root.output.starts_with("[project]"),
            "{}",
            at_root.output
        );
    }

    #[test]
    fn an_absent_forge_or_runner_is_unavailable() {
        let dir = project(MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let ports = LivePorts {
            composition,
            env: BTreeMap::new(),
        };
        assert!(matches!(ports.forge(), Err(PortError::Unavailable(_))));
        let missing = ports.runner(&RunnerName::from("local"));
        assert!(matches!(missing, Err(PortError::Unavailable(_))));
    }

    const SLICE: &str = "feature/0.1.1-37-46";

    /// A task ticket whose checks need no test run and no symbol: the gate line decides.
    fn ticket() -> crate::ports::Ticket {
        let body = "Goal: gate the slice.\n\
            Owned paths: src/lib.rs\n\
            Interfaces: none\n\
            Design pages: none\n\
            Architecture: the cli module wires ports; the check stays directory-agnostic\n\
            Acceptance:\n\
            \x20 - the gate runs in the slice\n\
            Tests: beside each module\n\
            Security trigger: no\n\
            Model: sonnet\n\
            Size: S\n\
            Blocked by: —\n";
        crate::ports::Ticket {
            number: IssueNumber::from(46),
            epic: Some(IssueNumber::from(37)),
            title: "check gates the slice".to_owned(),
            body: body.to_owned(),
            label: None,
        }
    }

    /// A gate that records the directory it was built for each time it runs.
    struct DirGate {
        dir: PathBuf,
        ran: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    }

    impl Gate for DirGate {
        fn run(&self, _level: crate::ports::GateLevel) -> Result<GateRun, PortError> {
            self.ran.lock().expect("lock").push(self.dir.clone());
            Ok(GateRun {
                verdict: crate::ports::GateVerdict::Passed,
                output: "ok\n".to_owned(),
            })
        }
    }

    /// A shell the fake ticket must never reach: it names no test, so a run is a failure.
    struct NoShell;

    impl Shell for NoShell {
        fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
            panic!("the fake ticket names no test to run, yet the shell ran {line:?}")
        }
    }

    /// Runs `check <task>` against fakes; returns the exit code, the result, and every gate run.
    fn check_with_fakes(
        composition: &Composition,
    ) -> (Result<(CheckRecord, String), u8>, Vec<PathBuf>) {
        let board = crate::fakes::FakeBoard::default();
        board.add_ticket(ticket());
        let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::clone(&ran);
        let gate_in = move |dir: &Path| -> Result<Box<dyn Gate>, CompositionError> {
            Ok(Box::new(DirGate {
                dir: dir.to_owned(),
                ran: Arc::clone(&recorder),
            }))
        };
        let ports = TaskPorts {
            board: &board,
            gate_in: &gate_in,
            shell_in: &|_| Box::new(NoShell),
        };
        let target = Target::Task(IssueNumber::from(46));
        let outcome =
            check_task_in_slice("check", composition, &ports, &target, IssueNumber::from(46));
        let runs = ran.lock().expect("lock").clone();
        (outcome, runs)
    }

    /// Writes the slice record the way `slice-open` leaves it: the workspace path, alone.
    fn record_workspace(root: &Path, task: u64, workspace: &Path) {
        let slices = root.join(".zaf/slices");
        std::fs::create_dir_all(&slices).expect("slices dir");
        std::fs::write(
            slices.join(task.to_string()),
            workspace.to_string_lossy().as_bytes(),
        )
        .expect("record");
    }

    #[test]
    fn check_runs_in_the_recorded_workspace() {
        let dir = project(MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        // The workspace is where the record says, not where the branch patterns would put it.
        let elsewhere = tempfile::tempdir().expect("workspace dir");
        let workspace = elsewhere.path().canonicalize().expect("absolute");
        record_workspace(dir.path(), 46, &workspace);
        let (outcome, runs) = check_with_fakes(&composition);
        let (record, line) = outcome.expect("checked");
        assert_eq!(runs, vec![workspace.clone()]);
        assert_eq!(line, format!("workspace {}", workspace.display()));
        assert_eq!(record.verdict, crate::app::check::Verdict::Clean);
    }

    const ENV_MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "owner/example"

[gate]
full = "true"
light = "printf '%s|%s' \"$CARGO_TARGET_DIR\" \"$HOME\" > seen && test -n \"$PATH\""

[env]
CARGO_TARGET_DIR = "{root}/.zaf/target"
HOME = "/overridden"
"#;

    #[test]
    fn check_runs_the_gate_with_env() {
        let dir = project(ENV_MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let elsewhere = tempfile::tempdir().expect("workspace dir");
        let workspace = elsewhere.path().canonicalize().expect("absolute");
        record_workspace(dir.path(), 46, &workspace);
        let board = crate::fakes::FakeBoard::default();
        board.add_ticket(ticket());
        let env = composition.project_env(None).expect("resolves");
        let target = Target::Task(IssueNumber::from(46));
        let (record, _) = check_task_with_board(
            "check",
            &composition,
            &board,
            &target,
            IssueNumber::from(46),
            &env,
        )
        .expect("checked");
        // The gate saw the variable at the absolute root, an `[env]` key over an inherited one,
        // and the rest of the inherited environment (PATH) still in place.
        assert_eq!(record.verdict, crate::app::check::Verdict::Clean);
        let seen = std::fs::read_to_string(workspace.join("seen")).expect("the gate ran");
        let expected = format!("{}/.zaf/target|/overridden", composition.root().display());
        assert_eq!(seen, expected);
        assert!(Path::new(seen.split('|').next().expect("value")).is_absolute());
    }

    #[test]
    fn a_command_line_runs_with_env_over_the_inherited_environment() {
        let dir = project(ENV_MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let env = composition.project_env(None).expect("resolves");
        let shell = LivePorts { composition, env };
        let line = CommandLine::from("echo \"$CARGO_TARGET_DIR $HOME\"; test -n \"$PATH\"");
        let ran = shell.run(&line).expect("runs");
        assert_eq!(ran.verdict, crate::ports::GateVerdict::Passed);
        assert!(
            ran.output.ends_with("/.zaf/target /overridden\n"),
            "{}",
            ran.output
        );
    }

    #[test]
    fn a_version_placeholder_without_a_version_names_the_key_and_the_placeholder() {
        let manifest = ENV_MANIFEST.replace("{root}/.zaf/target", "{root}/target-{version}");
        let dir = project(&manifest);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let error = composition.project_env(None).expect_err("needs a version");
        let text = error.to_string();
        assert!(text.contains("CARGO_TARGET_DIR"), "{text}");
        assert!(text.contains("{version}"), "{text}");
        let env = composition.project_env(Some("0.2.0")).expect("resolves");
        assert!(env["CARGO_TARGET_DIR"].ends_with("/target-0.2.0"));
    }

    #[test]
    fn check_without_a_record_refuses() {
        let dir = project(MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        // A workspace that predates the records sits where the patterns would put it: no record,
        // so it is refused rather than guessed at.
        std::fs::create_dir_all(dir.path().join(".zaf/workspaces").join(SLICE)).expect("old");
        let (outcome, runs) = check_with_fakes(&composition);
        assert_eq!(outcome.err(), Some(REFUSED));
        assert_eq!(
            not_open(IssueNumber::from(46)),
            "slice for #46 is not open; run zaf do slice-open 46"
        );
        assert!(runs.is_empty(), "the gate ran at {runs:?}");
        // A record whose workspace has gone is no record either.
        let gone = dir.path().join("gone");
        record_workspace(dir.path(), 46, &gone);
        let (outcome, runs) = check_with_fakes(&composition);
        assert_eq!(outcome.err(), Some(REFUSED));
        assert!(runs.is_empty(), "the gate ran at {runs:?}");
    }

    /// Lends a shared fake forge to a step's thread.
    struct SharedForge(Arc<crate::fakes::FakeForge>);

    impl Forge for SharedForge {
        fn open_pr(&self, request: &crate::ports::PrRequest) -> Result<PrNumber, PortError> {
            self.0.open_pr(request)
        }

        fn checks(&self, pr: PrNumber) -> Result<Vec<crate::ports::Check>, PortError> {
            self.0.checks(pr)
        }

        fn review_comments(
            &self,
            pr: PrNumber,
        ) -> Result<Vec<crate::ports::ReviewComment>, PortError> {
            self.0.review_comments(pr)
        }

        fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
            self.0.head(pr)
        }

        fn merged(&self, pr: PrNumber) -> Result<bool, PortError> {
            self.0.merged(pr)
        }

        fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
            self.0.last_changed_file(pr)
        }

        fn scanning_alerts(
            &self,
            pr: PrNumber,
        ) -> Result<Vec<crate::ports::ScanningAlert>, PortError> {
            self.0.scanning_alerts(pr)
        }

        fn merge(&self, pr: PrNumber) -> Result<crate::ports::PrMerge, PortError> {
            self.0.merge(pr)
        }
    }

    /// Ports whose shell fails the one step and, when told, moves the pull request's head
    /// while it runs.
    struct MovingPorts {
        forge: Arc<crate::fakes::FakeForge>,
        pr: PrNumber,
        moves_to: Option<&'static str>,
    }

    impl Shell for MovingPorts {
        fn run(&self, _line: &CommandLine) -> Result<GateRun, PortError> {
            if let Some(head) = self.moves_to {
                self.forge.set_head(self.pr, Some(head))?;
            }
            Ok(GateRun {
                verdict: crate::ports::GateVerdict::Failed,
                output: "error: something broke\n".to_owned(),
            })
        }
    }

    impl StepPorts for MovingPorts {
        fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
            Ok(Box::new(SharedForge(Arc::clone(&self.forge))))
        }

        fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
            Err(PortError::Unavailable(format!("no runner {name}")))
        }
    }

    /// A pull request on a fake forge, at `head` when one is given.
    fn pull_request(head: Option<&str>) -> (Arc<crate::fakes::FakeForge>, PrNumber) {
        let forge = Arc::new(crate::fakes::FakeForge::default());
        let pr = forge
            .open_pr(&crate::ports::PrRequest {
                slice: crate::ports::SliceName::from("feature/one-task"),
                base: crate::ports::BaseName::from("feature/one"),
                title: "#7 one task".to_owned(),
                body: String::new(),
            })
            .expect("opens");
        forge.set_head(pr, head).expect("sets the head");
        (forge, pr)
    }

    /// Runs one failing `command:lint` step over the ports.
    fn run_lint(ports: MovingPorts) -> CheckRecord {
        let dir = tempfile::tempdir().expect("temp dir");
        let pr = ports.pr;
        let settings = EngineSettings::new(dir.path().to_owned(), dir.path().join(".zaf/runs"));
        let steps = vec!["command:lint".parse::<Step>().expect("step")];
        let target = Target::Pr { pr, task: None };
        run_steps(Arc::new(ports), settings, &steps, &target, pr, false)
    }

    #[test]
    fn record_keeps_the_head_from_before_the_steps() {
        let (forge, pr) = pull_request(Some("old-head"));
        let ports = MovingPorts {
            forge: Arc::clone(&forge),
            pr,
            moves_to: Some("new-head"),
        };
        let record = run_lint(ports);
        assert_eq!(
            forge.head(pr).expect("reads"),
            Some("new-head".to_owned()),
            "the head moved during the run"
        );
        assert_eq!(record.head.as_deref(), Some("old-head"));
        assert!(record.at.is_some());
    }

    #[test]
    fn no_head_from_the_forge_keeps_the_verdict() {
        let run = |head: Option<&str>| {
            let (forge, pr) = pull_request(head);
            run_lint(MovingPorts {
                forge,
                pr,
                moves_to: None,
            })
        };
        let with_head = run(Some("abc123"));
        let without = run(None);
        assert_eq!(with_head.head.as_deref(), Some("abc123"));
        assert_eq!(without.head, None);
        assert_eq!(without.verdict, with_head.verdict);
        assert_eq!(without.verdict.code(), 1);
        // Each run kept its log in a temp project of its own, so the paths differ.
        let unlogged = |record: &CheckRecord| -> Vec<(String, String)> {
            let text = |line: &crate::app::check::RecordLine| match line.text.split_once(", log ") {
                Some((reason, _)) => reason.to_owned(),
                None => line.text.clone(),
            };
            record
                .lines
                .iter()
                .map(|line| (line.status.clone(), text(line)))
                .collect()
        };
        assert_eq!(unlogged(&without), unlogged(&with_head));
        // A forge that cannot answer at all (no such pull request) is the same: none.
        let (forge, _) = pull_request(None);
        let elsewhere = PrNumber::from(99);
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = EngineSettings::new(dir.path().to_owned(), dir.path().join(".zaf/runs"));
        let steps = vec!["command:lint".parse::<Step>().expect("step")];
        let target = Target::Pr {
            pr: elsewhere,
            task: None,
        };
        let ports = MovingPorts {
            forge,
            pr: elsewhere,
            moves_to: None,
        };
        let record = run_steps(Arc::new(ports), settings, &steps, &target, elsewhere, false);
        assert_eq!(record.head, None);
        assert_eq!(record.verdict, with_head.verdict);
    }

    #[test]
    fn a_stored_record_reads_back_with_its_exit_code() {
        let dir = project(MANIFEST);
        let composition = Composition::load_with_home(dir.path(), None).expect("loads");
        let target = Target::Pr {
            pr: PrNumber::from(5),
            task: None,
        };
        let stored = stored_path(&composition, &target);
        let record = CheckRecord {
            target: target.to_string(),
            verdict: crate::app::check::Verdict::MustFix,
            lines: Vec::new(),
            head: None,
            at: None,
        };
        store(&stored, &record).expect("stores");
        assert!(stored.ends_with(".zaf/runs/check-pr-5.json"));
        let cli = <Cli as clap::Parser>::try_parse_from(["zaf", "check", "pr:5", "--status"])
            .expect("parses");
        assert_eq!(show_stored(&cli, "check", &stored), 1);
        let absent = stored.with_file_name("check-pr-6.json");
        assert_eq!(show_stored(&cli, "check", &absent), REFUSED);
    }
}
