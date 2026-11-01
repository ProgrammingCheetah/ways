use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::app::check::step::tests::{ForgePorts, SharedForge, context, forge_with_pr};
use crate::app::check::{Shell, StepPorts};
use crate::fakes::FakeForge;
use crate::manifest::{CommandLine, RunnerName};
use crate::ports::{
    Check, CheckName, Forge, GateRun, PrMerge, PrNumber, PrRequest, ReviewComment, Runner,
    ScanningAlert,
};

fn source() -> FindingSource {
    FindingSource::from("ci")
}

fn check(name: &str, state: CheckState) -> Check {
    Check {
        name: CheckName::from(name),
        state,
    }
}

/// A forge whose `checks` answers come from a script, the last one repeating, and that
/// counts the polls; everything else is the shared fake.
struct ScriptedForge {
    inner: SharedForge,
    script: Mutex<VecDeque<Vec<Check>>>,
    polls: Arc<Mutex<usize>>,
}

impl Forge for ScriptedForge {
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        self.inner.open_pr(request)
    }

    fn checks(&self, _pr: PrNumber) -> Result<Vec<Check>, PortError> {
        *self.polls.lock().expect("polls") += 1;
        let mut script = self.script.lock().expect("script");
        let answer = if script.len() > 1 {
            script.pop_front().unwrap_or_default()
        } else {
            script.front().cloned().unwrap_or_default()
        };
        Ok(answer)
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        self.inner.review_comments(pr)
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.inner.head(pr)
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        self.inner.scanning_alerts(pr)
    }

    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError> {
        self.inner.merge(pr)
    }
}

struct ScriptedPorts {
    forge: Arc<FakeForge>,
    script: Vec<Vec<Check>>,
    polls: Arc<Mutex<usize>>,
}

impl Shell for ScriptedPorts {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        ForgePorts(Arc::clone(&self.forge)).run(line)
    }
}

impl StepPorts for ScriptedPorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        Ok(Box::new(ScriptedForge {
            inner: SharedForge(Arc::clone(&self.forge)),
            script: Mutex::new(self.script.clone().into()),
            polls: Arc::clone(&self.polls),
        }))
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        ForgePorts(Arc::clone(&self.forge)).runner(name)
    }
}

/// A scripted run of the step: the outcome, how many times the forge was asked and how
/// long the step took.
fn scripted(
    script: Vec<Vec<Check>>,
    grace: Duration,
    wait: Duration,
) -> (Result<Vec<Finding>, PortError>, usize, Duration) {
    let (forge, pr) = forge_with_pr();
    let polls = Arc::new(Mutex::new(0));
    let ports = ScriptedPorts {
        forge,
        script,
        polls: Arc::clone(&polls),
    };
    let mut step_context = context(pr, wait);
    step_context.grace = grace;
    let started = Instant::now();
    let outcome = run(&source(), &ports, &step_context);
    let took = started.elapsed();
    let polled = *polls.lock().expect("polls");
    (outcome, polled, took)
}

#[test]
fn an_empty_list_is_pending_until_a_check_registers_and_passes() {
    let script = vec![vec![], vec![], vec![check("gate", CheckState::Passed)]];
    let (outcome, polled, _) = scripted(script, Duration::from_secs(5), Duration::from_secs(5));
    assert_eq!(outcome, Ok(Vec::new()));
    assert_eq!(polled, 3);
}

#[test]
fn an_empty_list_is_clean_only_after_the_grace_window() {
    let grace = Duration::from_millis(60);
    let (outcome, polled, took) = scripted(vec![vec![]], grace, Duration::from_secs(5));
    assert_eq!(outcome, Ok(Vec::new()));
    assert!(took >= grace, "returned after {took:?}, before the grace");
    assert!(polled > 1, "polled {polled} times");
}

#[test]
fn a_check_that_registers_and_fails_is_one_must_fix() {
    let script = vec![vec![], vec![check("gate", CheckState::Failed)]];
    let (outcome, polled, _) = scripted(script, Duration::from_secs(5), Duration::from_secs(5));
    let findings = outcome.expect("the step finishes");
    assert_eq!(polled, 2);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::MustFix);
    assert_eq!(findings[0].message, "check `gate` failed");
}

#[test]
fn a_listed_check_that_stays_pending_runs_into_the_deadline() {
    let script = vec![vec![check("gate", CheckState::Pending)]];
    let (outcome, _, _) = scripted(script, Duration::ZERO, Duration::from_millis(40));
    assert!(
        matches!(outcome, Err(PortError::Unavailable(_))),
        "{outcome:?}"
    );
}

#[test]
fn without_grace_an_empty_list_is_clean_at_once() {
    let (outcome, polled, _) = scripted(vec![vec![]], Duration::ZERO, Duration::from_secs(5));
    assert_eq!(outcome, Ok(Vec::new()));
    assert_eq!(polled, 1);
}
