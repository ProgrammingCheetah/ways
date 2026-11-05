use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::app::actions::{NOT_RUN_EXIT, Tier};
use crate::app::check::step::Step;
use crate::app::check::{Engine, EngineSettings, Shell, StepStatus, Verdict};
use crate::manifest::{CommandLine, RunnerName};
use crate::ports::{Forge, GateRun, PrNumber, Runner};

/// Ports that reach no backend and answer every action with one scripted outcome.
struct ActionPorts(RunOutcome);

impl Shell for ActionPorts {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        Err(PortError::Unavailable(format!("no shell for {line}")))
    }
}

impl StepPorts for ActionPorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        Err(PortError::Unavailable("no forge".to_owned()))
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        Err(PortError::Unavailable(format!("no runner {name}")))
    }

    fn action(&self, _name: &str) -> RunOutcome {
        self.0.clone()
    }
}

fn source() -> FindingSource {
    FindingSource::from("action:nodes-check")
}

fn outcome(result: RunResult, exit: u8, meaning: Option<&str>) -> RunOutcome {
    RunOutcome {
        result,
        name: "nodes-check".to_owned(),
        tier: Some(Tier::Project),
        exit,
        meaning: meaning.map(str::to_owned),
        seconds: 0.5,
        log: Some(PathBuf::from(
            "/proj/.zaf/logs/20261105T101500Z_nodes-check.log",
        )),
    }
}

fn run_with(outcome: RunOutcome) -> Result<Vec<Finding>, PortError> {
    run(&source(), "nodes-check", &ActionPorts(outcome))
}

#[test]
fn action_step_clean_on_zero() {
    let findings = run_with(outcome(RunResult::Ok, 0, None)).expect("a clean step");
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn action_step_finding_names_the_log() {
    let failed = outcome(RunResult::Fail, 3, Some("a node chart is stale"));
    let findings = run_with(failed).expect("a FAIL is a finding, not a failed step");
    assert_eq!(findings.len(), 1, "{findings:?}");
    let finding = &findings[0];
    assert_eq!(finding.severity, Severity::MustFix);
    assert_eq!(finding.source, source());
    for part in [
        "nodes-check",
        "a node chart is stale",
        "/proj/.zaf/logs/20261105T101500Z_nodes-check.log",
    ] {
        assert!(
            finding.message.contains(part),
            "{part}: {}",
            finding.message
        );
    }
}

#[test]
fn missing_action_step_fails() {
    let missing = RunOutcome::not_run("nodes-check", None, NO_SUCH_ACTION.to_owned());
    let error = run_with(missing).expect_err("a missing action fails the step");
    assert!(
        error.to_string().contains("missing action nodes-check"),
        "{error}"
    );
}

#[test]
fn not_run_action_step_fails() {
    let refusals = [
        RunOutcome::not_run(
            "nodes-check",
            Some(Tier::Project),
            "not permitted for team".to_owned(),
        ),
        RunOutcome::not_run("nodes-check", None, "depth 8 reached: a > b".to_owned()),
    ];
    for refused in refusals {
        let reason = refused.meaning.clone().expect("a reason");
        assert_eq!(refused.exit, NOT_RUN_EXIT);
        let error = run_with(refused).expect_err("a refusal fails the step");
        let why = error.to_string();
        assert!(why.contains(&reason), "{why}");
        assert!(!why.contains("missing action"), "{why}");
    }
}

/// Through the engine, a refusal is a failed step with no finding, and a `FAIL` is a
/// must-fix finding with the step run.
#[test]
fn the_engine_tells_a_failed_step_from_a_finding() {
    let steps = ["action:nodes-check".parse::<Step>().expect("step")];
    let settings = || {
        let mut settings = EngineSettings::new(PathBuf::from("."), PathBuf::from("."));
        settings.timeout = Duration::from_secs(5);
        settings
    };
    let pr = PrNumber::from(1);

    let failed = outcome(RunResult::Fail, 1, Some("undeclared"));
    let ports = Arc::new(ActionPorts(failed));
    let report = Engine::new(ports, settings()).run(&steps, pr, false);
    assert_eq!(report.steps[0].status, StepStatus::Ran);
    assert_eq!(report.verdict(), Verdict::MustFix);
    assert_eq!(report.findings.len(), 1);

    let refused = RunOutcome::not_run("nodes-check", None, NO_SUCH_ACTION.to_owned());
    let ports = Arc::new(ActionPorts(refused));
    let report = Engine::new(ports, settings()).run(&steps, pr, false);
    assert!(matches!(report.steps[0].status, StepStatus::Failed(_)));
    assert_eq!(report.verdict(), Verdict::StepFailed);
    assert!(report.findings.is_empty());
}
