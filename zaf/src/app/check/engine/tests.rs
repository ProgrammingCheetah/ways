use std::sync::Mutex;

use super::*;
use crate::domain::{Location, Profile, RoleName};
use crate::fakes::{FakeForge, FakeRunner};
use crate::manifest::CommandLine;
use crate::ports::{
    BaseName, Check, CheckName, CheckState, GateRun, GateVerdict, PrMerge, PrRequest,
    ReviewComment, RunOutput, RunRequest, ScanningAlert, SliceName,
};

/// Lends a shared fake forge to a step's thread.
struct SharedForge(Arc<FakeForge>);

impl Forge for SharedForge {
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        self.0.open_pr(request)
    }

    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        self.0.checks(pr)
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        self.0.review_comments(pr)
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.0.head(pr)
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        self.0.scanning_alerts(pr)
    }

    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError> {
        self.0.merge(pr)
    }
}

/// Lends a shared fake runner to a step's thread.
struct SharedRunner(Arc<FakeRunner>);

impl Runner for SharedRunner {
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError> {
        self.0.run(request)
    }
}

type ShellScript = Box<dyn Fn(&CommandLine) -> Result<GateRun, PortError> + Send + Sync>;

/// The fakes behind the engine's ports, with a scripted shell.
struct FakePorts {
    forge: Arc<FakeForge>,
    runner: Arc<FakeRunner>,
    shell: ShellScript,
    shell_lines: Mutex<Vec<CommandLine>>,
}

impl FakePorts {
    fn new(shell: ShellScript) -> Self {
        Self {
            forge: Arc::new(FakeForge::default()),
            runner: Arc::new(FakeRunner::default()),
            shell,
            shell_lines: Mutex::new(Vec::new()),
        }
    }
}

impl Shell for FakePorts {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.shell_lines.lock().expect("lock").push(line.clone());
        (self.shell)(line)
    }
}

impl StepPorts for FakePorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        Ok(Box::new(SharedForge(Arc::clone(&self.forge))))
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        if name.as_ref() != "local" {
            return Err(PortError::Unavailable(format!("no runner {name}")));
        }
        Ok(Box::new(SharedRunner(Arc::clone(&self.runner))))
    }
}

fn passing() -> GateRun {
    GateRun {
        verdict: GateVerdict::Passed,
        output: String::new(),
    }
}

fn sleeping_shell(delay: Duration) -> ShellScript {
    Box::new(move |_| {
        thread::sleep(delay);
        Ok(passing())
    })
}

fn steps(texts: &[&str]) -> Vec<Step> {
    texts
        .iter()
        .map(|text| text.parse().expect("step parses"))
        .collect()
}

fn engine(ports: &Arc<FakePorts>, dir: &tempfile::TempDir, timeout: Duration) -> Engine {
    let settings = EngineSettings {
        root: dir.path().to_owned(),
        runs_dir: dir.path().join(".zaf/runs"),
        timeout,
        poll: Duration::from_millis(5),
    };
    let shared: Arc<dyn StepPorts> = Arc::clone(ports) as Arc<dyn StepPorts>;
    Engine::new(shared, settings)
}

fn open_pr(forge: &FakeForge) -> PrNumber {
    let request = PrRequest {
        slice: SliceName::from("feature/one-task"),
        base: BaseName::from("feature/one"),
        title: "#7 one task".to_owned(),
        body: String::new(),
    };
    forge.open_pr(&request).expect("opens")
}

fn record_report(runner: &FakeRunner, role: &str, transcript: &str) {
    let output = RunOutput {
        exit_code: 0,
        transcript: transcript.to_owned(),
    };
    runner.record(RoleName::from(role), output);
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

#[test]
fn independent_steps_run_in_parallel() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::from_millis(300))));
    let dir = temp_dir();
    let engine = engine(&ports, &dir, Duration::from_secs(10));
    let started = Instant::now();
    let report = engine.run(
        &steps(&["command:first", "command:second", "command:third"]),
        PrNumber::from(1),
        false,
    );
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(800), "took {elapsed:?}");
    assert_eq!(ports.shell_lines.lock().expect("lock").len(), 3);
    assert!(
        report
            .steps
            .iter()
            .all(|result| result.status == StepStatus::Ran)
    );
    assert_eq!(report.verdict(), Verdict::Clean);
    assert_eq!(report.verdict().code(), 0);
}

#[test]
fn a_step_past_the_timeout_fails_the_check_with_exit_two() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::from_secs(3))));
    let dir = temp_dir();
    let engine = engine(&ports, &dir, Duration::from_millis(100));
    let started = Instant::now();
    let report = engine.run(&steps(&["command:slow"]), PrNumber::from(1), false);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(report.steps[0].status, StepStatus::TimedOut);
    assert_eq!(report.verdict(), Verdict::StepFailed);
    assert_eq!(report.verdict().code(), 2);
}

#[test]
fn a_runner_report_becomes_must_and_note_findings() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::ZERO)));
    let pr = open_pr(&ports.forge);
    record_report(
        &ports.runner,
        "critique",
        "Looked at the diff.\nMUST: src/export.rs:12 the empty list is dropped\nNOTE: src/export.rs:30 name the constant\n",
    );
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_secs(10)).run(
        &steps(&["runner:local:critique"]),
        pr,
        false,
    );
    let source = FindingSource::from("runner:local:critique");
    let severities: Vec<(Severity, &str, Option<Location>)> = report
        .findings
        .iter()
        .map(|checked| {
            let finding = &checked.finding;
            assert_eq!(finding.source, source);
            (
                finding.severity,
                finding.message.as_str(),
                finding.location.clone(),
            )
        })
        .collect();
    let location = |line| {
        Some(Location {
            path: "src/export.rs".to_owned(),
            line: Some(line),
        })
    };
    let expected = [
        (Severity::MustFix, "the empty list is dropped", location(12)),
        (Severity::Advisory, "name the constant", location(30)),
    ];
    assert_eq!(severities, expected);
    assert_eq!(report.verdict(), Verdict::MustFix);
    assert_eq!(report.verdict().code(), 1);

    let requests = ports.runner.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].profile, Profile::Review);
    let brief_path = requests[0].dir.join(step::BRIEF_FILE);
    assert!(
        requests[0]
            .prompt
            .contains(&brief_path.display().to_string())
    );
    let brief = std::fs::read_to_string(&brief_path).expect("brief written");
    assert!(brief.contains(&pr.to_string()));
    assert!(brief.contains("MUST: <path>:<line> <title>"));
    assert!(requests[0].dir.starts_with(dir.path().join(".zaf/runs")));
}

#[test]
fn the_same_finding_from_two_sources_is_reported_once() {
    let ports = Arc::new(FakePorts::new(Box::new(|_| {
        let gate_run = GateRun {
            verdict: GateVerdict::Failed,
            output: "MUST: src/parse.rs:41 Unchecked input reaches the shell.\n".to_owned(),
        };
        Ok(gate_run)
    })));
    let pr = open_pr(&ports.forge);
    record_report(
        &ports.runner,
        "critique",
        "MUST: src/parse.rs:40 unchecked input reaches the shell\n",
    );
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_secs(10)).run(
        &steps(&["runner:local:critique", "command:lint"]),
        pr,
        false,
    );
    assert_eq!(report.findings.len(), 1);
    let checked = &report.findings[0];
    assert_eq!(
        checked.finding.source,
        FindingSource::from("runner:local:critique")
    );
    assert_eq!(checked.also_found_by, [FindingSource::from("command:lint")]);
}

#[test]
fn a_flagged_step_is_skipped_for_an_unflagged_task_and_run_for_a_flagged_one() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::ZERO)));
    let pr = open_pr(&ports.forge);
    let configured = steps(&["runner:local:security?flagged"]);
    let dir = temp_dir();
    let engine = engine(&ports, &dir, Duration::from_secs(10));

    let unflagged = engine.run(&configured, pr, false);
    assert_eq!(unflagged.steps[0].status, StepStatus::Skipped);
    assert!(ports.runner.requests().is_empty());
    assert_eq!(unflagged.verdict(), Verdict::Clean);

    record_report(
        &ports.runner,
        "security",
        "NOTE: src/net.rs:3 pin the version\n",
    );
    let flagged = engine.run(&configured, pr, true);
    assert_eq!(flagged.steps[0].status, StepStatus::Ran);
    assert_eq!(ports.runner.requests().len(), 1);
    assert_eq!(flagged.findings.len(), 1);
    let brief = std::fs::read_to_string(ports.runner.requests()[0].dir.join(step::BRIEF_FILE))
        .expect("brief");
    assert!(brief.starts_with("# Security review brief"));
}

#[test]
fn ci_turns_each_failed_check_into_a_must_fix() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::ZERO)));
    let pr = open_pr(&ports.forge);
    let checks = vec![
        Check {
            name: CheckName::from("build"),
            state: CheckState::Passed,
        },
        Check {
            name: CheckName::from("test"),
            state: CheckState::Failed,
        },
    ];
    ports.forge.set_checks(pr, checks).expect("set");
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_secs(10)).run(&steps(&["ci"]), pr, false);
    assert_eq!(report.findings.len(), 1);
    let finding = &report.findings[0].finding;
    assert_eq!(finding.source, FindingSource::from("ci"));
    assert_eq!(finding.severity, Severity::MustFix);
    assert!(finding.message.contains("test"));
    assert_eq!(report.verdict(), Verdict::MustFix);
}

#[test]
fn ci_waits_on_pending_checks_and_gives_up_at_the_deadline() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::ZERO)));
    let pr = open_pr(&ports.forge);
    let pending = vec![Check {
        name: CheckName::from("build"),
        state: CheckState::Pending,
    }];
    ports.forge.set_checks(pr, pending).expect("set");
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_millis(100)).run(&steps(&["ci"]), pr, false);
    assert!(matches!(
        report.steps[0].status,
        StepStatus::Failed(_) | StepStatus::TimedOut
    ));
    assert_eq!(report.verdict().code(), 2);
}

#[test]
fn an_unavailable_runner_or_failing_run_is_a_failed_step() {
    let ports = Arc::new(FakePorts::new(sleeping_shell(Duration::ZERO)));
    let pr = open_pr(&ports.forge);
    let failing = RunOutput {
        exit_code: 3,
        transcript: String::new(),
    };
    ports.runner.record(RoleName::from("critique"), failing);
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_secs(10)).run(
        &steps(&["runner:offline:critique", "runner:local:critique"]),
        pr,
        false,
    );
    assert!(matches!(report.steps[0].status, StepStatus::Failed(_)));
    assert!(matches!(report.steps[1].status, StepStatus::Failed(_)));
    assert_eq!(report.verdict(), Verdict::StepFailed);
}

#[test]
fn a_failing_command_without_report_lines_is_one_must_fix() {
    let ports = Arc::new(FakePorts::new(Box::new(|_| {
        let gate_run = GateRun {
            verdict: GateVerdict::Failed,
            output: "error: something broke\n".to_owned(),
        };
        Ok(gate_run)
    })));
    let dir = temp_dir();
    let report = engine(&ports, &dir, Duration::from_secs(10)).run(
        &steps(&["command:lint"]),
        PrNumber::from(1),
        false,
    );
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].finding.severity, Severity::MustFix);
    assert_eq!(report.verdict(), Verdict::MustFix);
}
