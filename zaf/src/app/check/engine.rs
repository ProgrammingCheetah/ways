//! The step engine: every step in its own thread under one timeout, the findings
//! collected, deduplicated and judged.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::step::{self, Step, StepContext};
use super::{CheckedFinding, Shell, Verdict, dedupe};
use crate::domain::{Finding, FindingSource, Severity};
use crate::manifest::RunnerName;
use crate::ports::{Forge, PortError, PrNumber, Runner};

/// How long a step may run before the check gives up on it. The manifest's
/// step list carries no timeout of its own, so one bound covers every step.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How often a `ci` step looks at the forge's checks.
pub const CI_POLL: Duration = Duration::from_secs(30);

/// The ports a step reaches, built inside the step's own thread.
///
/// A custom shape: the composition hands out boxed ports that are not `Send`,
/// so each thread asks for its own instead of sharing one.
pub trait StepPorts: Shell + Send + Sync {
    /// The forge; an absent one is [`PortError::Unavailable`].
    fn forge(&self) -> Result<Box<dyn Forge>, PortError>;

    /// The runner `[runners.<name>]` declares; an undeclared one is [`PortError::Unavailable`].
    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError>;
}

/// Where the engine runs and how patient it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSettings {
    /// The project root.
    pub root: PathBuf,
    /// Where runs leave their files, `.zaf/runs`.
    pub runs_dir: PathBuf,
    /// The bound on every step.
    pub timeout: Duration,
    /// How often `ci` polls.
    pub poll: Duration,
}

impl EngineSettings {
    /// The defaults for a project: [`STEP_TIMEOUT`] and [`CI_POLL`].
    pub fn new(root: PathBuf, runs_dir: PathBuf) -> Self {
        Self {
            root,
            runs_dir,
            timeout: STEP_TIMEOUT,
            poll: CI_POLL,
        }
    }
}

/// How one step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    /// It ran to its findings.
    Ran,
    /// `?flagged` on a task that is not security-flagged.
    Skipped,
    /// It could not finish, and why.
    Failed(String),
    /// It outlived the timeout.
    TimedOut,
}

/// One step's line in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// The step as configured.
    pub step: Step,
    /// How it ended.
    pub status: StepStatus,
}

/// A finished pull request check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Every step, in configured order.
    pub steps: Vec<StepResult>,
    /// The findings, deduplicated, in step order.
    pub findings: Vec<CheckedFinding>,
}

impl CheckReport {
    /// A failed or timed-out step outranks a must-fix: the result is incomplete.
    pub fn verdict(&self) -> Verdict {
        let step_failed = self
            .steps
            .iter()
            .any(|result| matches!(result.status, StepStatus::Failed(_) | StepStatus::TimedOut));
        let must_fix = self
            .findings
            .iter()
            .any(|checked| checked.finding.severity == Severity::MustFix);
        if step_failed {
            Verdict::StepFailed
        } else if must_fix {
            Verdict::MustFix
        } else {
            Verdict::Clean
        }
    }
}

/// Runs a pull request's steps.
pub struct Engine {
    ports: Arc<dyn StepPorts>,
    settings: EngineSettings,
}

type StepMessage = (usize, Result<Vec<Finding>, PortError>);

impl Engine {
    /// An engine over the given ports.
    pub fn new(ports: Arc<dyn StepPorts>, settings: EngineSettings) -> Self {
        Self { ports, settings }
    }

    /// Runs every step that applies in parallel and waits for each, at most the timeout.
    ///
    /// A step still running at the deadline is reported [`StepStatus::TimedOut`]
    /// and left to finish on its own; its result is discarded.
    pub fn run(&self, steps: &[Step], pr: PrNumber, flagged: bool) -> CheckReport {
        let started = Instant::now();
        let deadline = started + self.settings.timeout;
        let run_dir = self.settings.runs_dir.join(format!("check-{}", stamp()));
        let mut statuses: Vec<Option<StepStatus>> = vec![None; steps.len()];
        let (sender, receiver) = mpsc::channel::<StepMessage>();
        for (index, step) in steps.iter().enumerate() {
            if step.flagged_only && !flagged {
                statuses[index] = Some(StepStatus::Skipped);
                continue;
            }
            let context = StepContext {
                pr,
                root: self.settings.root.clone(),
                dir: run_dir.join(format!("step-{index}")),
                deadline,
                poll: self.settings.poll,
            };
            let ports = Arc::clone(&self.ports);
            let step = step.clone();
            let sender = sender.clone();
            thread::spawn(move || {
                let outcome = step::run(&step, ports.as_ref(), &context);
                // The engine may have stopped listening after a timeout; nothing to do then.
                let _ = sender.send((index, outcome));
            });
        }
        drop(sender);

        let mut found: Vec<Vec<Finding>> = vec![Vec::new(); steps.len()];
        while statuses.iter().any(Option::is_none) {
            let left = deadline.saturating_duration_since(Instant::now());
            let (index, outcome) = match receiver.recv_timeout(left) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    for status in statuses.iter_mut().filter(|status| status.is_none()) {
                        *status = Some(StepStatus::Failed(
                            "the step stopped without a result".to_owned(),
                        ));
                    }
                    break;
                }
            };
            statuses[index] = Some(match outcome {
                Ok(findings) => {
                    found[index] = findings;
                    StepStatus::Ran
                }
                Err(error) => StepStatus::Failed(error.to_string()),
            });
        }

        let results = steps
            .iter()
            .zip(statuses)
            .map(|(step, status)| StepResult {
                step: step.clone(),
                status: status.unwrap_or(StepStatus::TimedOut),
            })
            .collect();
        CheckReport {
            steps: results,
            findings: dedupe(found.into_iter().flatten().collect()),
        }
    }
}

impl StepResult {
    /// The step's source.
    pub fn source(&self) -> FindingSource {
        self.step.source()
    }
}

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

#[cfg(test)]
mod tests;
